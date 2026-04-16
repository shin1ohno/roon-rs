use anyhow::{bail, Result};
use roon_api::{
    BrowseOptions, ControlAction, Core, LoadOptions, MuteAction, SeekMode, VolumeMode,
};

use crate::output_format;
use crate::resolve;

pub async fn zones(
    core: &Core,
    json: bool,
) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    output_format::print_zones(&zones, json);
    Ok(())
}

pub async fn outputs(
    core: &Core,
    json: bool,
) -> Result<()> {
    let transport = core.transport();
    let outputs = transport.get_outputs().await?;
    output_format::print_outputs(&outputs, json);
    Ok(())
}

pub async fn play(
    core: &Core,
    zone_name: Option<&str>,
    zone_id: Option<&str>,
    album: Option<&str>,
    artist: Option<&str>,
) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let zid = resolve::get_zone_id(&zones, zone_name, zone_id)?;

    // If search filters provided, use browse to find and play
    if album.is_some() || artist.is_some() {
        return search_and_play(core, &zid, album, artist).await;
    }

    transport.control(&zid, ControlAction::Play).await?;
    println!("Playing.");
    Ok(())
}

pub async fn pause(core: &Core, zone_name: Option<&str>, zone_id: Option<&str>) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let zid = resolve::get_zone_id(&zones, zone_name, zone_id)?;
    transport.control(&zid, ControlAction::Pause).await?;
    println!("Paused.");
    Ok(())
}

pub async fn stop(core: &Core, zone_name: Option<&str>, zone_id: Option<&str>) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let zid = resolve::get_zone_id(&zones, zone_name, zone_id)?;
    transport.control(&zid, ControlAction::Stop).await?;
    println!("Stopped.");
    Ok(())
}

pub async fn next(core: &Core, zone_name: Option<&str>, zone_id: Option<&str>) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let zid = resolve::get_zone_id(&zones, zone_name, zone_id)?;
    transport.control(&zid, ControlAction::Next).await?;
    println!("Next track.");
    Ok(())
}

pub async fn previous(core: &Core, zone_name: Option<&str>, zone_id: Option<&str>) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let zid = resolve::get_zone_id(&zones, zone_name, zone_id)?;
    transport.control(&zid, ControlAction::Previous).await?;
    println!("Previous track.");
    Ok(())
}

pub async fn seek(
    core: &Core,
    seconds: i64,
    relative: bool,
    zone_name: Option<&str>,
    zone_id: Option<&str>,
) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let zid = resolve::get_zone_id(&zones, zone_name, zone_id)?;
    let mode = if relative {
        SeekMode::Relative
    } else {
        SeekMode::Absolute
    };
    transport.seek(&zid, mode, seconds).await?;
    println!("Seeked to {}s ({}).", seconds, if relative { "relative" } else { "absolute" });
    Ok(())
}

pub async fn volume(
    core: &Core,
    value: f64,
    relative: bool,
    output_name: Option<&str>,
    output_id_flag: Option<&str>,
) -> Result<()> {
    let transport = core.transport();
    let outputs = transport.get_outputs().await?;
    let oid = resolve::get_output_id(&outputs, output_name, output_id_flag)?;
    let mode = if relative {
        VolumeMode::Relative
    } else {
        VolumeMode::Absolute
    };
    transport.change_volume(&oid, mode, value).await?;
    println!("Volume set.");
    Ok(())
}

pub async fn mute(
    core: &Core,
    on: bool,
    output_name: Option<&str>,
    output_id_flag: Option<&str>,
) -> Result<()> {
    let transport = core.transport();
    let outputs = transport.get_outputs().await?;
    let oid = resolve::get_output_id(&outputs, output_name, output_id_flag)?;
    let action = if on { MuteAction::Mute } else { MuteAction::Unmute };
    transport.mute(&oid, action).await?;
    println!("{}.", if on { "Muted" } else { "Unmuted" });
    Ok(())
}

pub async fn pause_all(core: &Core) -> Result<()> {
    let transport = core.transport();
    transport.pause_all().await?;
    println!("All zones paused.");
    Ok(())
}

pub async fn transfer(
    core: &Core,
    from_zone: &str,
    to_zone: &str,
) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let from_id = resolve::resolve_zone(&zones, from_zone)?.zone_id.clone();
    let to_id = resolve::resolve_zone(&zones, to_zone)?.zone_id.clone();
    transport.transfer_zone(&from_id, &to_id).await?;
    println!("Transferred from '{}' to '{}'.", from_zone, to_zone);
    Ok(())
}

