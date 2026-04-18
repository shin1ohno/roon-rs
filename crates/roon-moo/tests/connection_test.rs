use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use roon_moo::connection::MooConnection;
use roon_moo::{MooBody, MooMessage, MooVerb};

/// Start a mock WebSocket server that runs the given handler for each connection.
async fn mock_ws_server<F, Fut>(handler: F) -> SocketAddr
where
    F: Fn(
            SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, WsMessage>,
            SplitStream<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>>,
        ) -> Fut
        + Send
        + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (sink, source) = ws.split();
            handler(sink, source).await;
        }
    });

    addr
}

fn build_moo_response(
    verb: MooVerb,
    name: &str,
    request_id: u32,
    body: Option<serde_json::Value>,
) -> Vec<u8> {
    let msg = MooMessage {
        verb,
        name: name.to_string(),
        request_id,
        headers: HashMap::new(),
        body: body.map(MooBody::Json),
    };
    roon_moo::serialize(&msg)
}

#[tokio::test]
async fn test_send_request_complete() {
    let addr = mock_ws_server(|mut sink, mut source| async move {
        // Receive request, send COMPLETE
        while let Some(Ok(msg)) = source.next().await {
            if let WsMessage::Binary(data) = msg {
                let parsed = roon_moo::parse(&data).unwrap();
                if parsed.verb == MooVerb::Request {
                    let response = build_moo_response(
                        MooVerb::Complete,
                        "Success",
                        parsed.request_id,
                        Some(serde_json::json!({"result": "ok"})),
                    );
                    sink.send(WsMessage::Binary(response.into())).await.unwrap();
                }
            }
        }
    })
    .await;

    let url = format!("ws://{}/api", addr);
    let conn = MooConnection::connect(&url, HashMap::new()).await.unwrap();

    let response = conn.send_request("com.test:1/method", None).await.unwrap();
    assert_eq!(response.verb, MooVerb::Complete);
    assert_eq!(response.name, "Success");
    assert_eq!(
        response.json_body().unwrap()["result"],
        serde_json::json!("ok")
    );

    conn.close().await;
}

#[tokio::test]
async fn test_subscription_continue_then_complete() {
    let addr = mock_ws_server(|mut sink, mut source| async move {
        while let Some(Ok(msg)) = source.next().await {
            if let WsMessage::Binary(data) = msg {
                let parsed = roon_moo::parse(&data).unwrap();
                if parsed.verb == MooVerb::Request {
                    let rid = parsed.request_id;

                    // Send 3 CONTINUEs
                    for i in 0..3 {
                        let resp = build_moo_response(
                            MooVerb::Continue,
                            "Changed",
                            rid,
                            Some(serde_json::json!({"seq": i})),
                        );
                        sink.send(WsMessage::Binary(resp.into())).await.unwrap();
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }

                    // Send COMPLETE to end
                    let resp = build_moo_response(MooVerb::Complete, "Unsubscribed", rid, None);
                    sink.send(WsMessage::Binary(resp.into())).await.unwrap();
                }
            }
        }
    })
    .await;

    let url = format!("ws://{}/api", addr);
    let conn = MooConnection::connect(&url, HashMap::new()).await.unwrap();

    let mut rx = conn
        .subscribe(
            "com.test:1/subscribe_data",
            serde_json::json!({"subscription_key": 0}),
        )
        .await
        .unwrap();

    // Should receive 3 CONTINUEs + 1 COMPLETE
    let mut messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        messages.push(msg);
    }

    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].verb, MooVerb::Continue);
    assert_eq!(messages[0].name, "Changed");
    assert_eq!(
        messages[0].json_body().unwrap()["seq"],
        serde_json::json!(0)
    );
    assert_eq!(
        messages[1].json_body().unwrap()["seq"],
        serde_json::json!(1)
    );
    assert_eq!(
        messages[2].json_body().unwrap()["seq"],
        serde_json::json!(2)
    );
    assert_eq!(messages[3].verb, MooVerb::Complete);
    assert_eq!(messages[3].name, "Unsubscribed");

    conn.close().await;
}

