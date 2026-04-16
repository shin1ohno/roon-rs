use std::time::Duration;

use anyhow::{Context, Result};
use roon_api::{Core, FileStateStore, RoonClientBuilder};

use crate::config;

pub struct RoonConnection {
    pub core: Core,
}

pub async fn connect(host: &str, port: u16, timeout_secs: u64) -> Result<RoonConnection> {
    let client = RoonClientBuilder::new(
        "com.roon-rs.cli",
        "roon-rs CLI",
        "0.1.0",
        "roon-rs",
        "dev@example.com",
    )
    .token_store(FileStateStore::new(config::token_path()))
    .require_transport()
    .require_browse()
    .build()?;

    let core = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        client.connect(host, port),
    )
    .await
    .context("connection timed out")?
    .context("failed to connect to Roon Core")?;

    Ok(RoonConnection { core })
}

pub async fn connect_from_config(
    host_override: Option<&str>,
    port_override: Option<u16>,
    timeout_secs: u64,
) -> Result<RoonConnection> {
    let cfg = config::load()?;

    let (host, port) = match (host_override, port_override) {
        (Some(h), Some(p)) => (h.to_string(), p),
        (Some(h), None) => {
            let p = cfg.server.as_ref().map(|s| s.port).unwrap_or(9330);
            (h.to_string(), p)
        }
        (None, Some(p)) => {
            let h = cfg
                .server
                .as_ref()
                .map(|s| s.host.clone())
                .context("No default server. Run `roon discover` first.")?;
            (h, p)
        }
        (None, None) => {
            let srv = cfg
                .server
                .as_ref()
                .context("No default server. Run `roon discover` first.")?;
            (srv.host.clone(), srv.port)
        }
    };

    connect(&host, port, timeout_secs).await
}

pub async fn discover_cores(timeout_secs: u64) -> Result<Vec<roon_sood::DiscoveredCore>> {
    let (discovery, mut core_rx) = roon_sood::SoodDiscovery::start().await?;

    let mut cores = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, core_rx.recv()).await {
            Ok(Ok(core)) => {
                if !cores.iter().any(|c: &roon_sood::DiscoveredCore| c.core_id == core.core_id) {
                    cores.push(core);
                }
            }
            Ok(Err(_)) => break,
            Err(_) => break, // timeout
        }
    }

    discovery.stop().await;
    Ok(cores)
}
