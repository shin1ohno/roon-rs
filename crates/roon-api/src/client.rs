use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, Mutex};

use crate::core::Core;
use crate::error::ApiError;
use crate::event::RoonEvent;
use crate::registry::{self, ExtensionInfo};
use crate::token::{MemoryStateStore, StateStore};

const BACKOFF_INITIAL: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Builder for constructing a `RoonClient`.
pub struct RoonClientBuilder {
    extension_id: String,
    display_name: String,
    display_version: String,
    publisher: String,
    email: String,
    website: Option<String>,
    token_store: Option<Arc<dyn StateStore>>,
    required_services: Vec<String>,
    optional_services: Vec<String>,
}

impl RoonClientBuilder {
    /// Create a new builder with the required extension metadata.
    pub fn new(
        extension_id: &str,
        display_name: &str,
        display_version: &str,
        publisher: &str,
        email: &str,
    ) -> Self {
        Self {
            extension_id: extension_id.to_string(),
            display_name: display_name.to_string(),
            display_version: display_version.to_string(),
            publisher: publisher.to_string(),
            email: email.to_string(),
            website: None,
            token_store: None,
            required_services: Vec::new(),
            optional_services: Vec::new(),
        }
    }

    /// Set a website URL for the extension.
    pub fn website(mut self, url: &str) -> Self {
        self.website = Some(url.to_string());
        self
    }

    /// Provide a custom state store for persisting tokens and pairing state.
    pub fn token_store(mut self, store: impl StateStore) -> Self {
        self.token_store = Some(Arc::new(store));
        self
    }

    /// Require the Transport service (zone subscription, playback control).
    pub fn require_transport(mut self) -> Self {
        self.required_services
            .push("com.roonlabs.transport:2".to_string());
        self
    }

    /// Require the Browse service (music library browsing).
    pub fn require_browse(mut self) -> Self {
        self.required_services
            .push("com.roonlabs.browse:1".to_string());
        self
    }

    /// Optionally request the Transport service.
    pub fn optional_transport(mut self) -> Self {
        self.optional_services
            .push("com.roonlabs.transport:2".to_string());
        self
    }

    /// Optionally request the Browse service.
    pub fn optional_browse(mut self) -> Self {
        self.optional_services
            .push("com.roonlabs.browse:1".to_string());
        self
    }

    /// Build the `RoonClient`.
    pub fn build(self) -> Result<RoonClient, ApiError> {
        let token_store: Arc<dyn StateStore> = self
            .token_store
            .unwrap_or_else(|| Arc::new(MemoryStateStore::new()));

        let (event_tx, _) = broadcast::channel::<RoonEvent>(32);

        Ok(RoonClient {
            info: ExtensionInfo {
                extension_id: self.extension_id,
                display_name: self.display_name,
                display_version: self.display_version,
                publisher: self.publisher,
                email: self.email,
                website: self.website,
                required_services: self.required_services,
                optional_services: self.optional_services,
            },
            token_store,
            event_tx,
        })
    }
}

/// The main Roon SDK client.
///
/// Use `RoonClientBuilder` to create an instance, then call
/// `start_discovery()` or `connect()` to begin interacting with Roon Core.
pub struct RoonClient {
    info: ExtensionInfo,
    token_store: Arc<dyn StateStore>,
    event_tx: broadcast::Sender<RoonEvent>,
}

impl RoonClient {
    /// Subscribe to SDK lifecycle events.
    ///
    /// Returns a receiver that yields `RoonEvent`s such as `CoreFound`,
    /// `CorePaired`, `CoreUnpaired`, and `CoreLost`.
    pub fn events(&self) -> broadcast::Receiver<RoonEvent> {
        self.event_tx.subscribe()
    }

