use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use roon_moo::MooVerb;
use roon_moo::connection::MooConnection;
use tokio::sync::mpsc;

use crate::error::ApiError;
use crate::output::Output;
use crate::queue::{QueueEvent, parse_queue_event};
use crate::zone::{Zone, ZoneSeek};

/// Zone and output subscriptions use the fixed keys 0 and 1, so queue
/// subscriptions — of which there may be several at once, one per zone — are
/// allocated from a counter starting above them.
static NEXT_QUEUE_SUBSCRIPTION_KEY: AtomicU32 = AtomicU32::new(2);

/// Events received from a zone subscription.
#[derive(Debug, Clone)]
pub enum ZoneEvent {
    /// Initial snapshot of all zones.
    Initial(Vec<Zone>),
    /// Zones were added.
    Added(Vec<Zone>),
    /// Zones were updated.
    Changed(Vec<Zone>),
    /// Zone seek positions updated.
    Seeked(Vec<ZoneSeek>),
    /// Zones were removed (by zone_id).
    Removed(Vec<String>),
}

/// Events received from an output subscription.
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// Initial snapshot of all outputs.
    Initial(Vec<Output>),
    /// Outputs were added.
    Added(Vec<Output>),
    /// Outputs were updated.
    Changed(Vec<Output>),
    /// Outputs were removed (by output_id).
    Removed(Vec<String>),
}

/// Playback control actions.
#[derive(Debug, Clone, Copy)]
pub enum ControlAction {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
}

impl ControlAction {
    fn as_str(&self) -> &'static str {
        match self {
            ControlAction::Play => "play",
            ControlAction::Pause => "pause",
            ControlAction::PlayPause => "playpause",
            ControlAction::Stop => "stop",
            ControlAction::Next => "next",
            ControlAction::Previous => "previous",
        }
    }
}

/// Seek mode.
#[derive(Debug, Clone, Copy)]
pub enum SeekMode {
    Relative,
    Absolute,
}

/// Volume adjustment mode.
#[derive(Debug, Clone, Copy)]
pub enum VolumeMode {
    Absolute,
    Relative,
    RelativeStep,
}

/// Mute action.
#[derive(Debug, Clone, Copy)]
pub enum MuteAction {
    Mute,
    Unmute,
}

/// Transport service providing zone subscriptions and playback control.
#[derive(Debug, Clone)]
pub struct Transport {
    connection: Arc<MooConnection>,
}

impl Transport {
    pub(crate) fn new(connection: Arc<MooConnection>) -> Self {
        Self { connection }
    }

