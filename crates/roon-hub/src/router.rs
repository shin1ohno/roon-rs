use roon_api::{ControlAction, MuteAction, SeekMode, Transport, VolumeMode};

/// Parse an MQTT command topic and payload, dispatch to Transport.
///
/// Topic format: `{prefix}/command/{action}`
/// Payload: JSON with `zone_or_output_id` and action-specific fields.
///
/// Supported actions:
/// - `control`: `{"zone_or_output_id": "...", "action": "play|pause|stop|next|previous"}`
/// - `seek`: `{"zone_or_output_id": "...", "how": "relative|absolute", "seconds": 30}`
/// - `volume`: `{"output_id": "...", "how": "absolute|relative|relative_step", "value": 50}`
/// - `mute`: `{"output_id": "...", "action": "mute|unmute"}`
pub async fn handle_command(
    transport: &Transport,
    topic_prefix: &str,
    topic: &str,
    payload: &str,
) -> anyhow::Result<()> {
    let action = topic
        .strip_prefix(&format!("{}/command/", topic_prefix))
        .unwrap_or("");

    let body: serde_json::Value = serde_json::from_str(payload)?;

    match action {
        "control" => {
            let zone_id = body["zone_or_output_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing zone_or_output_id"))?;
            let action_str = body["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing action"))?;
            let action = match action_str {
                "play" => ControlAction::Play,
                "pause" => ControlAction::Pause,
                "playpause" => ControlAction::PlayPause,
                "stop" => ControlAction::Stop,
                "next" => ControlAction::Next,
                "previous" => ControlAction::Previous,
                _ => anyhow::bail!("unknown control action: {}", action_str),
            };
            transport.control(zone_id, action).await?;
        }
        "seek" => {
            let zone_id = body["zone_or_output_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing zone_or_output_id"))?;
            let how = match body["how"].as_str().unwrap_or("relative") {
                "absolute" => SeekMode::Absolute,
                _ => SeekMode::Relative,
            };
            let seconds = body["seconds"].as_i64().unwrap_or(0);
            transport.seek(zone_id, how, seconds).await?;
        }
        "volume" => {
            let output_id = body["output_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing output_id"))?;
            let how = match body["how"].as_str().unwrap_or("relative") {
                "absolute" => VolumeMode::Absolute,
                "relative_step" => VolumeMode::RelativeStep,
                _ => VolumeMode::Relative,
            };
            let value = body["value"].as_f64().unwrap_or(0.0);
            transport.change_volume(output_id, how, value).await?;
        }
        "mute" => {
            let output_id = body["output_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing output_id"))?;
            let mute = match body["action"].as_str().unwrap_or("mute") {
                "unmute" => MuteAction::Unmute,
                _ => MuteAction::Mute,
            };
            transport.mute(output_id, mute).await?;
        }
        _ => {
            tracing::warn!("Unknown command action: {}", action);
        }
    }

    Ok(())
}
