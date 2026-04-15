//! Example: discover Roon Core, list zones, and control playback.
//!
//! Usage:
//!   cargo run -p roon-api --example list_zones
//!
//! Or connect directly to a known core:
//!   cargo run -p roon-api --example list_zones -- --host 192.168.1.10 --port 9100

use roon_api::{
    ControlAction, FileTokenStore, PlayState, RoonClientBuilder, RoonEvent, ZoneEvent,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let (host, port) = parse_args(&args);

    let token_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("roon-rs")
        .join("tokens.json");

    println!("Token store: {}", token_path.display());

    let client = RoonClientBuilder::new(
        "com.roon-rs.list_zones",
        "roon-rs Zone Lister",
        "0.1.0",
        "roon-rs",
        "dev@example.com",
    )
    .token_store(FileTokenStore::new(&token_path))
    .require_transport()
    .build()?;

    let mut events = client.events();

    if let Some((host, port)) = host.zip(port) {
        println!("Connecting directly to {}:{}", host, port);
        let core = client.connect(&host, port).await?;
        run_transport(core).await?;
    } else {
        println!("Starting SOOD discovery...");
        println!("(Make sure Roon Core is running on the same network)");
        client.start_discovery().await?;

        loop {
            match events.recv().await? {
                RoonEvent::CoreFound { core_id, .. } => {
                    println!("Found core: {}", core_id);
                }
                RoonEvent::CorePaired(core) => {
                    println!(
                        "Paired with: {} ({})",
                        core.display_name(),
                        core.core_id()
                    );
                    run_transport(core).await?;
                    break;
                }
                RoonEvent::CoreLost { core_id } => {
                    println!("Lost core: {}", core_id);
                }
                RoonEvent::CoreUnpaired { core_id } => {
                    println!("Unpaired from: {}", core_id);
                }
            }
        }
    }

    Ok(())
}

async fn run_transport(core: roon_api::Core) -> anyhow::Result<()> {
    let transport = core.transport();
    let mut zone_rx = transport.subscribe_zones().await?;

    println!("\nWaiting for zone events...\n");

    let mut initial_done = false;
    while let Some(event) = zone_rx.recv().await {
        match event {
            ZoneEvent::Initial(zones) => {
                println!("=== {} zone(s) ===", zones.len());
                for zone in &zones {
                    print_zone(zone);
                }
                initial_done = true;

                // Try to play the first zone that allows it
                if let Some(zone) = zones.iter().find(|z| z.is_play_allowed) {
                    println!("\n>> Sending Play to zone: {}", zone.display_name);
                    transport
                        .control(&zone.zone_id, ControlAction::Play)
                        .await?;
                }
            }
            ZoneEvent::Changed(zones) => {
                for zone in &zones {
                    println!("[Changed] {} → {:?}", zone.display_name, zone.state);
                    if let Some(np) = &zone.now_playing {
                        println!("  Now playing: {}", np.one_line.line1);
                    }
                }
                // After seeing the first change, pause and exit
                if initial_done {
                    if let Some(zone) = zones.iter().find(|z| z.state == PlayState::Playing) {
                        println!("\n>> Sending Pause to zone: {}", zone.display_name);
                        transport
                            .control(&zone.zone_id, ControlAction::Pause)
                            .await?;

                        // Wait a moment for the pause to take effect
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        println!("\nDone! SDK is working.");
                        break;
                    }
                }
            }
            ZoneEvent::Seeked(seeks) => {
                for seek in &seeks {
                    if let Some(pos) = seek.seek_position {
                        println!("[Seek] zone {} → {:.1}s", seek.zone_id, pos);
                    }
                }
            }
            ZoneEvent::Added(zones) => {
                for zone in &zones {
                    println!("[Added] {}", zone.display_name);
                }
            }
            ZoneEvent::Removed(ids) => {
                for id in &ids {
                    println!("[Removed] zone {}", id);
                }
            }
        }
    }

    Ok(())
}

fn print_zone(zone: &roon_api::Zone) {
    println!(
        "  {} [{}] state={:?}",
        zone.display_name, zone.zone_id, zone.state
    );
    for output in &zone.outputs {
        print!("    output: {} [{}]", output.display_name, output.output_id);
        if let Some(vol) = &output.volume {
            print!(" vol={}/{}", vol.value, vol.max);
            if vol.is_muted == Some(true) {
                print!(" (muted)");
            }
        }
        println!();
    }
    if let Some(np) = &zone.now_playing {
        println!("    now playing: {}", np.one_line.line1);
        if let Some(len) = np.length {
            let pos = zone.seek_position.unwrap_or(0.0);
            println!("    position: {:.0}s / {:.0}s", pos, len);
        }
    }
    if let Some(settings) = &zone.settings {
        println!(
            "    settings: shuffle={} loop={:?} auto_radio={}",
            settings.shuffle, settings.r#loop, settings.auto_radio
        );
    }
}

fn parse_args(args: &[String]) -> (Option<String>, Option<u16>) {
    let mut host = None;
    let mut port = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" if i + 1 < args.len() => {
                host = Some(args[i + 1].clone());
                i += 2;
            }
            "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().ok();
                i += 2;
            }
            _ => i += 1,
        }
    }
    (host, port)
}
