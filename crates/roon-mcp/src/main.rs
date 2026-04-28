mod tools;

use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio::sync::Mutex;

use roon_api::{FileStateStore, RoonClientBuilder, RoonEvent, Zone, ZoneEvent};
use roon_mcp::auth::{self, AuthConfig, JwksCache};
use tools::RoonMcpServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let host = args
        .iter()
        .position(|a| a == "--host")
        .map(|i| args[i + 1].clone());
    let port: Option<u16> = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args[i + 1].parse().ok());
    let transport_mode = args
        .iter()
        .position(|a| a == "--transport")
        .map(|i| args[i + 1].clone())
        .unwrap_or_else(|| "stdio".to_string());
    let http_port: u16 = args
        .iter()
        .position(|a| a == "--http-port")
        .and_then(|i| args[i + 1].parse().ok())
        .unwrap_or(8080);
    let extra_allowed_hosts: Vec<String> = args
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            if a == "--allowed-host" {
                args.get(i + 1).cloned()
            } else {
                None
            }
        })
        .collect();

    let auth_issuer = args
        .iter()
        .position(|a| a == "--issuer")
        .and_then(|i| args.get(i + 1).cloned());
    let auth_audience = args
        .iter()
        .position(|a| a == "--audience")
        .and_then(|i| args.get(i + 1).cloned());
    let auth_jwks_url = args
        .iter()
        .position(|a| a == "--jwks-url")
        .and_then(|i| args.get(i + 1).cloned());
    let require_auth = args.iter().any(|a| a == "--require-auth");

    let token_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("roon-rs")
        .join("tokens.json");

    // Shared Roon state
    let transport_state: Arc<Mutex<Option<roon_api::Transport>>> = Arc::new(Mutex::new(None));
    let browse_state: Arc<Mutex<Option<roon_api::Browse>>> = Arc::new(Mutex::new(None));
    let zones_state: Arc<Mutex<Vec<Zone>>> = Arc::new(Mutex::new(Vec::new()));

    // Connect to Roon Core
    let client = RoonClientBuilder::new(
        "com.roon-rs.mcp",
        "roon-rs MCP Server",
        "0.5.2",
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

    // Roon event handler task
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
                                            if let Some(pos) =
                                                zones.iter().position(|z| z.zone_id == cz.zone_id)
                                            {
                                                zones[pos] = cz;
                                            }
                                        }
                                    }
                                    ZoneEvent::Added(added) => zones.extend(added),
                                    ZoneEvent::Removed(ids) => {
                                        zones.retain(|z| !ids.contains(&z.zone_id))
                                    }
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

    match transport_mode.as_str() {
        "stdio" => {
            tracing::info!("Starting MCP server (stdio)");
            let server = RoonMcpServer {
                transport: transport_state,
                browse: browse_state,
                zones: zones_state,
            };
            let (stdin, stdout) = rmcp::transport::io::stdio();
            let running = server
                .serve((stdin, stdout))
                .await
                .map_err(|e| anyhow::anyhow!("MCP server error: {:?}", e))?;
            running
                .waiting()
                .await
                .map_err(|e| anyhow::anyhow!("MCP server error: {:?}", e))?;
        }
        "sse" | "http" => {
            tracing::info!("Starting MCP server (SSE on port {})", http_port);

            let bind_addr = format!("0.0.0.0:{}", http_port);
            let mut allowed_hosts = vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
                format!("0.0.0.0:{}", http_port),
            ];
            allowed_hosts.extend(extra_allowed_hosts.iter().cloned());
            let config = StreamableHttpServerConfig::default().with_allowed_hosts(allowed_hosts);

            let transport_s = transport_state.clone();
            let browse_s = browse_state.clone();
            let zones_s = zones_state.clone();

            let session_manager = Arc::new(LocalSessionManager::default());
            let service = StreamableHttpService::new(
                move || {
                    Ok(RoonMcpServer {
                        transport: transport_s.clone(),
                        browse: browse_s.clone(),
                        zones: zones_s.clone(),
                    })
                },
                session_manager,
                config,
            );

            let auth_cfg = match (
                auth_issuer.as_deref(),
                auth_audience.as_deref(),
                auth_jwks_url.as_deref(),
            ) {
                (Some(iss), Some(aud), Some(url)) => Some(Arc::new(AuthConfig {
                    issuer: iss.to_string(),
                    audience: aud.to_string(),
                    jwks_url: url.to_string(),
                    require_auth,
                })),
                _ => {
                    if require_auth {
                        anyhow::bail!("--require-auth needs --issuer, --audience, and --jwks-url");
                    }
                    None
                }
            };
            let jwks_cache = auth_cfg
                .as_ref()
                .map(|c| Arc::new(JwksCache::new(c.jwks_url.clone())));

            if let Some(cache) = &jwks_cache {
                let cache_warm = cache.clone();
                tokio::spawn(async move { auth::warm_cache(&cache_warm).await });
            }

            let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
            tracing::info!("MCP SSE server listening on {}", bind_addr);

            let app = hyper::service::service_fn(move |req| {
                let mut svc = service.clone();
                let cfg = auth_cfg.clone();
                let cache = jwks_cache.clone();
                async move {
                    let path = req.uri().path().to_string();
                    let method = req.method().clone();

                    // OAuth-protected-resource metadata is served unconditionally.
                    if path == "/.well-known/oauth-protected-resource"
                        && let Some(cfg) = cfg.as_ref()
                    {
                        return Ok::<_, std::convert::Infallible>(auth::metadata_response(cfg));
                    }

                    let bypass = method == http::Method::OPTIONS
                        || path.starts_with("/.well-known/")
                        || path == "/health";

                    if !bypass
                        && let (Some(cfg), Some(cache)) = (cfg.as_ref(), cache.as_ref())
                        && cfg.require_auth
                    {
                        let header_value = req
                            .headers()
                            .get(http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok());
                        match auth::verify_bearer(header_value, cache, cfg).await {
                            Ok(_claims) => {}
                            Err(auth::AuthError::Missing) => {
                                return Ok(auth::unauthorized_response(cfg, false));
                            }
                            Err(_other) => {
                                return Ok(auth::unauthorized_response(cfg, true));
                            }
                        }
                    }

                    tower_service::Service::call(&mut svc, req).await
                }
            });

            loop {
                let (stream, _) = listener.accept().await?;
                let app = app.clone();
                tokio::spawn(async move {
                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), app)
                    .await
                    {
                        tracing::warn!("HTTP connection error: {}", e);
                    }
                });
            }
        }
        _ => {
            anyhow::bail!(
                "Unknown transport: {}. Use 'stdio' or 'sse'",
                transport_mode
            );
        }
    }

    Ok(())
}