    /// Subscribe to zone updates.
    ///
    /// Returns a receiver that yields `ZoneEvent`s as zones are added,
    /// changed, seeked, or removed.
    pub async fn subscribe_zones(&self) -> Result<mpsc::Receiver<ZoneEvent>, ApiError> {
        let mut raw_rx = self
            .connection
            .subscribe(
                "com.roonlabs.transport:2/subscribe_zones",
                serde_json::json!({"subscription_key": 0}),
            )
            .await?;

        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            while let Some(msg) = raw_rx.recv().await {
                if msg.verb == MooVerb::Complete {
                    break;
                }
                if let Some(body) = msg.json_body()
                    && let Some(event) = parse_zone_event(&msg.name, body)
                    && tx.send(event).await.is_err()
                {
                    break;
                }
            }
        });

        Ok(rx)
    }

    /// Subscribe to output updates.
    pub async fn subscribe_outputs(&self) -> Result<mpsc::Receiver<OutputEvent>, ApiError> {
        let mut raw_rx = self
            .connection
            .subscribe(
                "com.roonlabs.transport:2/subscribe_outputs",
                serde_json::json!({"subscription_key": 1}),
            )
            .await?;

        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            while let Some(msg) = raw_rx.recv().await {
                if msg.verb == MooVerb::Complete {
                    break;
                }
                if let Some(body) = msg.json_body()
                    && let Some(event) = parse_output_event(&msg.name, body)
                    && tx.send(event).await.is_err()
                {
                    break;
                }
            }
        });

        Ok(rx)
    }

    /// Control playback in a zone.
    pub async fn control(
        &self,
        zone_or_output_id: &str,
        action: ControlAction,
    ) -> Result<(), ApiError> {
        self.connection
            .send_request(
                "com.roonlabs.transport:2/control",
                Some(serde_json::json!({
                    "zone_or_output_id": zone_or_output_id,
                    "control": action.as_str()
                })),
            )
            .await?;
        Ok(())
    }

    /// Seek within the currently playing track.
    pub async fn seek(
        &self,
        zone_or_output_id: &str,
        mode: SeekMode,
        seconds: i64,
    ) -> Result<(), ApiError> {
        let how = match mode {
            SeekMode::Relative => "relative",
            SeekMode::Absolute => "absolute",
        };
        self.connection
            .send_request(
                "com.roonlabs.transport:2/seek",
                Some(serde_json::json!({
                    "zone_or_output_id": zone_or_output_id,
                    "how": how,
                    "seconds": seconds
                })),
            )
            .await?;
        Ok(())
    }

    /// Change volume of an output.
    pub async fn change_volume(
        &self,
        output_id: &str,
        mode: VolumeMode,
        value: f64,
    ) -> Result<(), ApiError> {
        let how = match mode {
            VolumeMode::Absolute => "absolute",
            VolumeMode::Relative => "relative",
            VolumeMode::RelativeStep => "relative_step",
        };
        self.connection
            .send_request(
                "com.roonlabs.transport:2/change_volume",
                Some(serde_json::json!({
                    "output_id": output_id,
                    "how": how,
                    "value": value
                })),
            )
            .await?;
        Ok(())
    }

    /// Mute or unmute an output.
    pub async fn mute(&self, output_id: &str, action: MuteAction) -> Result<(), ApiError> {
        let how = match action {
            MuteAction::Mute => "mute",
            MuteAction::Unmute => "unmute",
        };
        self.connection
            .send_request(
                "com.roonlabs.transport:2/mute",
                Some(serde_json::json!({
                    "output_id": output_id,
                    "how": how
                })),
            )
            .await?;
        Ok(())
    }

    /// Change zone settings (shuffle, loop, auto_radio).
    pub async fn change_settings(
        &self,
        zone_or_output_id: &str,
        shuffle: Option<bool>,
        loop_mode: Option<&str>,
        auto_radio: Option<bool>,
    ) -> Result<(), ApiError> {
        let mut settings = serde_json::Map::new();
        if let Some(s) = shuffle {
            settings.insert("shuffle".into(), serde_json::Value::Bool(s));
        }
        if let Some(l) = loop_mode {
            settings.insert("loop".into(), serde_json::Value::String(l.to_string()));
        }
        if let Some(a) = auto_radio {
            settings.insert("auto_radio".into(), serde_json::Value::Bool(a));
        }

        self.connection
            .send_request(
                "com.roonlabs.transport:2/change_settings",
                Some(serde_json::json!({
                    "zone_or_output_id": zone_or_output_id,
                    "settings": settings
                })),
            )
            .await?;
        Ok(())
    }

    /// Group outputs into a single zone.
    pub async fn group_outputs(&self, output_ids: &[&str]) -> Result<(), ApiError> {
        self.connection
            .send_request(
                "com.roonlabs.transport:2/group_outputs",
                Some(serde_json::json!({
                    "output_ids": output_ids
                })),
            )
            .await?;
        Ok(())
    }

    /// Ungroup outputs from a zone.
    pub async fn ungroup_outputs(&self, output_ids: &[&str]) -> Result<(), ApiError> {
        self.connection
            .send_request(
                "com.roonlabs.transport:2/ungroup_outputs",
                Some(serde_json::json!({
                    "output_ids": output_ids
                })),
            )
            .await?;
        Ok(())
    }

    /// Transfer the current queue from one zone to another.
    pub async fn transfer_zone(
        &self,
        from_zone_or_output_id: &str,
        to_zone_or_output_id: &str,
    ) -> Result<(), ApiError> {
        self.connection
            .send_request(
                "com.roonlabs.transport:2/transfer_zone",
                Some(serde_json::json!({
                    "from_zone_or_output_id": from_zone_or_output_id,
                    "to_zone_or_output_id": to_zone_or_output_id
                })),
            )
            .await?;
        Ok(())
    }

    /// Standby an output.
    pub async fn standby(
        &self,
        output_id: &str,
        control_key: Option<&str>,
    ) -> Result<(), ApiError> {
        let mut body = serde_json::json!({ "output_id": output_id });
        if let Some(ck) = control_key {
            body["control_key"] = serde_json::Value::String(ck.to_string());
        }
        self.connection
            .send_request("com.roonlabs.transport:2/standby", Some(body))
            .await?;
        Ok(())
    }

    /// Toggle standby on an output.
    pub async fn toggle_standby(
        &self,
        output_id: &str,
        control_key: Option<&str>,
    ) -> Result<(), ApiError> {
        let mut body = serde_json::json!({ "output_id": output_id });
        if let Some(ck) = control_key {
            body["control_key"] = serde_json::Value::String(ck.to_string());
        }
        self.connection
            .send_request("com.roonlabs.transport:2/toggle_standby", Some(body))
            .await?;
        Ok(())
    }

    /// Trigger a convenience switch on an output.
    pub async fn convenience_switch(
        &self,
        output_id: &str,
        control_key: Option<&str>,
    ) -> Result<(), ApiError> {
        let mut body = serde_json::json!({ "output_id": output_id });
        if let Some(ck) = control_key {
            body["control_key"] = serde_json::Value::String(ck.to_string());
        }
        self.connection
            .send_request("com.roonlabs.transport:2/convenience_switch", Some(body))
            .await?;
        Ok(())
    }

    /// Pause all zones.
    pub async fn pause_all(&self) -> Result<(), ApiError> {
        self.connection
            .send_request("com.roonlabs.transport:2/pause_all", None)
            .await?;
        Ok(())
    }

    /// Get current zones (one-shot, non-subscription).
    pub async fn get_zones(&self) -> Result<Vec<Zone>, ApiError> {
        let response = self
            .connection
            .send_request("com.roonlabs.transport:2/get_zones", None)
            .await?;
        let body = response
            .json_body()
            .ok_or(ApiError::RegistryFailed("get_zones: no body".into()))?;
        let zones: Vec<Zone> = serde_json::from_value(body["zones"].clone()).unwrap_or_default();
        Ok(zones)
    }

    /// Get current outputs (one-shot, non-subscription).
    pub async fn get_outputs(&self) -> Result<Vec<Output>, ApiError> {
        let response = self
            .connection
            .send_request("com.roonlabs.transport:2/get_outputs", None)
            .await?;
        let body = response
            .json_body()
            .ok_or(ApiError::RegistryFailed("get_outputs: no body".into()))?;
        let outputs: Vec<Output> =
            serde_json::from_value(body["outputs"].clone()).unwrap_or_default();
        Ok(outputs)
    }

    /// Subscribe to a zone's play queue.
    ///
    /// Returns the subscription key — needed for [`Self::unsubscribe_queue`] —
    /// and a receiver yielding [`QueueEvent::Subscribed`] with the current
    /// contents, followed by [`QueueEvent::Changed`] for each later edit.
    ///
    /// `max_item_count` caps how many items the core sends; it is a window on
    /// the front of the queue, not a page size, and there is no way to ask for
    /// a later page.
    pub async fn subscribe_queue(
        &self,
        zone_or_output_id: &str,
        max_item_count: u32,
    ) -> Result<(u32, mpsc::Receiver<QueueEvent>), ApiError> {
        let key = NEXT_QUEUE_SUBSCRIPTION_KEY.fetch_add(1, Ordering::Relaxed);
        let mut raw_rx = self
            .connection
            .subscribe(
                "com.roonlabs.transport:2/subscribe_queue",
                serde_json::json!({
                    "subscription_key": key,
                    "zone_or_output_id": zone_or_output_id,
                    "max_item_count": max_item_count,
                }),
            )
            .await?;

        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            while let Some(msg) = raw_rx.recv().await {
                if msg.verb == MooVerb::Complete {
                    break;
                }
                if let Some(body) = msg.json_body()
                    && let Some(event) = parse_queue_event(&msg.name, body)
                    && tx.send(event).await.is_err()
                {
                    break;
                }
            }
        });

        Ok((key, rx))
    }

    /// End a queue subscription so the core can release it.
    ///
    /// Switching a UI between zones means subscribing to a new queue; without
    /// this the old subscription stays live on the core.
    pub async fn unsubscribe_queue(&self, subscription_key: u32) -> Result<(), ApiError> {
        self.connection
            .send_request(
                "com.roonlabs.transport:2/unsubscribe_queue",
                Some(serde_json::json!({
                    "subscription_key": subscription_key
                })),
            )
            .await?;
        Ok(())
    }

    /// Jump playback to a specific item in the zone's queue, keeping the rest
    /// of the queue intact.
    ///
    /// `queue_item_id` comes from a [`crate::QueueItem`] delivered by
    /// [`Self::subscribe_queue`].
    pub async fn play_from_here(
        &self,
        zone_or_output_id: &str,
        queue_item_id: u64,
    ) -> Result<(), ApiError> {
        self.connection
            .send_request(
                "com.roonlabs.transport:2/play_from_here",
                Some(serde_json::json!({
                    "zone_or_output_id": zone_or_output_id,
                    "queue_item_id": queue_item_id,
                })),
            )
            .await?;
        Ok(())
    }
}

