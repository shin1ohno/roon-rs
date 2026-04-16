use roon_api::{ControlAction, MuteAction, SeekMode, Transport, VolumeMode};

/// Parse a weave-compatible MQTT command topic and payload, dispatch to Transport.
///
/// Topic format: `service/roon/{zone_id}/command/{intent}`
/// Payload: JSON with intent-specific fields from weave engine.
///
/// Supported intents:
/// - `play`, `pause`, `playpause`, `stop`, `next`, `previous`
/// - `volume_change`: `{"data": {"delta": 3.0}}`
/// - `volume_set`: `{"data": {"value": 50.0}}`
/// - `mute`, `unmute`
/// - `seek_relative`: `{"data": {"seconds": 30.0}}`
/// - `seek_absolute`: `{"data": {"seconds": 120.0}}`
pub async fn handle_command(
    transport: &Transport,
    topic: &str,
    payload: &str,
) -> anyhow::Result<()> {
    // Parse topic: service/roon/{zone_id}/command/{intent}
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() < 5 || parts[0] != "service" || parts[1] != "roon" || parts[3] != "command" {
        return Ok(());
    }

    let zone_id = parts[2];
    let intent = parts[4];
    let body: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();

    match intent {
        "play" => transport.control(zone_id, ControlAction::Play).await?,
        "pause" => transport.control(zone_id, ControlAction::Pause).await?,
        "playpause" => transport.control(zone_id, ControlAction::PlayPause).await?,
        "stop" => transport.control(zone_id, ControlAction::Stop).await?,
        "next" => transport.control(zone_id, ControlAction::Next).await?,
        "previous" => transport.control(zone_id, ControlAction::Previous).await?,

        "volume_change" => {
            let delta = body["data"]["delta"].as_f64().unwrap_or(0.0);
            // Find first output for this zone to apply volume
            transport
                .change_volume(zone_id, VolumeMode::Relative, delta)
                .await?;
        }
        "volume_set" => {
            let value = body["data"]["value"].as_f64().unwrap_or(0.0);
            transport
                .change_volume(zone_id, VolumeMode::Absolute, value)
                .await?;
        }
        "mute" => {
            transport.mute(zone_id, MuteAction::Mute).await?;
        }
        "unmute" => {
            transport.mute(zone_id, MuteAction::Unmute).await?;
        }
        "seek_relative" => {
            let seconds = body["data"]["seconds"].as_f64().unwrap_or(0.0) as i64;
            transport.seek(zone_id, SeekMode::Relative, seconds).await?;
        }
        "seek_absolute" => {
            let seconds = body["data"]["seconds"].as_f64().unwrap_or(0.0) as i64;
            transport.seek(zone_id, SeekMode::Absolute, seconds).await?;
        }
        _ => {
            tracing::warn!("Unknown intent: {}", intent);
        }
    }

    Ok(())
}
