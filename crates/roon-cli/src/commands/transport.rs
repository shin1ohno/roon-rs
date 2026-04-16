use anyhow::{bail, Result};
use roon_api::{
    BrowseOptions, ControlAction, Core, LoadOptions, MuteAction, SeekMode, VolumeMode,
};

use crate::output_format;
use crate::resolve;

pub async fn status(
    core: &Core,
    zone_name: Option<&str>,
    zone_id: Option<&str>,
    json: bool,
) -> Result<()> {
    let transport = core.transport();
    let zones = transport.get_zones().await?;
    let zid = resolve::get_zone_id(&zones, zone_name, zone_id)?;
    let zone = zones
        .iter()
        .find(|z| z.zone_id == zid)
        .ok_or_else(|| anyhow::anyhow!("Zone {} not found", zid))?;

    if json {
        println!("{}", serde_json::to_string_pretty(zone)?);
        return Ok(());
    }

    println!("Zone: {}", zone.display_name);
    println!("State: {:?}", zone.state);
    if let Some(np) = &zone.now_playing {
        println!("Now playing: {}", np.one_line.line1);
        if let Some(two) = &np.two_line {
            if let Some(line2) = &two.line2 {
                println!("             {}", line2);
            }
        }
        if let (Some(pos), Some(len)) = (np.seek_position, np.length) {
            println!("Position: {:.0}s / {:.0}s", pos, len);
        }
    } else {
        println!("(nothing queued)");
    }
    Ok(())
}

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
    let debug = std::env::var("ROON_DEBUG").is_ok();

    let query = match (album, artist) {
        (Some(a), Some(ar)) => format!("{} {}", ar, a),
        (Some(a), None) => a.to_string(),
        (None, Some(ar)) => ar.to_string(),
        (None, None) => bail!("No search criteria provided."),
    };

    if debug {
        eprintln!("[DEBUG] Searching for: {}", query);
    }

    // Step 1: enter search hierarchy with query
    browse
        .browse(BrowseOptions {
            hierarchy: Some("search".to_string()),
            pop_all: Some(true),
            input: Some(query.clone()),
            zone_or_output_id: Some(zone_id.to_string()),
            ..Default::default()
        })
        .await?;

    let items = browse
        .load(LoadOptions {
            hierarchy: Some("search".to_string()),
            count: Some(10),
            ..Default::default()
        })
        .await?;

    if debug {
        eprintln!("[DEBUG] Search results (list: {:?}):", items.list.as_ref().map(|l| &l.title));
        for (i, it) in items.items.iter().enumerate() {
            eprintln!(
                "[DEBUG]   [{}] title='{}' subtitle={:?} hint={:?} has_key={}",
                i, it.title, it.subtitle, it.hint, it.item_key.is_some()
            );
        }
    }

    let top = items
        .items
        .first()
        .ok_or_else(|| anyhow::anyhow!("No results for '{}'.", query))?;
    let top_title = top.title.clone();
    let top_key = top
        .item_key
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Top hit has no item_key"))?;

    let mut next_key = top_key;
    let max_depth = 5;

    for depth in 0..max_depth {
        let nav = browse
            .browse(BrowseOptions {
                hierarchy: Some("search".to_string()),
                item_key: Some(next_key.clone()),
                zone_or_output_id: Some(zone_id.to_string()),
                ..Default::default()
            })
            .await?;

        if debug {
            eprintln!(
                "[DEBUG] depth={} browse action='{}' list={:?}",
                depth,
                nav.action,
                nav.list.as_ref().map(|l| &l.title)
            );
        }

        let page = browse
            .load(LoadOptions {
                hierarchy: Some("search".to_string()),
                count: Some(20),
                ..Default::default()
            })
            .await?;

        if debug {
            for (i, it) in page.items.iter().enumerate() {
                eprintln!(
                    "[DEBUG]   depth={} [{}] title='{}' hint={:?} has_key={}",
                    depth, i, it.title, it.hint, it.item_key.is_some()
                );
            }
        }

        // Priority 1: trigger a playable action_list that has a Play Now submenu.
        // Skip "Play Artist" and "Play Genre" because their submenus don't include
        // Play Now (only Shuffle/Start Radio).
        let playable_action = page.items.iter().find(|i| {
            matches!(
                i.title.as_str(),
                "Play Album" | "Play Now" | "Play Track" | "Play From Here" | "Play Work"
            )
        });

        if let Some(action_item) = playable_action {
            if debug {
                eprintln!("[DEBUG] Found direct play action: '{}'", action_item.title);
            }
            let action_key = action_item
                .item_key
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Action has no item_key"))?;

            let play_result = browse
                .browse(BrowseOptions {
                    hierarchy: Some("search".to_string()),
                    item_key: Some(action_key),
                    zone_or_output_id: Some(zone_id.to_string()),
                    ..Default::default()
                })
                .await?;

            if debug {
                eprintln!("[DEBUG] Play action result: action='{}'", play_result.action);
            }

            // If the result is a list (action_list), find "Play Now" submenu
            if play_result.action == "list" {
                let sub = browse
                    .load(LoadOptions {
                        hierarchy: Some("search".to_string()),
                        count: Some(10),
                        ..Default::default()
                    })
                    .await?;

                if debug {
                    for (i, it) in sub.items.iter().enumerate() {
                        eprintln!("[DEBUG]   sub [{}] title='{}'", i, it.title);
                    }
                }

                if let Some(play_now_key) = sub
                    .items
                    .iter()
                    .find(|i| i.title == "Play Now")
                    .and_then(|i| i.item_key.clone())
                {
                    browse
                        .browse(BrowseOptions {
                            hierarchy: Some("search".to_string()),
                            item_key: Some(play_now_key),
                            zone_or_output_id: Some(zone_id.to_string()),
                            ..Default::default()
                        })
                        .await?;
                    println!("Playing: {}", top_title);
                    return Ok(());
                }
                bail!(
                    "'{}' has no 'Play Now' submenu (found: {})",
                    action_item.title,
                    sub.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>().join(", ")
                );
            }

            // Direct action executed (action != "list")
            println!("Playing: {}", top_title);
            return Ok(());
        }

        // Priority 2: descend into the first list-hinted item (album/track/etc.),
        // skipping action-hinted items that don't give us Play Now access.
        let descend = page.items.iter().find(|i| {
            i.hint.as_deref() == Some("list") && i.item_key.is_some()
        });

        next_key = descend
            .and_then(|i| i.item_key.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "depth {}: no playable action and no list items to descend into. Items: {}",
                    depth,
                    page.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>().join(", ")
                )
            })?;

        if debug {
            eprintln!("[DEBUG] Descending into next item with key: {}", next_key);
        }
    }

    bail!("Could not find Play action within {} levels.", max_depth)
}
