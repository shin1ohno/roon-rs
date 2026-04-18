use std::collections::HashMap;
use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::MooError;
use crate::message::{MooBody, MooMessage, MooVerb};
use crate::{parse, serialize};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = SplitSink<WsStream, WsMessage>;
type WsSource = SplitStream<WsStream>;

/// Callback slot for a pending request.
enum RequestSlot {
    /// One-shot request expecting a single COMPLETE response.
    OneShot(oneshot::Sender<MooMessage>),
    /// Subscription expecting multiple CONTINUE messages and a final COMPLETE.
    Subscription(mpsc::Sender<MooMessage>),
}

/// Handler for incoming REQUEST messages from the server.
pub type ServiceHandler = Arc<dyn Fn(MooMessage, ResponseSender) + Send + Sync>;

/// Allows service handlers to send CONTINUE/COMPLETE responses back.
#[derive(Clone)]
pub struct ResponseSender {
    sink: mpsc::Sender<WsMessage>,
    request_id: u32,
}

impl ResponseSender {
    pub async fn send_complete(
        &self,
        status: &str,
        body: Option<serde_json::Value>,
    ) -> Result<(), MooError> {
        let msg = MooMessage {
            verb: MooVerb::Complete,
            name: status.to_string(),
            request_id: self.request_id,
            headers: HashMap::new(),
            body: body.map(MooBody::Json),
        };
        let raw = serialize(&msg);
        self.sink
            .send(WsMessage::Binary(raw.into()))
            .await
            .map_err(|_| MooError::ConnectionClosed)
    }

    pub async fn send_continue(
        &self,
        status: &str,
        body: Option<serde_json::Value>,
    ) -> Result<(), MooError> {
        let msg = MooMessage {
            verb: MooVerb::Continue,
            name: status.to_string(),
            request_id: self.request_id,
            headers: HashMap::new(),
            body: body.map(MooBody::Json),
        };
        let raw = serialize(&msg);
        self.sink
            .send(WsMessage::Binary(raw.into()))
            .await
            .map_err(|_| MooError::ConnectionClosed)
    }
}

/// A MOO protocol connection over WebSocket.
///
/// Provides request/response and subscription messaging with automatic
/// heartbeat management and bidirectional dispatch.
pub struct MooConnection {
    /// Channel to send outgoing WebSocket frames.
    ws_tx: mpsc::Sender<WsMessage>,
    /// Next request ID to assign.
    next_request_id: Arc<Mutex<u32>>,
    /// Pending requests awaiting responses.
    pending: Arc<Mutex<HashMap<u32, RequestSlot>>>,
    /// Handle to the background dispatch task.
    task_handle: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for MooConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MooConnection")
            .field("alive", &!self.task_handle.is_finished())
            .finish()
    }
}

