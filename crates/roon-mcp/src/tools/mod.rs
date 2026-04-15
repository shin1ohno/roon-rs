use rmcp::{schemars, tool, tool_router};
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;

use std::sync::Arc;
use tokio::sync::Mutex;

use roon_api::{
    Browse, BrowseOptions, ControlAction, LoadOptions, MuteAction, SeekMode, Transport,
    VolumeMode, Zone,
};

pub struct RoonMcpServer {
    pub transport: Arc<Mutex<Option<Transport>>>,
    pub browse: Arc<Mutex<Option<Browse>>>,
    pub zones: Arc<Mutex<Vec<Zone>>>,
}

// --- Input types ---

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ZoneInput {
    pub zone_or_output_id: String,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SeekInput {
    pub zone_or_output_id: String,
    pub seconds: i64,
    #[serde(default = "default_relative")]
    pub how: String,
}
fn default_relative() -> String { "relative".to_string() }

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct VolumeInput {
    pub output_id: String,
    pub value: f64,
    #[serde(default = "default_absolute")]
    pub how: String,
}
fn default_absolute() -> String { "absolute".to_string() }

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct MuteInput {
    pub output_id: String,
    /// "mute" or "unmute"
    pub action: String,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct TransferInput {
    pub from_zone_or_output_id: String,
    pub to_zone_or_output_id: String,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SettingsInput {
    pub zone_or_output_id: String,
    pub shuffle: Option<bool>,
    pub r#loop: Option<String>,
    pub auto_radio: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct BrowseInput {
    pub hierarchy: Option<String>,
    pub item_key: Option<String>,
    pub pop_all: Option<bool>,
    pub zone_or_output_id: Option<String>,
    pub input: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct LoadInput {
    pub hierarchy: Option<String>,
    pub offset: Option<u32>,
    pub count: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SearchPlayInput {
    pub query: String,
    pub zone_or_output_id: String,
}

// --- Tool implementations ---

#[tool_router(server_handler)]
impl RoonMcpServer {
    #[tool(name = "list_zones", description = "List all Roon playback zones with their current state, now playing info, and volume")]
    async fn list_zones(&self) -> String {
        let zones = self.zones.lock().await;
        let summary: Vec<serde_json::Value> = zones.iter().map(|z| {
            let mut info = serde_json::json!({
                "zone_id": z.zone_id,
                "display_name": z.display_name,
                "state": z.state,
            });
            if let Some(np) = &z.now_playing {
                info["now_playing"] = serde_json::json!(np.one_line.line1);
                if let Some(len) = np.length { info["length"] = serde_json::json!(len); }
            }
            if let Some(pos) = z.seek_position { info["seek_position"] = serde_json::json!(pos); }
            for output in &z.outputs {
                if let Some(vol) = &output.volume {
                    info["volume"] = serde_json::json!(vol.value);
                    info["is_muted"] = serde_json::json!(vol.is_muted);
                }
                info["output_id"] = serde_json::json!(output.output_id);
            }
            info
        }).collect();
        json_str(serde_json::json!({"zones": summary}))
    }

    #[tool(name = "play", description = "Start playback in a zone")]
    async fn play(&self, Parameters(input): Parameters<ZoneInput>) -> String {
        self.transport_cmd(&input.zone_or_output_id, ControlAction::Play).await
    }

    #[tool(name = "pause", description = "Pause playback in a zone")]
    async fn pause(&self, Parameters(input): Parameters<ZoneInput>) -> String {
        self.transport_cmd(&input.zone_or_output_id, ControlAction::Pause).await
    }

    #[tool(name = "stop", description = "Stop playback in a zone")]
    async fn stop(&self, Parameters(input): Parameters<ZoneInput>) -> String {
        self.transport_cmd(&input.zone_or_output_id, ControlAction::Stop).await
    }

    #[tool(name = "next", description = "Skip to next track")]
    async fn next(&self, Parameters(input): Parameters<ZoneInput>) -> String {
        self.transport_cmd(&input.zone_or_output_id, ControlAction::Next).await
    }

    #[tool(name = "previous", description = "Skip to previous track")]
    async fn previous(&self, Parameters(input): Parameters<ZoneInput>) -> String {
        self.transport_cmd(&input.zone_or_output_id, ControlAction::Previous).await
    }

    #[tool(name = "pause_all", description = "Pause all zones")]
    async fn pause_all(&self) -> String {
        let t = self.transport.lock().await;
        match t.as_ref() {
            Some(t) => match t.pause_all().await {
                Ok(_) => "All paused".into(),
                Err(e) => format!("Error: {}", e),
            },
            None => "Not connected".into(),
        }
    }

    #[tool(name = "seek", description = "Seek within the currently playing track")]
    async fn seek(&self, Parameters(input): Parameters<SeekInput>) -> String {
        let t = self.transport.lock().await;
        match t.as_ref() {
            Some(t) => {
                let mode = if input.how == "absolute" { SeekMode::Absolute } else { SeekMode::Relative };
                match t.seek(&input.zone_or_output_id, mode, input.seconds).await {
                    Ok(_) => format!("Seeked {}s ({})", input.seconds, input.how),
                    Err(e) => format!("Error: {}", e),
                }
            }
            None => "Not connected".into(),
        }
    }

    #[tool(name = "change_volume", description = "Change volume of an output")]
    async fn change_volume(&self, Parameters(input): Parameters<VolumeInput>) -> String {
        let t = self.transport.lock().await;
        match t.as_ref() {
            Some(t) => {
                let mode = match input.how.as_str() {
                    "relative" => VolumeMode::Relative,
                    "relative_step" => VolumeMode::RelativeStep,
                    _ => VolumeMode::Absolute,
                };
                match t.change_volume(&input.output_id, mode, input.value).await {
                    Ok(_) => format!("Volume: {} ({})", input.value, input.how),
                    Err(e) => format!("Error: {}", e),
                }
            }
            None => "Not connected".into(),
        }
    }

    #[tool(name = "mute", description = "Mute or unmute an output")]
    async fn mute(&self, Parameters(input): Parameters<MuteInput>) -> String {
        let t = self.transport.lock().await;
        match t.as_ref() {
            Some(t) => {
                let action = if input.action == "unmute" { MuteAction::Unmute } else { MuteAction::Mute };
                match t.mute(&input.output_id, action).await {
                    Ok(_) => format!("Mute: {}", input.action),
                    Err(e) => format!("Error: {}", e),
                }
            }
            None => "Not connected".into(),
        }
    }

    #[tool(name = "transfer_zone", description = "Transfer the current queue from one zone to another")]
    async fn transfer_zone(&self, Parameters(input): Parameters<TransferInput>) -> String {
        let t = self.transport.lock().await;
        match t.as_ref() {
            Some(t) => match t.transfer_zone(&input.from_zone_or_output_id, &input.to_zone_or_output_id).await {
                Ok(_) => "Transferred".into(),
                Err(e) => format!("Error: {}", e),
            },
            None => "Not connected".into(),
        }
    }

    #[tool(name = "change_settings", description = "Change zone settings: shuffle, loop (loop/loop_one/disabled), auto_radio")]
    async fn change_settings(&self, Parameters(input): Parameters<SettingsInput>) -> String {
        let t = self.transport.lock().await;
        match t.as_ref() {
            Some(t) => match t.change_settings(&input.zone_or_output_id, input.shuffle, input.r#loop.as_deref(), input.auto_radio).await {
                Ok(_) => "Settings updated".into(),
                Err(e) => format!("Error: {}", e),
            },
            None => "Not connected".into(),
        }
    }

    #[tool(name = "browse", description = "Browse the Roon music library. Use hierarchy='browse' for library, 'search' for search. Navigate with item_key. Set input for search queries.")]
    async fn browse(&self, Parameters(input): Parameters<BrowseInput>) -> String {
        let b = self.browse.lock().await;
        match b.as_ref() {
            Some(b) => {
                let opts = BrowseOptions {
                    hierarchy: input.hierarchy,
                    item_key: input.item_key,
                    pop_all: input.pop_all,
                    zone_or_output_id: input.zone_or_output_id,
                    input: input.input,
                    ..Default::default()
                };
                match b.browse(opts).await {
                    Ok(result) => json_str(serde_json::to_value(&result).unwrap_or_default()),
                    Err(e) => json_str(serde_json::json!({"error": e.to_string()})),
                }
            }
            None => json_str(serde_json::json!({"error": "Not connected"})),
        }
    }

    #[tool(name = "load", description = "Load items from the current browse list. Use after browse to get items.")]
    async fn load(&self, Parameters(input): Parameters<LoadInput>) -> String {
        let b = self.browse.lock().await;
        match b.as_ref() {
            Some(b) => {
                let opts = LoadOptions {
                    hierarchy: input.hierarchy,
                    offset: input.offset,
                    count: input.count,
                    ..Default::default()
                };
                match b.load(opts).await {
                    Ok(result) => json_str(serde_json::to_value(&result).unwrap_or_default()),
                    Err(e) => json_str(serde_json::json!({"error": e.to_string()})),
                }
            }
            None => json_str(serde_json::json!({"error": "Not connected"})),
        }
    }

    #[tool(name = "search_and_play", description = "Search for music and play the top result. Navigates search → album → Play Now automatically.")]
    async fn search_and_play(&self, Parameters(input): Parameters<SearchPlayInput>) -> String {
        let b = self.browse.lock().await;
        let b = match b.as_ref() {
            Some(b) => b,
            None => return json_str(serde_json::json!({"error": "Not connected"})),
        };

        // Search
        if let Err(e) = b.browse(BrowseOptions {
            hierarchy: Some("search".into()),
            pop_all: Some(true),
            input: Some(input.query.clone()),
            zone_or_output_id: Some(input.zone_or_output_id.clone()),
            ..Default::default()
        }).await {
            return json_str(serde_json::json!({"error": format!("Search failed: {}", e)}));
        }

        let items = match b.load(LoadOptions { hierarchy: Some("search".into()), count: Some(5), ..Default::default() }).await {
            Ok(r) => r,
            Err(e) => return json_str(serde_json::json!({"error": format!("Load failed: {}", e)})),
        };

        let top_key = match items.items.first().and_then(|i| i.item_key.as_ref()) {
            Some(k) => k.clone(),
            None => return json_str(serde_json::json!({"error": "No results"})),
        };
        let top_title = items.items[0].title.clone();

        // Navigate to top hit
        let _ = b.browse(BrowseOptions { hierarchy: Some("search".into()), item_key: Some(top_key), zone_or_output_id: Some(input.zone_or_output_id.clone()), ..Default::default() }).await;
        let items = match b.load(LoadOptions { hierarchy: Some("search".into()), count: Some(5), ..Default::default() }).await {
            Ok(r) => r,
            Err(_) => return json_str(serde_json::json!({"result": "partial", "top_hit": top_title})),
        };

        // Navigate deeper + find Play action
        if let Some(first_key) = items.items.first().and_then(|i| i.item_key.as_ref()) {
            let _ = b.browse(BrowseOptions { hierarchy: Some("search".into()), item_key: Some(first_key.clone()), zone_or_output_id: Some(input.zone_or_output_id.clone()), ..Default::default() }).await;
            if let Ok(items) = b.load(LoadOptions { hierarchy: Some("search".into()), count: Some(10), ..Default::default() }).await {
                if let Some(play_key) = items.items.iter().find(|i| i.title.contains("Play")).and_then(|i| i.item_key.as_ref()) {
                    let _ = b.browse(BrowseOptions { hierarchy: Some("search".into()), item_key: Some(play_key.clone()), zone_or_output_id: Some(input.zone_or_output_id.clone()), ..Default::default() }).await;
                    // Handle "Play Album" → "Play Now" sub-menu
                    if let Ok(sub) = b.load(LoadOptions { hierarchy: Some("search".into()), count: Some(5), ..Default::default() }).await {
                        if let Some(play_now) = sub.items.iter().find(|i| i.title == "Play Now").and_then(|i| i.item_key.as_ref()) {
                            let _ = b.browse(BrowseOptions { hierarchy: Some("search".into()), item_key: Some(play_now.clone()), zone_or_output_id: Some(input.zone_or_output_id.clone()), ..Default::default() }).await;
                        }
                    }
                }
            }
        }

        json_str(serde_json::json!({"result": "Playing", "query": input.query, "top_hit": top_title}))
    }
}

fn json_str(v: serde_json::Value) -> String {
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

impl RoonMcpServer {
    async fn transport_cmd(&self, zone_or_output_id: &str, action: ControlAction) -> String {
        let t = self.transport.lock().await;
        match t.as_ref() {
            Some(t) => match t.control(zone_or_output_id, action).await {
                Ok(_) => format!("{:?}", action),
                Err(e) => format!("Error: {}", e),
            },
            None => "Not connected".into(),
        }
    }
}