#[tokio::test]
async fn test_service_handler_incoming_request() {
    let (handler_tx, mut handler_rx) = tokio::sync::mpsc::channel::<String>(1);

    let addr = mock_ws_server(|mut sink, mut source| async move {
        // Wait for the connection to be established, then send a REQUEST to the client
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let req = build_moo_response(MooVerb::Request, "com.roonlabs.ping:1/ping", 0, None);
        sink.send(WsMessage::Binary(req.into())).await.unwrap();

        // Read the client's response
        while let Some(Ok(msg)) = source.next().await {
            if let WsMessage::Binary(data) = msg {
                let parsed = roon_moo::parse(&data).unwrap();
                if parsed.verb == MooVerb::Complete {
                    // Good, client responded
                    break;
                }
            }
        }
    })
    .await;

    let handler: roon_moo::connection::ServiceHandler =
        Arc::new(move |msg: MooMessage, responder| {
            let tx = handler_tx.clone();
            let method = msg.method().unwrap_or("unknown").to_string();
            tokio::spawn(async move {
                tx.send(method).await.unwrap();
                responder.send_complete("Success", None).await.unwrap();
            });
        });

    let mut handlers = HashMap::new();
    handlers.insert("com.roonlabs.ping:1".to_string(), handler);

    let url = format!("ws://{}/api", addr);
    let _conn = MooConnection::connect(&url, handlers).await.unwrap();

    // Wait for the handler to be called
    let method = tokio::time::timeout(std::time::Duration::from_secs(2), handler_rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(method, "ping");
}

#[tokio::test]
async fn test_continue_on_oneshot_delivers_first_continue() {
    // This tests the registry pattern: send_request gets a CONTINUE Registered
    let addr = mock_ws_server(|mut sink, mut source| async move {
        while let Some(Ok(msg)) = source.next().await {
            if let WsMessage::Binary(data) = msg {
                let parsed = roon_moo::parse(&data).unwrap();
                if parsed.verb == MooVerb::Request {
                    // Reply with CONTINUE (like registry Registered)
                    let resp = build_moo_response(
                        MooVerb::Continue,
                        "Registered",
                        parsed.request_id,
                        Some(serde_json::json!({
                            "core_id": "test-core",
                            "token": "auth-token"
                        })),
                    );
                    sink.send(WsMessage::Binary(resp.into())).await.unwrap();
                }
            }
        }
    })
    .await;

    let url = format!("ws://{}/api", addr);
    let conn = MooConnection::connect(&url, HashMap::new()).await.unwrap();

    let response = conn
        .send_request(
            "com.roonlabs.registry:1/register",
            Some(serde_json::json!({"extension_id": "test"})),
        )
        .await
        .unwrap();

    assert_eq!(response.verb, MooVerb::Continue);
    assert_eq!(response.name, "Registered");
    assert_eq!(
        response.json_body().unwrap()["core_id"],
        serde_json::json!("test-core")
    );

    conn.close().await;
}

#[tokio::test]
async fn test_connection_close_ends_subscription() {
    let addr = mock_ws_server(|mut sink, mut source| async move {
        while let Some(Ok(msg)) = source.next().await {
            if let WsMessage::Binary(data) = msg {
                let parsed = roon_moo::parse(&data).unwrap();
                if parsed.verb == MooVerb::Request {
                    // Send one CONTINUE, then close the connection
                    let resp = build_moo_response(
                        MooVerb::Continue,
                        "Subscribed",
                        parsed.request_id,
                        Some(serde_json::json!({"zones": []})),
                    );
                    sink.send(WsMessage::Binary(resp.into())).await.unwrap();
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    // Close from server side
                    sink.send(WsMessage::Close(None)).await.ok();
                    break;
                }
            }
        }
    })
    .await;

    let url = format!("ws://{}/api", addr);
    let conn = MooConnection::connect(&url, HashMap::new()).await.unwrap();

    let mut rx = conn
        .subscribe(
            "com.test:1/subscribe_zones",
            serde_json::json!({"subscription_key": 0}),
        )
        .await
        .unwrap();

    let mut messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        messages.push(msg);
    }

    // Should have received at least the initial Subscribed CONTINUE
    assert!(!messages.is_empty());
    assert_eq!(messages[0].verb, MooVerb::Continue);
    assert_eq!(messages[0].name, "Subscribed");
}

#[tokio::test]
async fn test_multiple_concurrent_requests() {
    let addr = mock_ws_server(|mut sink, mut source| async move {
        while let Some(Ok(msg)) = source.next().await {
            if let WsMessage::Binary(data) = msg {
                let parsed = roon_moo::parse(&data).unwrap();
                if parsed.verb == MooVerb::Request {
                    let resp = build_moo_response(
                        MooVerb::Complete,
                        "Success",
                        parsed.request_id,
                        Some(serde_json::json!({"echo": parsed.name})),
                    );
                    sink.send(WsMessage::Binary(resp.into())).await.unwrap();
                }
            }
        }
    })
    .await;

    let url = format!("ws://{}/api", addr);
    let conn = MooConnection::connect(&url, HashMap::new()).await.unwrap();

    // Send two requests concurrently
    let (r1, r2) = tokio::join!(
        conn.send_request("svc/method_a", None),
        conn.send_request("svc/method_b", None),
    );

    let r1 = r1.unwrap();
    let r2 = r2.unwrap();

    // Both should complete successfully (order may vary based on request_id)
    assert_eq!(r1.verb, MooVerb::Complete);
    assert_eq!(r2.verb, MooVerb::Complete);

    conn.close().await;
}

/// Client must originate `com.roonlabs.ping:1/ping` requests on its own so
/// Roon Core's idle-MOO-traffic timeout never fires. The first tick is
/// delayed by 3s (to let registration finish), then they fire every 2s.
#[tokio::test]
async fn test_client_sends_moo_keepalive_ping() {
    let (ping_tx, mut ping_rx) = tokio::sync::mpsc::channel::<String>(8);

    let addr = mock_ws_server(move |mut sink, mut source| {
        let ping_tx = ping_tx.clone();
        async move {
            while let Some(Ok(msg)) = source.next().await {
                if let WsMessage::Binary(data) = msg
                    && let Ok(parsed) = roon_moo::parse(&data)
                    && parsed.verb == MooVerb::Request
                {
                    let _ = ping_tx.send(parsed.name.clone()).await;
                    // Respond so client's pending table stays tidy even though
                    // keepalive pings are not registered there.
                    let resp =
                        build_moo_response(MooVerb::Complete, "Success", parsed.request_id, None);
                    let _ = sink.send(WsMessage::Binary(resp.into())).await;
                }
            }
        }
    })
    .await;

    let url = format!("ws://{}/api", addr);
    let conn = MooConnection::connect(&url, HashMap::new()).await.unwrap();

    // Wait up to ~5s for the first ping. The heartbeat is delayed by 3s, so
    // allowing 6s of slack covers CI jitter without flaking.
    let first = tokio::time::timeout(std::time::Duration::from_secs(6), async {
        loop {
            if let Some(name) = ping_rx.recv().await
                && name == "com.roonlabs.ping:1/ping"
            {
                return name;
            }
        }
    })
    .await
    .expect("expected a MOO keepalive ping within 6s");
    assert_eq!(first, "com.roonlabs.ping:1/ping");

    conn.close().await;
}