impl MooConnection {
    /// Connect to a Roon Core's MOO endpoint.
    ///
    /// `url` should be `ws://<host>:<port>/api`.
    /// `service_handlers` maps service names to handler functions for incoming REQUESTs.
    pub async fn connect(
        url: &str,
        service_handlers: HashMap<String, ServiceHandler>,
    ) -> Result<Self, MooError> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| MooError::WebSocket(e.to_string()))?;

        let (ws_sink, ws_source) = ws_stream.split();

        let pending: Arc<Mutex<HashMap<u32, RequestSlot>>> = Arc::new(Mutex::new(HashMap::new()));

        // Channel for outgoing WS frames (from send_request, heartbeat, service responses)
        let (ws_tx, ws_rx) = mpsc::channel::<WsMessage>(64);

        let task_handle = tokio::spawn(dispatch_loop(
            ws_sink,
            ws_source,
            ws_rx,
            pending.clone(),
            ws_tx.clone(),
            service_handlers,
        ));

        Ok(MooConnection {
            ws_tx,
            next_request_id: Arc::new(Mutex::new(0)),
            pending,
            task_handle,
        })
    }

    /// Send a one-shot REQUEST and wait for the COMPLETE response.
    pub async fn send_request(
        &self,
        name: &str,
        body: Option<serde_json::Value>,
    ) -> Result<MooMessage, MooError> {
        let request_id = {
            let mut id = self.next_request_id.lock().await;
            let current = *id;
            *id += 1;
            current
        };

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id, RequestSlot::OneShot(tx));
        }

        let msg = MooMessage {
            verb: MooVerb::Request,
            name: name.to_string(),
            request_id,
            headers: HashMap::new(),
            body: body.map(MooBody::Json),
        };
        let raw = serialize(&msg);
        self.ws_tx
            .send(WsMessage::Binary(raw.into()))
            .await
            .map_err(|_| MooError::ConnectionClosed)?;

        rx.await.map_err(|_| MooError::ConnectionClosed)
    }

    /// Send a subscription REQUEST and return a receiver for CONTINUE messages.
    ///
    /// The receiver yields each CONTINUE message. When the server sends COMPLETE
    /// or the connection closes, the receiver stream ends.
    pub async fn subscribe(
        &self,
        name: &str,
        body: serde_json::Value,
    ) -> Result<mpsc::Receiver<MooMessage>, MooError> {
        let request_id = {
            let mut id = self.next_request_id.lock().await;
            let current = *id;
            *id += 1;
            current
        };

        let (tx, rx) = mpsc::channel(32);
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id, RequestSlot::Subscription(tx));
        }

        let msg = MooMessage {
            verb: MooVerb::Request,
            name: name.to_string(),
            request_id,
            headers: HashMap::new(),
            body: Some(MooBody::Json(body)),
        };
        let raw = serialize(&msg);
        self.ws_tx
            .send(WsMessage::Binary(raw.into()))
            .await
            .map_err(|_| MooError::ConnectionClosed)?;

        Ok(rx)
    }

    /// Close the connection gracefully.
    pub async fn close(self) {
        let _ = self.ws_tx.send(WsMessage::Close(None)).await;
        // Abort the dispatch task
        self.task_handle.abort();
        // Clean up pending requests
        let mut pending = self.pending.lock().await;
        pending.clear();
    }

    /// Check if the connection dispatch task is still running.
    pub fn is_alive(&self) -> bool {
        !self.task_handle.is_finished()
    }
}

