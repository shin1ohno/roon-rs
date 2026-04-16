//! Memory profiling: connect to Roon Core, subscribe to zones, measure RSS over time.
//!
//! Usage:
//!   cargo run --release -p roon-api --example memory_profile -- --host 192.168.1.20 --port 9330

use roon_api::{FileTokenStore, RoonClientBuilder, ZoneEvent};

fn get_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            return parts.get(1)?.parse().ok();
        }
    }
    None
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let host = args
        .iter()
        .position(|a| a == "--host")
        .map(|i| &args[i + 1]);
    let port: Option<u16> = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args[i + 1].parse().ok());
    let duration_secs: u64 = args
        .iter()
        .position(|a| a == "--duration")
        .and_then(|i| args[i + 1].parse().ok())
        .unwrap_or(60);

    let token_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("roon-rs")
        .join("tokens.json");

    let client = RoonClientBuilder::new(
        "com.roon-rs.memprofile",
        "roon-rs Memory Profile",
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
            println!("Usage: --host <ip> --port <port> [--duration <secs>]");
            return Ok(());
        }
    };

    let transport = core.transport();
    let mut zone_rx = transport.subscribe_zones().await?;

    let baseline_rss = get_rss_kb().unwrap_or(0);
    println!(
        "Baseline RSS: {} KB ({:.1} MB)",
        baseline_rss,
        baseline_rss as f64 / 1024.0
    );
    println!("Monitoring for {}s...\n", duration_secs);
    println!(
        "{:>6}  {:>10}  {:>10}  {:>8}",
        "Time", "RSS (KB)", "RSS (MB)", "Events"
    );

    let start = std::time::Instant::now();
    let mut event_count: u64 = 0;
    let mut sample_interval = tokio::time::interval(std::time::Duration::from_secs(5));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(duration_secs);

    loop {
        tokio::select! {
            Some(event) = zone_rx.recv() => {
                event_count += 1;
                match &event {
                    ZoneEvent::Seeked(_) => {} // high frequency, just count
                    _ => {}
                }
            }

            _ = sample_interval.tick() => {
                let elapsed = start.elapsed().as_secs();
                let rss = get_rss_kb().unwrap_or(0);
                let delta = rss as i64 - baseline_rss as i64;
                println!("{:>5}s  {:>10}  {:>9.1}  {:>8}  (delta: {:+} KB)",
                    elapsed, rss, rss as f64 / 1024.0, event_count, delta);
            }

            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
        }
    }

    let final_rss = get_rss_kb().unwrap_or(0);
    let delta = final_rss as i64 - baseline_rss as i64;
    println!("\n=== Summary ===");
    println!("Duration: {}s", duration_secs);
    println!("Events received: {}", event_count);
    println!(
        "Baseline RSS: {} KB ({:.1} MB)",
        baseline_rss,
        baseline_rss as f64 / 1024.0
    );
    println!(
        "Final RSS:    {} KB ({:.1} MB)",
        final_rss,
        final_rss as f64 / 1024.0
    );
    println!(
        "Delta:        {:+} KB ({:+.1} MB)",
        delta,
        delta as f64 / 1024.0
    );

    if delta > 1024 {
        println!("\nWARNING: RSS grew by more than 1 MB — potential memory leak");
    } else {
        println!("\nMemory usage stable.");
    }

    Ok(())
}
