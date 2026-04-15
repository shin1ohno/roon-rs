//! Test volume and mute controls against a real Roon Core.
//!
//! Usage:
//!   cargo run -p roon-api --example volume_test -- --host 192.168.1.20 --port 9330

use roon_api::{
    FileTokenStore, MuteAction, RoonClientBuilder, VolumeMode, ZoneEvent,
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
        "com.roon-rs.volume_test",
        "roon-rs Volume Test",
        "0.1.0",
        "roon-rs",
        "dev@example.com",
    )
    .token_store(FileTokenStore::new(&token_path))
    .require_transport()
    .build()?;

    let core = match (host, port) {
        (Some(h), Some(p)) => client.connect(h, p).await?,
        _ => {
            println!("Usage: --host <ip> --port <port>");
            return Ok(());
        }
    };

    let transport = core.transport();
    let mut zone_rx = transport.subscribe_zones().await?;

    // Wait for initial zones
    let zones = match zone_rx.recv().await {
        Some(ZoneEvent::Initial(zones)) => zones,
        other => {
            println!("Unexpected first event: {:?}", other);
            return Ok(());
        }
    };

    println!("=== Zones with volume info ===\n");
    for zone in &zones {
        println!("Zone: {} [{}]", zone.display_name, zone.zone_id);
        for output in &zone.outputs {
            print!("  Output: {} [{}]", output.display_name, output.output_id);
            if let Some(vol) = &output.volume {
                println!(
                    " — type={} value={} range=[{}, {}] step={} muted={:?}",
                    vol.volume_type, vol.value, vol.min, vol.max, vol.step, vol.is_muted
                );
            } else {
                println!(" — no volume control");
            }
        }
    }

    // Find first output with volume control
    let target = zones
        .iter()
        .flat_map(|z| z.outputs.iter())
        .find(|o| o.volume.is_some());

    let target = match target {
        Some(o) => o,
        None => {
            println!("\nNo outputs with volume control found.");
            return Ok(());
        }
    };

    let output_id = &target.output_id;
    let vol = target.volume.as_ref().unwrap();
    let original_vol = vol.value;

    println!("\n--- Testing output: {} ---", target.display_name);
    println!("Original volume: {}", original_vol);

    // Test 1: Relative volume change (+1 step)
    println!("\n[Test 1] Volume +1 step (relative_step)...");
    transport
        .change_volume(output_id, VolumeMode::RelativeStep, 1.0)
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("  Sent successfully");

    // Test 2: Volume -1 step (back to original)
    println!("[Test 2] Volume -1 step (relative_step)...");
    transport
        .change_volume(output_id, VolumeMode::RelativeStep, -1.0)
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("  Sent successfully");

    // Test 3: Mute
    println!("[Test 3] Mute...");
    transport.mute(output_id, MuteAction::Mute).await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("  Sent successfully");

    // Test 4: Unmute
    println!("[Test 4] Unmute...");
    transport.mute(output_id, MuteAction::Unmute).await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("  Sent successfully");

    // Test 5: Set absolute volume back to original
    println!("[Test 5] Set absolute volume back to {}...", original_vol);
    transport
        .change_volume(output_id, VolumeMode::Absolute, original_vol)
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("  Sent successfully");

    // Drain zone events to see if volume changes are reflected
    println!("\n--- Zone change events ---");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match tokio::time::timeout_at(deadline, zone_rx.recv()).await {
            Ok(Some(ZoneEvent::Changed(zones))) => {
                for zone in &zones {
                    for output in &zone.outputs {
                        if output.output_id == *output_id {
                            if let Some(vol) = &output.volume {
                                println!(
                                    "  {} volume={} muted={:?}",
                                    output.display_name, vol.value, vol.is_muted
                                );
                            }
                        }
                    }
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }

    println!("\nDone! Volume controls working.");
    Ok(())
}
