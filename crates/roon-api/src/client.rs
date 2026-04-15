use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, Mutex};

use crate::core::Core;
use crate::error::ApiError;
use crate::event::RoonEvent;
use crate::pairing::PairingState;
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
    provided_services: Vec<String>,
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
            provided_services: Vec::new(),
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

    /// Register an additional provided service name.
    pub fn provide_service(mut self, service_name: &str) -> Self {
        self.provided_services.push(service_name.to_string());
        self
    }

    /// Build the `RoonClient`.
    pub fn build(self) -> Result<RoonClient, ApiError> {
        let store: Arc<dyn StateStore> = self
            .token_store
            .unwrap_or_else(|| Arc::new(MemoryStateStore::new()));

        let pairing = PairingState::new(&*store);
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
                provided_services: self.provided_services,
            },
            store,
            pairing,
            event_tx,
        })
    }
}

/// The main Roon SDK client.
pub struct RoonClient {
    info: ExtensionInfo,
    store: Arc<dyn StateStore>,
    pairing: PairingState,
    event_tx: broadcast::Sender<RoonEvent>,
}

impl RoonClient {
    /// Subscribe to SDK lifecycle events.
    pub fn events(&self) -> broadcast::Receiver<RoonEvent> {
        self.event_tx.subscribe()
    }

    /// Access the pairing state (e.g., to check paired_core_id).
    pub fn pairing(&self) -> &PairingState {
        &self.pairing
    }

    /// Start SOOD-based discovery of Roon Cores on the local network.
    ///
    /// If a connection is lost, the core is removed from the connected set
    /// and will be reconnected on the next discovery response.
    pub async fn start_discovery(&self) -> Result<(), ApiError> {
        let (discovery, mut core_rx) = roon_sood::SoodDiscovery::start().await?;

        let info = self.info.clone();
        let event_tx = self.event_tx.clone();
        let store = self.store.clone();
        let pairing = self.pairing.clone();

        // Set up pairing change callback to emit events
        let event_tx_pair = event_tx.clone();
        pairing
            .on_pair_change(move |old, _new| {
                if let Some(old_id) = old {
                    let _ = event_tx_pair.send(RoonEvent::CoreUnpaired { core_id: old_id });
                }
            })
            .await;

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

                match registry::perform_handshake(&url, &info, &store, &pairing).await {
                    Ok(core) => {
                        let core_id = discovered.core_id.clone();
                        connected_cores.lock().await.insert(core_id.clone());

                        // Auto-pair with first core if not already paired
                        if pairing.paired_core_id().await.is_none() {
                            pairing.pair_with(&core_id, &*store).await;
                        }

                        let cores_ref = connected_cores.clone();
                        let event_tx_ref = event_tx.clone();
                        let core_ref = core.clone();
                        tokio::spawn(async move {
                            monitor_connection(&core_ref).await;
                            cores_ref.lock().await.remove(&core_id);
                            let _ = event_tx_ref.send(RoonEvent::CoreLost {
                                core_id: core_id.clone(),
                            });
                            tracing::info!("Core {} disconnected", core_id);
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
    pub async fn connect(&self, host: &str, port: u16) -> Result<Core, ApiError> {
        let url = format!("ws://{}:{}/api", host, port);
        let core =
            registry::perform_handshake(&url, &self.info, &self.store, &self.pairing).await?;

        // Auto-pair
        if self.pairing.paired_core_id().await.is_none() {
            self.pairing.pair_with(core.core_id(), &*self.store).await;
        }

        let _ = self.event_tx.send(RoonEvent::CorePaired(core.clone()));

        // Reconnection loop
        let url = url.clone();
        let info = self.info.clone();
        let store = self.store.clone();
        let pairing = self.pairing.clone();
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
                tracing::info!("Reconnecting to {} in {:?}...", url, backoff);
                tokio::time::sleep(backoff).await;

                match registry::perform_handshake(&url, &info, &store, &pairing).await {
                    Ok(new_core) => {
                        tracing::info!("Reconnected to core {}", new_core.core_id());
                        let _ = event_tx.send(RoonEvent::CorePaired(new_core.clone()));
                        monitor_connection(&new_core).await;
                        let _ = event_tx.send(RoonEvent::CoreLost {
                            core_id: new_core.core_id().to_string(),
                        });
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

    /// Connect using a one-time token (skips discovery and info request).
    pub async fn connect_with_token(
        &self,
        host: &str,
        port: u16,
        token: &str,
    ) -> Result<Core, ApiError> {
        let url = format!("ws://{}:{}/api", host, port);
        let core = registry::perform_handshake_with_token(
            &url,
            &self.info,
            token,
            &self.store,
            &self.pairing,
        )
        .await?;
        let _ = self.event_tx.send(RoonEvent::CorePaired(core.clone()));
        Ok(core)
    }
}

async fn monitor_connection(core: &Core) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if !core.is_alive() {
            break;
        }
    }
}
