use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use tokio::sync::mpsc;

use crate::config::MqttConfig;

/// MQTT bridge that publishes Roon zone state and receives device commands.
pub struct MqttBridge {
    client: AsyncClient,
    event_loop: EventLoop,
    topic_prefix: String,
    /// Receiver for inbound MQTT commands (topic, payload).
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
            topic_prefix: config.topic_prefix.clone(),
            command_rx,
            command_tx,
        }
    }

    /// Topic prefix for constructing topic paths.
    pub fn topic_prefix(&self) -> &str {
        &self.topic_prefix
    }

    /// Subscribe to command topics and run the MQTT event loop.
    /// Returns a receiver for incoming commands.
    pub async fn start(mut self) -> anyhow::Result<(AsyncClient, mpsc::Receiver<(String, String)>)> {
        let command_topic = format!("{}/command/#", self.topic_prefix);
        self.client
            .subscribe(&command_topic, QoS::AtLeastOnce)
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

/// Publish a zone state update to MQTT.
pub async fn publish_zone(
    client: &AsyncClient,
    topic_prefix: &str,
    zone: &roon_api::Zone,
) -> anyhow::Result<()> {
    let topic = format!("{}/zone/{}", topic_prefix, zone.zone_id);
    let payload = serde_json::to_string(zone)?;
    client
        .publish(&topic, QoS::AtLeastOnce, true, payload)
        .await?;
    Ok(())
}

/// Publish all zones as a list.
pub async fn publish_zones(
    client: &AsyncClient,
    topic_prefix: &str,
    zones: &[roon_api::Zone],
) -> anyhow::Result<()> {
    let topic = format!("{}/zones", topic_prefix);
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
    let payload = serde_json::to_string(&summary)?;
    client
        .publish(&topic, QoS::AtLeastOnce, true, payload)
        .await?;

    for zone in zones {
        publish_zone(client, topic_prefix, zone).await?;
    }
    Ok(())
}