    /// Start SOOD-based discovery of Roon Cores on the local network.
    ///
    /// Discovered cores are automatically connected to and registered with.
    /// If a connection is lost, the core is removed from the connected set
    /// and will be reconnected on the next discovery response.
    /// Events are emitted via the `events()` channel.
    pub async fn start_discovery(&self) -> Result<(), ApiError> {
        let (discovery, mut core_rx) = roon_sood::SoodDiscovery::start().await?;

        let info = self.info.clone();
        let event_tx = self.event_tx.clone();
        let token_store = self.token_store.clone();

        tokio::spawn(async move {
            let connected_cores: Arc<Mutex<HashSet<String>>> =
                Arc::new(Mutex::new(HashSet::new()));

            while let Ok(discovered) = core_rx.recv().await {
                {
                    let cores = connected_cores.lock().await;
                    if cores.contains(&discovered.core_id) {
                        continue;
                    }
                }

                let url = format!("ws://{}:{}/api", discovered.host, discovered.http_port);

                let _ = event_tx.send(RoonEvent::CoreFound {
                    core_id: discovered.core_id.clone(),
                    display_name: String::new(),
                });

                match registry::perform_handshake(&url, &info, &*token_store).await {
                    Ok(core) => {
                        let core_id = discovered.core_id.clone();
                        connected_cores.lock().await.insert(core_id.clone());

                        // Monitor connection health — remove from set on disconnect
                        let cores_ref = connected_cores.clone();
                        let event_tx_ref = event_tx.clone();
                        let core_ref = core.clone();
                        tokio::spawn(async move {
                            monitor_connection(&core_ref).await;
                            cores_ref.lock().await.remove(&core_id);
                            let _ = event_tx_ref.send(RoonEvent::CoreLost {
                                core_id: core_id.clone(),
                            });
                            tracing::info!(
                                "Core {} disconnected, will reconnect on next discovery",
                                core_id
                            );
                        });

                        let _ = event_tx.send(RoonEvent::CorePaired(core));
                    }
                    Err(e) => {
                        tracing::warn!("Failed to register with core at {}: {}", url, e);
                    }
                }
            }
            discovery.stop().await;
        });

        Ok(())
    }

    /// Connect directly to a known Roon Core with automatic reconnection.
    ///
    /// On connection loss, retries with exponential backoff (1s → 60s max).
    /// Emits `CoreLost` on disconnect and `CorePaired` on reconnect.
    pub async fn connect(&self, host: &str, port: u16) -> Result<Core, ApiError> {
        let url = format!("ws://{}:{}/api", host, port);
        let core = registry::perform_handshake(&url, &self.info, &*self.token_store).await?;
        let _ = self.event_tx.send(RoonEvent::CorePaired(core.clone()));

        // Spawn reconnection loop
        let url = url.clone();
        let info = self.info.clone();
        let token_store = self.token_store.clone();
        let event_tx = self.event_tx.clone();
        let initial_core = core.clone();

        tokio::spawn(async move {
            monitor_connection(&initial_core).await;

            let core_id = initial_core.core_id().to_string();
            let _ = event_tx.send(RoonEvent::CoreLost {
                core_id: core_id.clone(),
            });

            let mut backoff = BACKOFF_INITIAL;
            loop {
                tracing::info!(
                    "Reconnecting to {} in {:?}...",
                    url,
                    backoff
                );
                tokio::time::sleep(backoff).await;

                match registry::perform_handshake(&url, &info, &*token_store).await {
                    Ok(new_core) => {
                        tracing::info!("Reconnected to core {}", new_core.core_id());
                        let _ = event_tx.send(RoonEvent::CorePaired(new_core.clone()));

                        // Monitor again
                        monitor_connection(&new_core).await;
                        let _ = event_tx.send(RoonEvent::CoreLost {
                            core_id: new_core.core_id().to_string(),
                        });

                        // Reset backoff on successful reconnection
                        backoff = BACKOFF_INITIAL;
                    }
                    Err(e) => {
                        tracing::warn!("Reconnection failed: {}", e);
                        backoff = (backoff * 2).min(BACKOFF_MAX);
                    }
                }
            }
        });

        Ok(core)
    }
}

/// Wait until the connection to a core is no longer alive.
async fn monitor_connection(core: &Core) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if !core.is_alive() {
            break;
        }
    }
}
