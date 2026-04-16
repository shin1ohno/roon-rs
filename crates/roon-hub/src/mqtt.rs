use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use tokio::sync::mpsc;

use crate::config::MqttConfig;

/// MQTT bridge that publishes Roon zone state and receives commands.
///
/// Topic structure aligned with weave SPEC:
///   Publish: service/roon/{zone_id}/state/{property}
///   Subscribe: service/roon/+/command/+
pub struct MqttBridge {
    client: AsyncClient,
    event_loop: EventLoop,
    command_rx: mpsc::Receiver<(String, String)>,
    command_tx: mpsc::Sender<(String, String)>,
}

impl MqttBridge {
    pub fn new(config: &MqttConfig) -> Self {
        let mut opts = MqttOptions::new(&config.client_id, &config.host, config.port);
        opts.set_keep_alive(std::time::Duration::from_secs(30));

        let (client, event_loop) = AsyncClient::new(opts, 64);
        let (command_tx, command_rx) = mpsc::channel(64);

        MqttBridge {
            client,
            event_loop,
            command_rx,
            command_tx,
        }
    }

    pub async fn start(
        mut self,
    ) -> anyhow::Result<(AsyncClient, mpsc::Receiver<(String, String)>)> {
        // Subscribe to weave-compatible command topics
        self.client
            .subscribe("service/roon/+/command/+", QoS::AtLeastOnce)
            .await?;

        let command_tx = self.command_tx.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                match self.event_loop.poll().await {
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(msg))) => {
                        let topic = msg.topic.clone();
                        if let Ok(payload) = String::from_utf8(msg.payload.to_vec()) {
                            let _ = command_tx.send((topic, payload)).await;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("MQTT error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok((client, self.command_rx))
    }
}

/// Publish zone state to service/roon/{zone_id}/state/zone (full JSON).
pub async fn publish_zone(client: &AsyncClient, zone: &roon_api::Zone) -> anyhow::Result<()> {
    let topic = format!("service/roon/{}/state/zone", zone.zone_id);
    let payload = serde_json::to_string(zone)?;
    client
        .publish(&topic, QoS::AtLeastOnce, true, payload)
        .await?;

    // Also publish individual state properties for weave routing
    let state_str = serde_json::to_string(&zone.state)?;
    client
        .publish(
            &format!("service/roon/{}/state/playback", zone.zone_id),
            QoS::AtLeastOnce,
            true,
            state_str,
        )
        .await?;

    if let Some(np) = &zone.now_playing {
        client
            .publish(
                &format!("service/roon/{}/state/now_playing", zone.zone_id),
                QoS::AtLeastOnce,
                true,
                serde_json::to_string(np)?,
            )
            .await?;
    }

    // Publish volume per output
    for output in &zone.outputs {
        if let Some(vol) = &output.volume {
            client
                .publish(
                    &format!("service/roon/{}/state/volume", zone.zone_id),
                    QoS::AtLeastOnce,
                    true,
                    serde_json::to_string(vol)?,
                )
                .await?;
        }
    }

    Ok(())
}

/// Publish seek position updates.
pub async fn publish_seek(
    client: &AsyncClient,
    seeks: &[roon_api::ZoneSeek],
) -> anyhow::Result<()> {
    for seek in seeks {
        let topic = format!("service/roon/{}/state/seek", seek.zone_id);
        let payload = serde_json::to_string(seek)?;
        client
            .publish(&topic, QoS::AtMostOnce, false, payload)
            .await?;
    }
    Ok(())
}

/// Publish all zones.
pub async fn publish_zones(client: &AsyncClient, zones: &[roon_api::Zone]) -> anyhow::Result<()> {
    // Publish zone list
    let summary: Vec<serde_json::Value> = zones
        .iter()
        .map(|z| {
            serde_json::json!({
                "zone_id": z.zone_id,
                "display_name": z.display_name,
                "state": z.state,
            })
        })
        .collect();
    client
        .publish(
            "service/roon/zones",
            QoS::AtLeastOnce,
            true,
            serde_json::to_string(&summary)?,
        )
        .await?;

    for zone in zones {
        publish_zone(client, zone).await?;
    }
    Ok(())
}
