use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use roon_api::{MemoryTokenStore, RoonClientBuilder, RoonEvent, TokenStore};

fn build_moo_response(
    verb: &str,
    name: &str,
    request_id: u32,
    body: Option<serde_json::Value>,
) -> Vec<u8> {
    let mut buf = format!("MOO/1 {} {}\nRequest-Id: {}\n", verb, name, request_id);
    if let Some(ref b) = body {
        let json = serde_json::to_string(b).unwrap();
        buf.push_str(&format!(
            "Content-Length: {}\nContent-Type: application/json\n",
            json.len()
        ));
        buf.push('\n');
        buf.push_str(&json);
    } else {
        buf.push('\n');
    }
    buf.into_bytes()
}

/// Mock Roon Core: handles info + register requests
async fn mock_roon_core() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut sink, mut source) = ws.split();

            while let Some(Ok(msg)) = source.next().await {
                if let WsMessage::Binary(data) = msg {
                    let parsed = roon_moo::parse(&data).unwrap();
                    let name = parsed.name.clone();

                    if name == "com.roonlabs.registry:1/info" {
                        let resp = build_moo_response(
                            "COMPLETE",
                            "Success",
                            parsed.request_id,
                            Some(serde_json::json!({
                                "core_id": "test-core-001",
                                "display_name": "Test Roon Core",
                                "display_version": "2.0"
                            })),
                        );
                        sink.send(WsMessage::Binary(resp.into())).await.unwrap();
                    } else if name == "com.roonlabs.registry:1/register" {
                        let resp = build_moo_response(
                            "CONTINUE",
                            "Registered",
                            parsed.request_id,
                            Some(serde_json::json!({
                                "core_id": "test-core-001",
                                "display_name": "Test Roon Core",
                                "display_version": "2.0",
                                "token": "auth-token-xyz",
                                "provided_services": ["com.roonlabs.transport:2"],
                                "http_port": 9100
                            })),
                        );
                        sink.send(WsMessage::Binary(resp.into())).await.unwrap();
                    }
                }
            }
        }
    });

    addr
}

#[tokio::test]
async fn test_connect_and_register() {
    let addr = mock_roon_core().await;

    let client = RoonClientBuilder::new(
        "com.test.ext",
        "Test Extension",
        "1.0.0",
        "Test Publisher",
        "test@example.com",
    )
    .token_store(MemoryTokenStore::new())
    .require_transport()
    .build()
    .unwrap();

    let core = client
        .connect(&addr.ip().to_string(), addr.port())
        .await
        .unwrap();

    assert_eq!(core.core_id(), "test-core-001");
    assert_eq!(core.display_name(), "Test Roon Core");
    assert_eq!(core.display_version(), "2.0");
}

#[tokio::test]
async fn test_events_emitted_on_connect() {
    let addr = mock_roon_core().await;

    let client = RoonClientBuilder::new(
        "com.test.ext",
        "Test Extension",
        "1.0.0",
        "Test Publisher",
        "test@example.com",
    )
    .build()
    .unwrap();

    let mut events = client.events();

    let _core = client
        .connect(&addr.ip().to_string(), addr.port())
        .await
        .unwrap();

    // Should receive CorePaired event
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        RoonEvent::CorePaired(core) => {
            assert_eq!(core.core_id(), "test-core-001");
        }
        other => panic!("expected CorePaired, got {:?}", other),
    }
}

#[tokio::test]
async fn test_token_persistence() {
    let addr = mock_roon_core().await;
    let store = MemoryTokenStore::new();

    // First connection — no token yet
    assert!(store.load_token("test-core-001").is_none());

    let client = RoonClientBuilder::new(
        "com.test.ext",
        "Test Extension",
        "1.0.0",
        "Test Publisher",
        "test@example.com",
    )
    .token_store(store)
    .build()
    .unwrap();

    let _core = client
        .connect(&addr.ip().to_string(), addr.port())
        .await
        .unwrap();

    // Token should be persisted now — but we can't access the store after move.
    // This test validates the flow doesn't error; actual token persistence
    // is tested in the token module unit tests.
}

#[tokio::test]
async fn test_builder_defaults() {
    let client = RoonClientBuilder::new(
        "com.test.ext",
        "Test Extension",
        "1.0.0",
        "Test Publisher",
        "test@example.com",
    )
    .build()
    .unwrap();

    // Just verify it builds successfully with defaults
    let _events = client.events();
}
