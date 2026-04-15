//! Browse → play top hit → stop → transfer zone.
//!
//! Usage:
//!   cargo run -p roon-api --example browse_play -- --host 192.168.1.20 --port 9330

use roon_api::{
    BrowseOptions, ControlAction, FileTokenStore, LoadOptions, PlayState,
    RoonClientBuilder, ZoneEvent,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let host = args.iter().position(|a| a == "--host").map(|i| &args[i + 1]);
    let port: Option<u16> = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args[i + 1].parse().ok());

    let token_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("roon-rs")
        .join("tokens.json");

    let client = RoonClientBuilder::new(
        "com.roon-rs.browse_play",
        "roon-rs Browse+Play Test",
        "0.1.0",
        "roon-rs",
        "dev@example.com",
    )
    .token_store(FileTokenStore::new(&token_path))
    .require_browse()
    .require_transport()
    .build()?;

    let core = match (host, port) {
        (Some(h), Some(p)) => client.connect(h, p).await?,
        _ => {
            println!("Usage: --host <ip> --port <port>");
            return Ok(());
        }
    };

    let browse_svc = core.browse();
    let transport = core.transport();

    // Get zones first
    let mut zone_rx = transport.subscribe_zones().await?;
    let zones = match zone_rx.recv().await {
        Some(ZoneEvent::Initial(z)) => z,
        _ => anyhow::bail!("no zones"),
    };

    println!("=== Zones ===");
    for (i, z) in zones.iter().enumerate() {
        println!("  [{}] {} ({:?})", i, z.display_name, z.state);
    }

    let zone = &zones[0];
    let zone_id = &zone.zone_id;
    println!("\nUsing zone: {}", zone.display_name);

    // Step 1: Search for "bach"
    println!("\n--- Step 1: Search \"bach\" ---");
    let result = browse_svc
        .browse(BrowseOptions {
            hierarchy: Some("search".into()),
            pop_all: Some(true),
            input: Some("bach".into()),
            zone_or_output_id: Some(zone_id.clone()),
            ..Default::default()
        })
        .await?;
    println!("Action: {}", result.action);
    if let Some(list) = &result.list {
        println!("List: {} ({} items)", list.title, list.count);
    }

    let items = browse_svc
        .load(LoadOptions {
            hierarchy: Some("search".into()),
            count: Some(10),
            ..Default::default()
        })
        .await?;

    println!("Results:");
    for item in &items.items {
        println!(
            "  {} {}{}",
            item.title,
            item.subtitle.as_deref().map(|s| format!("— {} ", s)).unwrap_or_default(),
            item.hint.as_deref().map(|h| format!("[{}]", h)).unwrap_or_default(),
        );
    }

    // Step 2: Navigate into the top hit (first item, usually an album or top result)
    let top_hit = items
        .items
        .first()
        .and_then(|i| i.item_key.as_ref())
        .ok_or_else(|| anyhow::anyhow!("no browseable top hit"))?;

    println!("\n--- Step 2: Select top hit: {} ---", items.items[0].title);
    let result = browse_svc
        .browse(BrowseOptions {
            hierarchy: Some("search".into()),
            item_key: Some(top_hit.clone()),
            zone_or_output_id: Some(zone_id.clone()),
            ..Default::default()
        })
        .await?;
    println!("Action: {}", result.action);
    if let Some(list) = &result.list {
        println!("List: {} ({} items)", list.title, list.count);
    }

    let items = browse_svc
        .load(LoadOptions {
            hierarchy: Some("search".into()),
            count: Some(10),
            ..Default::default()
        })
        .await?;

    println!("Items:");
    for item in &items.items {
        println!(
            "  {} {}{}",
            item.title,
            item.subtitle.as_deref().map(|s| format!("— {} ", s)).unwrap_or_default(),
            item.hint.as_deref().map(|h| format!("[{}]", h)).unwrap_or_default(),
        );
    }

    // Step 3: Select the first playable item to get action list
    let first_item_key = items
        .items
        .first()
        .and_then(|i| i.item_key.as_ref())
        .ok_or_else(|| anyhow::anyhow!("no item to select"))?;

    println!("\n--- Step 3: Select \"{}\" ---", items.items[0].title);
    let result = browse_svc
        .browse(BrowseOptions {
            hierarchy: Some("search".into()),
            item_key: Some(first_item_key.clone()),
            zone_or_output_id: Some(zone_id.clone()),
            ..Default::default()
        })
        .await?;
    println!("Action: {}", result.action);
    if let Some(list) = &result.list {
        println!("List: {} ({} items)", list.title, list.count);
    }

    let items = browse_svc
        .load(LoadOptions {
            hierarchy: Some("search".into()),
            count: Some(10),
            ..Default::default()
        })
        .await?;

    println!("Action items:");
    for item in &items.items {
        println!(
            "  {} {}",
            item.title,
            item.hint.as_deref().map(|h| format!("[{}]", h)).unwrap_or_default(),
        );
    }

    // Step 4: Select "Play Album" to get sub-actions (Play Now, Add Next, Queue, etc.)
    let play_album_key = items
        .items
        .iter()
        .find(|i| i.title.contains("Play"))
        .and_then(|i| i.item_key.as_ref())
        .ok_or_else(|| anyhow::anyhow!("no Play Album action found"))?;

    println!("\n--- Step 4a: Select \"Play Album\" ---");
    let result = browse_svc
        .browse(BrowseOptions {
            hierarchy: Some("search".into()),
            item_key: Some(play_album_key.clone()),
            zone_or_output_id: Some(zone_id.clone()),
            ..Default::default()
        })
        .await?;
    println!("Action: {}", result.action);
    if let Some(list) = &result.list {
        println!("List: {} ({} items)", list.title, list.count);
    }

    let items = browse_svc
        .load(LoadOptions {
            hierarchy: Some("search".into()),
            count: Some(10),
            ..Default::default()
        })
        .await?;

    println!("Sub-actions:");
    for item in &items.items {
        println!("  {}", item.title);
    }

    // Select "Play Now" from the sub-actions
    let play_now_key = items
        .items
        .iter()
        .find(|i| i.title == "Play Now")
        .or(items.items.first())
        .and_then(|i| i.item_key.as_ref())
        .ok_or_else(|| anyhow::anyhow!("no Play Now action found"))?;

    let play_title = items
        .items
        .iter()
        .find(|i| i.title == "Play Now")
        .or(items.items.first())
        .map(|i| i.title.as_str())
        .unwrap_or("?");

    println!("\n--- Step 4b: Execute \"{}\" ---", play_title);
    let result = browse_svc
        .browse(BrowseOptions {
            hierarchy: Some("search".into()),
            item_key: Some(play_now_key.clone()),
            zone_or_output_id: Some(zone_id.clone()),
            ..Default::default()
        })
        .await?;
    println!("Action: {}", result.action);

    // Check if there's a result message
    if let Some(list) = &result.list {
        println!("List: {} ({} items)", list.title, list.count);
        let post_items = browse_svc
            .load(LoadOptions {
                hierarchy: Some("search".into()),
                count: Some(5),
                ..Default::default()
            })
            .await;
        if let Ok(post_items) = post_items {
            for item in &post_items.items {
                println!("  {}", item.title);
            }
        }
    }

    // Wait for playback to start
    println!("\nWaiting for playback...");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut playing = false;
    loop {
        match tokio::time::timeout_at(deadline, zone_rx.recv()).await {
            Ok(Some(ZoneEvent::Changed(zones))) => {
                for z in &zones {
                    if z.zone_id == *zone_id && z.state == PlayState::Playing {
                        if let Some(np) = &z.now_playing {
                            println!("  Playing: {}", np.one_line.line1);
                        }
                        playing = true;
                        break;
                    }
                }
                if playing {
                    break;
                }
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    if !playing {
        println!("  (no playback state change detected within 5s)");
    }

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Step 5: Stop playback
    println!("\n--- Step 5: Stop ---");
    transport.control(zone_id, ControlAction::Stop).await?;
    println!("  Stopped");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Step 6: Transfer zone (if more than one zone)
    if zones.len() >= 2 {
        let from_zone = &zones[0];
        let to_zone = &zones[1];
        println!(
            "\n--- Step 6: Transfer zone {} → {} ---",
            from_zone.display_name, to_zone.display_name
        );
        transport
            .transfer_zone(&from_zone.zone_id, &to_zone.zone_id)
            .await?;
        println!("  Transfer sent");

        // Wait for change event
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match tokio::time::timeout_at(deadline, zone_rx.recv()).await {
                Ok(Some(ZoneEvent::Changed(zones))) => {
                    for z in &zones {
                        println!("  [Changed] {} → {:?}", z.display_name, z.state);
                    }
                }
                Ok(Some(_)) => {}
                _ => break,
            }
        }
    } else {
        println!("\n--- Step 6: Skipped (need 2+ zones for transfer) ---");
    }

    println!("\nDone!");
    Ok(())
}
