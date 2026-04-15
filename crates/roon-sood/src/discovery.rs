use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use tokio::net::UdpSocket;
use tokio::sync::broadcast;

use crate::{parse, serialize_query, SoodType, ROON_CORE_SERVICE_ID, SOOD_MULTICAST_IP, SOOD_PORT};

/// Information about a discovered Roon Core on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCore {
    /// Unique identifier for this Roon Core instance.
    pub core_id: String,
    /// IP address to connect to.
    pub host: IpAddr,
    /// TCP port for the MOO/WebSocket API endpoint.
    pub http_port: u16,
}

/// SOOD network discovery for Roon Cores.
///
/// Sends multicast/broadcast UDP queries on port 9003 and listens for
/// responses from Roon Cores on the local network.
pub struct SoodDiscovery {
    /// Signal to stop the discovery task.
    cancel_tx: tokio::sync::watch::Sender<bool>,
    /// Handle to the background discovery task.
    task_handle: tokio::task::JoinHandle<()>,
}

impl SoodDiscovery {
    /// Start SOOD discovery. Returns the discovery handle and a receiver for
    /// discovered cores.
    ///
    /// The receiver yields `DiscoveredCore` each time a Roon Core responds to
    /// a query. Duplicate responses for the same core may be emitted.
    pub async fn start() -> Result<(Self, broadcast::Receiver<DiscoveredCore>), crate::SoodError> {
        let (core_tx, core_rx) = broadcast::channel::<DiscoveredCore>(16);
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        // Bind receive socket on the SOOD port
        let recv_socket = bind_recv_socket().await?;
        // Bind send socket on an ephemeral port
        let send_socket = bind_send_socket().await?;

        let task_handle = tokio::spawn(discovery_loop(
            recv_socket,
            send_socket,
            core_tx,
            cancel_rx,
        ));

        Ok((
            SoodDiscovery {
                cancel_tx,
                task_handle,
            },
            core_rx,
        ))
    }

    /// Send an immediate discovery query (don't wait for the next scheduled tick).
    pub fn query_now(&self) {
        // The discovery loop will re-query on the next tick.
        // For immediate query, we could use a channel, but the 10s interval
        // is fast enough for most use cases. This is a placeholder for future
        // enhancement.
    }

    /// Stop discovery and release network resources.
    pub async fn stop(self) {
        let _ = self.cancel_tx.send(true);
        let _ = self.task_handle.await;
    }
}

async fn bind_recv_socket() -> Result<UdpSocket, crate::SoodError> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    socket
        .set_reuse_address(true)
        .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    socket
        .set_nonblocking(true)
        .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, SOOD_PORT);
    socket
        .bind(&addr.into())
        .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    // Join multicast group on all interfaces
    let multicast_addr: Ipv4Addr = SOOD_MULTICAST_IP
        .parse()
        .expect("hardcoded multicast IP is valid");
    socket
        .join_multicast_v4(&multicast_addr, &Ipv4Addr::UNSPECIFIED)
        .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    let std_socket: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_socket).map_err(|e| crate::SoodError::Io(e.to_string()))
}

async fn bind_send_socket() -> Result<UdpSocket, crate::SoodError> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    socket
        .set_broadcast(true)
        .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    socket
        .set_multicast_ttl_v4(1)
        .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    socket
        .set_nonblocking(true)
        .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    socket
        .bind(&addr.into())
        .map_err(|e| crate::SoodError::Io(e.to_string()))?;

    let std_socket: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_socket).map_err(|e| crate::SoodError::Io(e.to_string()))
}

/// Build the query packet for Roon Core discovery.
fn build_query_packet() -> Vec<u8> {
    let mut props = HashMap::new();
    props.insert(
        "query_service_id".to_string(),
        Some(ROON_CORE_SERVICE_ID.to_string()),
    );
    props.insert(
        "_tid".to_string(),
        Some(uuid::Uuid::new_v4().to_string()),
    );
    serialize_query(&props)
}

/// Main discovery loop: send queries periodically and process responses.
async fn discovery_loop(
    recv_socket: UdpSocket,
    send_socket: UdpSocket,
    core_tx: broadcast::Sender<DiscoveredCore>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) {
    let multicast_target: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(SOOD_MULTICAST_IP.parse().unwrap(), SOOD_PORT));
    let broadcast_target: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, SOOD_PORT));

    // Send initial query immediately
    send_query(&send_socket, &multicast_target, &broadcast_target).await;

    let mut scan_interval = tokio::time::interval(std::time::Duration::from_secs(10));
    let mut tick_count: u64 = 0;
    let mut buf = vec![0u8; 65535];

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    break;
                }
            }

            // Periodic query
            _ = scan_interval.tick() => {
                tick_count += 1;
                // Adaptive frequency: every tick for first 60s, then every 6th tick
                if tick_count <= 6 || tick_count.is_multiple_of(6) {
                    send_query(&send_socket, &multicast_target, &broadcast_target).await;
                }
            }

            // Incoming responses
            result = recv_socket.recv_from(&mut buf) => {
                match result {
                    Ok((len, from)) => {
                        if let Some(core) = process_response(&buf[..len], from) {
                            let _ = core_tx.send(core);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("SOOD recv error: {}", e);
                    }
                }
            }
        }
    }
}