fn parse_zone_event(status: &str, body: &serde_json::Value) -> Option<ZoneEvent> {
    match status {
        "Subscribed" => {
            let zones: Vec<Zone> = serde_json::from_value(body["zones"].clone()).ok()?;
            Some(ZoneEvent::Initial(zones))
        }
        "Changed" => {
            if let Some(zones) = body.get("zones_changed") {
                let zones: Vec<Zone> = serde_json::from_value(zones.clone()).ok()?;
                return Some(ZoneEvent::Changed(zones));
            }
            if let Some(zones) = body.get("zones_added") {
                let zones: Vec<Zone> = serde_json::from_value(zones.clone()).ok()?;
                return Some(ZoneEvent::Added(zones));
            }
            if let Some(zones) = body.get("zones_removed") {
                let ids: Vec<String> = serde_json::from_value(zones.clone()).ok()?;
                return Some(ZoneEvent::Removed(ids));
            }
            if let Some(zones) = body.get("zones_seek_changed") {
                let seeks: Vec<ZoneSeek> = serde_json::from_value(zones.clone()).ok()?;
                return Some(ZoneEvent::Seeked(seeks));
            }
            None
        }
        _ => None,
    }
}

fn parse_output_event(status: &str, body: &serde_json::Value) -> Option<OutputEvent> {
    match status {
        "Subscribed" => {
            let outputs: Vec<Output> = serde_json::from_value(body["outputs"].clone()).ok()?;
            Some(OutputEvent::Initial(outputs))
        }
        "Changed" => {
            if let Some(outputs) = body.get("outputs_changed") {
                let outputs: Vec<Output> = serde_json::from_value(outputs.clone()).ok()?;
                return Some(OutputEvent::Changed(outputs));
            }
            if let Some(outputs) = body.get("outputs_added") {
                let outputs: Vec<Output> = serde_json::from_value(outputs.clone()).ok()?;
                return Some(OutputEvent::Added(outputs));
            }
            if let Some(outputs) = body.get("outputs_removed") {
                let ids: Vec<String> = serde_json::from_value(outputs.clone()).ok()?;
                return Some(OutputEvent::Removed(ids));
            }
            None
        }
        _ => None,
    }
}
