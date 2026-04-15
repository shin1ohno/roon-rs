mod config;
mod mqtt;
mod router;

use std::path::PathBuf;

use roon_api::{FileTokenStore, RoonClientBuilder, RoonEvent, ZoneEvent};
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("roon-hub.toml"));

    let config = config::Config::load(&config_path)?;
    tracing::info!("Config loaded: roon={}, mqtt={}:{}",
        config.roon.extension_id, config.mqtt.host, config.mqtt.port);

    // MQTT bridge
    let mqtt_bridge = mqtt::MqttBridge::new(&config.mqtt);
    let topic_prefix = mqtt_bridge.topic_prefix().to_string();
    let (mqtt_client, mut command_rx) = mqtt_bridge.start().await?;
    tracing::info!("MQTT connected to {}:{}", config.mqtt.host, config.mqtt.port);

    // Roon SDK client
    let client = RoonClientBuilder::new(
        &config.roon.extension_id,
        &config.roon.display_name,
        &config.roon.display_version,
        &config.roon.publisher,
        &config.roon.email,
    )
    .token_store(FileTokenStore::new(&config.roon.token_path))
    .require_transport()
    .build()?;

    let mut events = client.events();

    // Connect to Roon Core
    if let (Some(host), Some(port)) = (&config.roon.host, config.roon.port) {
        tracing::info!("Connecting directly to {}:{}", host, port);
        client.connect(host, port).await?;
    } else {
        tracing::info!("Starting SOOD discovery...");
        client.start_discovery().await?;
    }

    // Channel for passing transport handle to command router
    let (transport_tx, mut transport_rx) = tokio::sync::mpsc::channel::<roon_api::Transport>(1);

    // Command handler task (spawned once, waits for transport)
    let cmd_prefix = topic_prefix.clone();
    tokio::spawn(async move {
        if let Some(transport) = transport_rx.recv().await {
            while let Some((topic, payload)) = command_rx.recv().await {
                if let Err(e) =
                    router::handle_command(&transport, &cmd_prefix, &topic, &payload).await
                {
                    tracing::warn!("Command error: {}", e);
                }
            }
        }
    });

    // Main event loop
    loop {
        tokio::select! {
            Ok(event) = events.recv() => {
                match event {
                    RoonEvent::CorePaired(core) => {
                        tracing::info!("Paired with core: {} ({})",
                            core.display_name(), core.core_id());

                        let transport = core.transport();
                        let mut zone_rx = transport.subscribe_zones().await?;
                        let mqtt = mqtt_client.clone();
                        let prefix = topic_prefix.clone();

                        // Send transport to command handler
                        let _ = transport_tx.send(transport).await;

                        // Zone event forwarder
                        tokio::spawn(async move {
                            while let Some(event) = zone_rx.recv().await {
                                match &event {
                                    ZoneEvent::Initial(zones) => {
                                        tracing::info!("Received {} zones", zones.len());
                                        if let Err(e) = mqtt::publish_zones(&mqtt, &prefix, zones).await {
                                            tracing::warn!("MQTT publish error: {}", e);
                                        }
                                    }
                                    ZoneEvent::Changed(zones) | ZoneEvent::Added(zones) => {
                                        for zone in zones {
                                            if let Err(e) = mqtt::publish_zone(&mqtt, &prefix, zone).await {
                                                tracing::warn!("MQTT publish error: {}", e);
                                            }
                                        }
                                    }
                                    ZoneEvent::Removed(ids) => {
                                        tracing::info!("Zones removed: {:?}", ids);
                                    }
                                    ZoneEvent::Seeked(_) => {}
                                }
                            }
                        });
                    }
                    RoonEvent::CoreLost { core_id } => {
                        tracing::warn!("Lost core: {}", core_id);
                    }
                    RoonEvent::CoreUnpaired { core_id } => {
                        tracing::warn!("Unpaired from: {}", core_id);
                    }
                    RoonEvent::CoreFound { core_id, .. } => {
                        tracing::info!("Found core: {}", core_id);
                    }
                }
            }

            _ = signal::ctrl_c() => {
                tracing::info!("Shutting down...");
                break;
            }
        }
    }

    Ok(())
}