/// Send a discovery query to multicast and broadcast addresses.
async fn send_query(
    send_socket: &UdpSocket,
    multicast_target: &SocketAddr,
    broadcast_target: &SocketAddr,
) {
    let packet = build_query_packet();

    if let Err(e) = send_socket.send_to(&packet, multicast_target).await {
        tracing::debug!("SOOD multicast send failed: {}", e);
    }
    if let Err(e) = send_socket.send_to(&packet, broadcast_target).await {
        tracing::debug!("SOOD broadcast send failed: {}", e);
    }
}

/// Process a received SOOD response and extract core information.
fn process_response(buf: &[u8], from: SocketAddr) -> Option<DiscoveredCore> {
    let msg = match parse(buf, from) {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!("SOOD parse error: {}", e);
            return None;
        }
    };

    // Only process Response messages
    if msg.msg_type != SoodType::Response {
        return None;
    }

    // Extract required fields
    let core_id = msg.props.get("unique_id")?.as_ref()?.clone();
    let http_port_str = msg.props.get("http_port")?.as_ref()?;
    let http_port: u16 = http_port_str.parse().ok()?;

    Some(DiscoveredCore {
        core_id,
        host: msg.from.ip(),
        http_port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_response_valid() {
        // Build a valid SOOD response packet
        let mut props = HashMap::new();
        props.insert(
            "service_id".to_string(),
            Some(ROON_CORE_SERVICE_ID.to_string()),
        );
        props.insert(
            "unique_id".to_string(),
            Some("test-core-123".to_string()),
        );
        props.insert("http_port".to_string(), Some("9100".to_string()));
        props.insert(
            "_tid".to_string(),
            Some("tid-placeholder".to_string()),
        );

        // Build raw bytes manually: SOOD\x02R + TLV properties
        let mut buf = Vec::new();
        buf.extend_from_slice(b"SOOD\x02R");
        for (name, value) in &props {
            buf.push(name.len() as u8);
            buf.extend_from_slice(name.as_bytes());
            match value {
                Some(v) => {
                    buf.extend_from_slice(&(v.len() as u16).to_be_bytes());
                    buf.extend_from_slice(v.as_bytes());
                }
                None => {
                    buf.extend_from_slice(&0xFFFFu16.to_be_bytes());
                }
            }
        }

        let from: SocketAddr = "192.168.1.100:9003".parse().unwrap();
        let core = process_response(&buf, from).unwrap();
        assert_eq!(core.core_id, "test-core-123");
        assert_eq!(core.http_port, 9100);
        assert_eq!(core.host, IpAddr::V4("192.168.1.100".parse().unwrap()));
    }

    #[test]
    fn test_process_response_ignores_queries() {
        // Build a query packet (type = Q)
        let mut buf = Vec::new();
        buf.extend_from_slice(b"SOOD\x02Q");

        let from: SocketAddr = "192.168.1.100:9003".parse().unwrap();
        assert!(process_response(&buf, from).is_none());
    }

    #[test]
    fn test_process_response_missing_fields() {
        // Response with no properties
        let buf = b"SOOD\x02R";
        let from: SocketAddr = "192.168.1.100:9003".parse().unwrap();
        assert!(process_response(buf, from).is_none());
    }

    #[test]
    fn test_build_query_packet_is_valid() {
        let packet = build_query_packet();
        let from: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let msg = parse(&packet, from).unwrap();
        assert_eq!(msg.msg_type, SoodType::Query);
        assert_eq!(
            msg.props.get("query_service_id").unwrap().as_ref().unwrap(),
            ROON_CORE_SERVICE_ID
        );
        assert!(msg.props.contains_key("_tid"));
    }

    #[tokio::test]
    async fn test_loopback_send_recv() {
        // Bind a UDP socket to receive our own query
        let recv = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let recv_addr = recv.local_addr().unwrap();

        let send = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        send.set_broadcast(true).unwrap();

        let packet = build_query_packet();
        send.send_to(&packet, recv_addr).await.unwrap();

        let mut buf = vec![0u8; 65535];
        let (len, from) = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            recv.recv_from(&mut buf),
        )
        .await
        .unwrap()
        .unwrap();

        let msg = parse(&buf[..len], from).unwrap();
        assert_eq!(msg.msg_type, SoodType::Query);
    }
}
