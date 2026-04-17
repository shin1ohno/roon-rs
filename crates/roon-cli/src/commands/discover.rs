use std::io::{self, Write};

use anyhow::{Result, bail};

use crate::config::{self, ServerConfig, ZoneConfig};
use crate::connect;

pub async fn discover(scan_secs: u64, pairing_timeout_secs: u64) -> Result<()> {
    eprintln!("Discovering Roon Cores ({}s scan)...", scan_secs);
    let cores = connect::discover_cores(scan_secs).await?;

    if cores.is_empty() {
        bail!("No Roon Cores found on the network.");
    }

    println!("Found {} Roon Core(s):", cores.len());
    for (i, core) in cores.iter().enumerate() {
        let label = core.name.as_deref().unwrap_or("(unnamed)");
        println!("  {}) {} — {}:{}", i + 1, label, core.host, core.http_port);
    }

    let selected = if cores.len() == 1 {
        println!("Auto-selecting the only core.");
        &cores[0]
    } else {
        let idx = prompt_selection(cores.len())?;
        &cores[idx]
    };

    let mut cfg = config::load().unwrap_or_default();
    cfg.server = Some(ServerConfig {
        host: selected.host.to_string(),
        port: selected.http_port,
        name: selected
            .name
            .clone()
            .unwrap_or_else(|| selected.core_id.clone()),
    });
    config::save(&cfg)?;

    println!(
        "Default server set: {}:{}",
        selected.host, selected.http_port
    );

    // Attempt pairing
    eprintln!("Connecting... Approve 'roon-rs CLI' in Roon Settings > Extensions if prompted.");
    match connect::connect(
        &selected.host.to_string(),
        selected.http_port,
        pairing_timeout_secs,
    )
    .await
    {
        Ok(conn) => println!("Paired with: {}", conn.core.display_name()),
        Err(e) => println!(
            "Pairing pending — authorize in Roon Settings > Extensions. ({})",
            e
        ),
    }

    Ok(())
}

pub async fn disconnect() -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    cfg.server = None;
    cfg.zone = None;
    config::save(&cfg)?;
    println!("Default server cleared.");
    Ok(())
}

pub async fn select_zone(
    host_override: Option<&str>,
    port_override: Option<u16>,
    timeout_secs: u64,
) -> Result<()> {
    let conn = connect::connect_from_config(host_override, port_override, timeout_secs).await?;
    let transport = conn.core.transport();
    let zones: Vec<roon_api::Zone> = transport.get_zones().await?;

    if zones.is_empty() {
        bail!("No zones available.");
    }

    println!("Zones:");
    for (i, zone) in zones.iter().enumerate() {
        let state = format!("{:?}", zone.state);
        let np = zone
            .now_playing
            .as_ref()
            .map(|np| format!(": {}", np.one_line.line1))
            .unwrap_or_default();
        println!("  {}) {} ({}{})", i + 1, zone.display_name, state, np);
    }

    let idx = if zones.len() == 1 {
        println!("Auto-selecting the only zone.");
        0
    } else {
        prompt_selection(zones.len())?
    };

    let selected = &zones[idx];
    let mut cfg = config::load()?;
    cfg.zone = Some(ZoneConfig {
        name: selected.display_name.clone(),
    });
    config::save(&cfg)?;

    println!("Default zone set: {}", selected.display_name);
    Ok(())
}

fn prompt_selection(count: usize) -> Result<usize> {
    print!("Select [1-{}]: ", count);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let n: usize = input
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid number"))?;
    if n < 1 || n > count {
        bail!("Selection out of range.");
    }
    Ok(n - 1)
}