/// Background task that manages the WebSocket connection.
async fn dispatch_loop(
    mut ws_sink: WsSink,
    mut ws_source: WsSource,
    mut outgoing_rx: mpsc::Receiver<WsMessage>,
    pending: Arc<Mutex<HashMap<u32, RequestSlot>>>,
    ws_tx: mpsc::Sender<WsMessage>,
    service_handlers: HashMap<String, ServiceHandler>,
) {
    // Roon Core closes idle MOO connections after ~0.5-7s with no client
    // traffic. The WebSocket-level Ping it sent previously was too slow (10s)
    // and Roon does not count it as liveness — Roon watches for MOO messages.
    // Fix: send a `com.roonlabs.ping:1/ping` REQUEST every 2s, with the first
    // tick delayed by 3s so the registry handshake completes first.
    let ping_period = std::time::Duration::from_secs(2);
    let mut ping_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + std::time::Duration::from_secs(3),
        ping_period,
    );
    let mut is_alive = true;
    // Ping request IDs are reserved at the top of the u32 space so they
    // cannot collide with the 0-counting-up IDs that send_request hands out.
    let mut ping_req_id: u32 = u32::MAX;

    loop {
        tokio::select! {
            // Heartbeat tick — send a MOO ping to keep the Core happy.
            _ = ping_interval.tick() => {
                if !is_alive {
                    tracing::warn!("MOO heartbeat timeout: no traffic within window");
                    break;
                }
                is_alive = false;
                let ping = MooMessage {
                    verb: MooVerb::Request,
                    name: "com.roonlabs.ping:1/ping".to_string(),
                    request_id: ping_req_id,
                    headers: HashMap::new(),
                    body: None,
                };
                ping_req_id = ping_req_id.wrapping_sub(1);
                let raw = serialize(&ping);
                if ws_sink.send(WsMessage::Binary(raw.into())).await.is_err() {
                    break;
                }
            }

            // Outgoing messages from send_request/subscribe/service handlers
            Some(msg) = outgoing_rx.recv() => {
                if ws_sink.send(msg).await.is_err() {
                    break;
                }
            }

            // Incoming WebSocket messages
            Some(result) = ws_source.next() => {
                match result {
                    Ok(WsMessage::Binary(data)) => {
                        // Any inbound binary proves the connection is live.
                        is_alive = true;
                        if data.is_empty() {
                            // Roon occasionally sends empty binary frames.
                            // Not an error — do not log at warn!.
                            tracing::trace!("empty binary frame from server");
                            continue;
                        }
                        match parse(&data) {
                            Ok(msg) => {
                                handle_incoming(
                                    msg,
                                    &pending,
                                    &ws_tx,
                                    &service_handlers,
                                ).await;
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse MOO message: {}", e);
                            }
                        }
                    }
                    Ok(WsMessage::Pong(_)) => {
                        is_alive = true;
                    }
                    Ok(WsMessage::Ping(data)) => {
                        // Mirror any server-originated WS Ping back as Pong.
                        // Tokio-tungstenite's split streams do NOT auto-pong,
                        // so we have to.
                        if ws_sink.send(WsMessage::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Ok(WsMessage::Close(_)) => {
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {
                        // Text frames and other non-binary messages are not
                        // part of the MOO wire protocol — ignore.
                    }
                }
            }

            else => break,
        }
    }

    // Clean up: invoke all pending callbacks with no data (signal disconnection)
    let mut pending = pending.lock().await;
    pending.clear();
}

/// Handle a parsed incoming MOO message.
async fn handle_incoming(
    msg: MooMessage,
    pending: &Arc<Mutex<HashMap<u32, RequestSlot>>>,
    ws_tx: &mpsc::Sender<WsMessage>,
    service_handlers: &HashMap<String, ServiceHandler>,
) {
    match msg.verb {
        MooVerb::Request => {
            // Incoming REQUEST from server — dispatch to service handler
            let service = msg.service().unwrap_or("").to_string();
            let response_sender = ResponseSender {
                sink: ws_tx.clone(),
                request_id: msg.request_id,
            };

            if let Some(handler) = service_handlers.get(&service) {
                handler(msg, response_sender);
            } else {
                // Unknown service — send InvalidRequest
                let _ = response_sender
                    .send_complete(
                        "InvalidRequest",
                        Some(serde_json::json!({"error": format!("unknown service: {}", service)})),
                    )
                    .await;
            }
        }
        MooVerb::Continue => {
            let mut pending = pending.lock().await;
            match pending.get(&msg.request_id) {
                Some(RequestSlot::Subscription(tx)) => {
                    let request_id = msg.request_id;
                    if tx.send(msg).await.is_err() {
                        // Receiver dropped — remove from pending
                        pending.remove(&request_id);
                    }
                }
                Some(RequestSlot::OneShot(_)) => {
                    // CONTINUE on a one-shot: upgrade to subscription behavior.
                    // Remove the oneshot and send the message through it as a one-shot
                    // response, since the caller may not expect CONTINUE on a one-shot.
                    // In practice, registration uses CONTINUE Registered on a request
                    // that was sent as send_request — so we deliver the first CONTINUE
                    // and close the oneshot.
                    if let Some(RequestSlot::OneShot(tx)) = pending.remove(&msg.request_id) {
                        let _ = tx.send(msg);
                    }
                }
                None => {
                    tracing::warn!(
                        "CONTINUE for unknown request_id {}: closing connection",
                        msg.request_id
                    );
                }
            }
        }
        MooVerb::Complete => {
            let mut pending = pending.lock().await;
            match pending.remove(&msg.request_id) {
                Some(RequestSlot::OneShot(tx)) => {
                    let _ = tx.send(msg);
                }
                Some(RequestSlot::Subscription(tx)) => {
                    // Final message on subscription — send it and drop the sender
                    let _ = tx.send(msg).await;
                    // Sender dropped when it goes out of scope
                }
                None => {
                    tracing::warn!(
                        "COMPLETE for unknown request_id {}: closing connection",
                        msg.request_id
                    );
                }
            }
        }
    }
}