pub async fn settings(
    core: &Core,
    zone_name: Option<&str>,
    zone_id: Option<&str>,
    shuffle: Option<bool>,
    loop_mode: Option<&str>,
    auto_radio: Option<bool>,
) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let zid = resolve::get_zone_id(&zones, zone_name, zone_id)?;
    transport
        .change_settings(&zid, shuffle, loop_mode, auto_radio)
        .await?;
    println!("Settings updated.");
    Ok(())
}

pub async fn group(core: &Core, output_names: &[String]) -> Result<()> {
    let transport = core.transport();
    let outputs = transport.get_outputs().await?;
    let ids: Vec<String> = output_names
        .iter()
        .map(|n| resolve::resolve_output(&outputs, n).map(|o| o.output_id.clone()))
        .collect::<Result<Vec<_>>>()?;
    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    transport.group_outputs(&id_refs).await?;
    println!("Outputs grouped.");
    Ok(())
}

pub async fn ungroup(core: &Core, output_names: &[String]) -> Result<()> {
    let transport = core.transport();
    let outputs = transport.get_outputs().await?;
    let ids: Vec<String> = output_names
        .iter()
        .map(|n| resolve::resolve_output(&outputs, n).map(|o| o.output_id.clone()))
        .collect::<Result<Vec<_>>>()?;
    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    transport.ungroup_outputs(&id_refs).await?;
    println!("Outputs ungrouped.");
    Ok(())
}

async fn search_and_play(
    core: &Core,
    zone_id: &str,
    album: Option<&str>,
    artist: Option<&str>,
) -> Result<()> {
    let browse = core.browse();

    // Build search query from filters
    let query = match (album, artist) {
        (Some(a), Some(ar)) => format!("{} {}", ar, a),
        (Some(a), None) => a.to_string(),
        (None, Some(ar)) => ar.to_string(),
        (None, None) => bail!("No search criteria provided."),
    };

    // Browse to search hierarchy
    let result = browse
        .browse(BrowseOptions {
            hierarchy: Some("search".to_string()),
            zone_or_output_id: Some(zone_id.to_string()),
            input: Some(query.clone()),
            pop_all: Some(true),
            ..Default::default()
        })
        .await?;

    let list = result
        .list
        .as_ref()
        .filter(|l| l.count > 0);

    if list.is_none() {
        bail!("No results for '{}'.", query);
    }

    // Load top results
    let load_result = browse
        .load(LoadOptions {
            hierarchy: Some("search".to_string()),
            count: Some(5),
            ..Default::default()
        })
        .await?;

    if load_result.items.is_empty() {
        bail!("No results for '{}'.", query);
    }

    // Navigate into first result
    let first = &load_result.items[0];
    let item_key = first
        .item_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("First result has no item_key"))?;

    let nav_result = browse
        .browse(BrowseOptions {
            hierarchy: Some("search".to_string()),
            item_key: Some(item_key.clone()),
            zone_or_output_id: Some(zone_id.to_string()),
            ..Default::default()
        })
        .await?;

    // Look for a Play action in the result
    if nav_result.action == "list" {
        // Load action items
        let actions = browse
            .load(LoadOptions {
                hierarchy: Some("search".to_string()),
                count: Some(10),
                ..Default::default()
            })
            .await?;

        // Find "Play" or similar action
        for item in &actions.items {
            let title_lower = item.title.to_lowercase();
            if title_lower.contains("play") {
                if let Some(key) = &item.item_key {
                    browse
                        .browse(BrowseOptions {
                            hierarchy: Some("search".to_string()),
                            item_key: Some(key.clone()),
                            zone_or_output_id: Some(zone_id.to_string()),
                            ..Default::default()
                        })
                        .await?;

                    // Try to find "Play Now"
                    let sub_actions = browse
                        .load(LoadOptions {
                            hierarchy: Some("search".to_string()),
                            count: Some(10),
                            ..Default::default()
                        })
                        .await?;

                    for sub in &sub_actions.items {
                        if sub.title.to_lowercase().contains("play now")
                            || sub.title.to_lowercase() == "play"
                        {
                            if let Some(sub_key) = &sub.item_key {
                                browse
                                    .browse(BrowseOptions {
                                        hierarchy: Some("search".to_string()),
                                        item_key: Some(sub_key.clone()),
                                        zone_or_output_id: Some(zone_id.to_string()),
                                        ..Default::default()
                                    })
                                    .await?;
                                println!("Playing: {}", first.title);
                                return Ok(());
                            }
                        }
                    }

                    // If no "Play Now" submenu, the Play action itself might work
                    println!("Playing: {}", first.title);
                    return Ok(());
                }
            }
        }

        bail!("Could not find a Play action for '{}'.", first.title);
    }

    println!("Playing: {}", first.title);
    Ok(())
}
