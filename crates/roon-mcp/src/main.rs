mod tools;

use std::sync::Arc;

use rmcp::ServiceExt;
use tokio::sync::Mutex;

use roon_api::{FileStateStore, RoonClientBuilder, RoonEvent, Zone, ZoneEvent};
use tools::RoonMcpServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let host = args.iter().position(|a| a == "--host").map(|i| args[i + 1].clone());
    let port: Option<u16> = args.iter().position(|a| a == "--port").and_then(|i| args[i + 1].parse().ok());

    let token_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("roon-rs")
        .join("tokens.json");

    let transport_state: Arc<Mutex<Option<roon_api::Transport>>> = Arc::new(Mutex::new(None));
    let browse_state: Arc<Mutex<Option<roon_api::Browse>>> = Arc::new(Mutex::new(None));
    let zones_state: Arc<Mutex<Vec<Zone>>> = Arc::new(Mutex::new(Vec::new()));

    let client = RoonClientBuilder::new(
        "com.roon-rs.mcp",
        "roon-rs MCP Server",
        "0.1.0",
        "roon-rs",
        "dev@example.com",
    )
    .token_store(FileStateStore::new(&token_path))
    .require_transport()
    .require_browse()
    .build()?;

    let mut events = client.events();

    if let (Some(h), Some(p)) = (&host, port) {
        tracing::info!("Connecting to {}:{}", h, p);
        client.connect(h, p).await?;
    } else {
        tracing::info!("Starting SOOD discovery...");
        client.start_discovery().await?;
    }

    let transport_init = transport_state.clone();
    let browse_init = browse_state.clone();
    let zones_init = zones_state.clone();

    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(RoonEvent::CorePaired(core)) => {
                    tracing::info!("Paired with core: {}", core.display_name());
                    let transport = core.transport();
                    let browse = core.browse();

                    *transport_init.lock().await = Some(transport.clone());
                    *browse_init.lock().await = Some(browse);

                    let zones_ref = zones_init.clone();
                    if let Ok(mut zone_rx) = transport.subscribe_zones().await {
                        tokio::spawn(async move {
                            while let Some(event) = zone_rx.recv().await {
                                let mut zones = zones_ref.lock().await;
                                match event {
                                    ZoneEvent::Initial(z) => *zones = z,
                                    ZoneEvent::Changed(changed) => {
                                        for cz in changed {
                                            if let Some(pos) = zones.iter().position(|z| z.zone_id == cz.zone_id) {
                                                zones[pos] = cz;
                                            }
                                        }
                                    }
                                    ZoneEvent::Added(added) => zones.extend(added),
                                    ZoneEvent::Removed(ids) => zones.retain(|z| !ids.contains(&z.zone_id)),
                                    ZoneEvent::Seeked(_) => {}
                                }
                            }
                        });
                    }
                }
                Ok(RoonEvent::CoreLost { core_id }) => {
                    tracing::warn!("Lost core: {}", core_id);
                    *transport_init.lock().await = None;
                    *browse_init.lock().await = None;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let server = RoonMcpServer {
        transport: transport_state,
        browse: browse_state,
        zones: zones_state,
    };

    tracing::info!("Starting MCP server (stdio)");
    let (stdin, stdout) = rmcp::transport::io::stdio();
    let running = server.serve((stdin, stdout)).await
        .map_err(|e| anyhow::anyhow!("MCP server error: {:?}", e))?;
    running.waiting().await
        .map_err(|e| anyhow::anyhow!("MCP server error: {:?}", e))?;

    Ok(())
}
