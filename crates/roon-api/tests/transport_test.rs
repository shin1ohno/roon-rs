use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use roon_api::{
    ControlAction, MemoryTokenStore, RoonClientBuilder, Zone, ZoneEvent,
};

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

/// Mock Roon Core that handles registry + transport subscribe_zones + control
async fn mock_roon_core_with_transport() -> std::net::SocketAddr {
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
                                "display_name": "Test Core",
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
                                "display_name": "Test Core",
                                "display_version": "2.0",
                                "token": "tok",
                                "provided_services": ["com.roonlabs.transport:2"],
                                "http_port": 9100
                            })),
                        );
                        sink.send(WsMessage::Binary(resp.into())).await.unwrap();
                    } else if name == "com.roonlabs.transport:2/subscribe_zones" {
                        // Send initial zones
                        let resp = build_moo_response(
                            "CONTINUE",
                            "Subscribed",
                            parsed.request_id,
                            Some(serde_json::json!({
                                "zones": [{
                                    "zone_id": "zone-1",
                                    "display_name": "Living Room",
                                    "state": "stopped",
                                    "outputs": [{
                                        "output_id": "out-1",
                                        "display_name": "Speaker",
                                        "zone_id": "zone-1"
                                    }],
                                    "is_play_allowed": true,
                                    "is_pause_allowed": false,
                                    "is_next_allowed": false,
                                    "is_previous_allowed": false,
                                    "is_seek_allowed": false
                                }]
                            })),
                        );
                        sink.send(WsMessage::Binary(resp.into())).await.unwrap();

                        // Send a zone change after a short delay
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        let update = build_moo_response(
                            "CONTINUE",
                            "Changed",
                            parsed.request_id,
                            Some(serde_json::json!({
                                "zones_changed": [{
                                    "zone_id": "zone-1",
                                    "display_name": "Living Room",
                                    "state": "playing",
                                    "outputs": [{
                                        "output_id": "out-1",
                                        "display_name": "Speaker",
                                        "zone_id": "zone-1"
                                    }],
                                    "is_play_allowed": false,
                                    "is_pause_allowed": true,
                                    "is_next_allowed": true,
                                    "is_previous_allowed": true,
                                    "is_seek_allowed": true,
                                    "now_playing": {
                                        "one_line": {"line1": "Track - Artist"},
                                        "length": 240
                                    }
                                }]
                            })),
                        );
                        sink.send(WsMessage::Binary(update.into())).await.unwrap();
                    } else if name == "com.roonlabs.transport:2/control" {
                        let resp = build_moo_response(
                            "COMPLETE",
                            "Success",
                            parsed.request_id,
                            None,
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
async fn test_subscribe_zones_initial_and_changed() {
    let addr = mock_roon_core_with_transport().await;

    let client = RoonClientBuilder::new(
        "com.test.ext",
        "Test",
        "1.0",
        "Test",
        "test@test.com",
    )
    .token_store(MemoryTokenStore::new())
    .require_transport()
    .build()
    .unwrap();

    let core = client.connect(&addr.ip().to_string(), addr.port()).await.unwrap();
    let transport = core.transport();

    let mut zone_rx = transport.subscribe_zones().await.unwrap();

    // First event: Initial zones
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), zone_rx.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        ZoneEvent::Initial(zones) => {
            assert_eq!(zones.len(), 1);
            assert_eq!(zones[0].zone_id, "zone-1");
            assert_eq!(zones[0].display_name, "Living Room");
            assert_eq!(zones[0].state, roon_api::PlayState::Stopped);
            assert_eq!(zones[0].outputs.len(), 1);
        }
        other => panic!("expected Initial, got {:?}", other),
    }

    // Second event: Changed zones
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), zone_rx.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        ZoneEvent::Changed(zones) => {
            assert_eq!(zones.len(), 1);
            assert_eq!(zones[0].state, roon_api::PlayState::Playing);
            assert!(zones[0].now_playing.is_some());
            assert_eq!(
                zones[0].now_playing.as_ref().unwrap().one_line.line1,
                "Track - Artist"
            );
        }
        other => panic!("expected Changed, got {:?}", other),
    }
}

#[tokio::test]
async fn test_transport_control() {
    let addr = mock_roon_core_with_transport().await;

    let client = RoonClientBuilder::new(
        "com.test.ext",
        "Test",
        "1.0",
        "Test",
        "test@test.com",
    )
    .token_store(MemoryTokenStore::new())
    .build()
    .unwrap();

    let core = client.connect(&addr.ip().to_string(), addr.port()).await.unwrap();
    let transport = core.transport();

    // Control commands should succeed
    transport.control("zone-1", ControlAction::Play).await.unwrap();
    transport.control("zone-1", ControlAction::Pause).await.unwrap();
    transport.control("zone-1", ControlAction::Next).await.unwrap();
}

#[tokio::test]
async fn test_zone_deserialization() {
    let json = serde_json::json!({
        "zone_id": "z1",
        "display_name": "Office",
        "state": "paused",
        "outputs": [],
        "settings": {
            "shuffle": true,
            "auto_radio": false,
            "loop": "loop_one"
        },
        "now_playing": {
            "one_line": {"line1": "Song Title"},
            "two_line": {"line1": "Song Title", "line2": "Artist Name"},
            "length": 300.5,
            "seek_position": 45.2,
            "image_key": "abc123"
        },
        "seek_position": 45.2,
        "is_play_allowed": true,
        "is_pause_allowed": true,
        "is_next_allowed": true,
        "is_previous_allowed": false,
        "is_seek_allowed": true,
        "queue_items_remaining": 5,
        "queue_time_remaining": 1200.0
    });

    let zone: Zone = serde_json::from_value(json).unwrap();
    assert_eq!(zone.zone_id, "z1");
    assert_eq!(zone.display_name, "Office");
    assert_eq!(zone.state, roon_api::PlayState::Paused);
    assert!(zone.settings.is_some());
    let settings = zone.settings.unwrap();
    assert!(settings.shuffle);
    assert!(!settings.auto_radio);
    assert_eq!(settings.r#loop, roon_api::zone::LoopMode::LoopOne);
    assert!(zone.now_playing.is_some());
    let np = zone.now_playing.unwrap();
    assert_eq!(np.one_line.line1, "Song Title");
    assert_eq!(np.length.unwrap(), 300.5);
    assert_eq!(zone.queue_items_remaining.unwrap(), 5);
}
