use std::sync::Arc;

use roon_moo::connection::MooConnection;

use crate::browse::Browse;
use crate::transport::Transport;

/// A handle to a connected and registered Roon Core.
///
/// Provides access to services (transport, browse) available on the core.
/// This type is cheaply cloneable.
#[derive(Debug, Clone)]
pub struct Core {
    pub(crate) inner: Arc<CoreInner>,
}

#[derive(Debug)]
pub(crate) struct CoreInner {
    pub(crate) core_id: String,
    pub(crate) display_name: String,
    pub(crate) display_version: String,
    #[allow(dead_code)]
    pub(crate) token: Option<String>,
    #[allow(dead_code)]
    pub(crate) http_port: u16,
    pub(crate) connection: Arc<MooConnection>,
}

impl Core {
    /// The unique identifier for this Roon Core.
    pub fn core_id(&self) -> &str {
        &self.inner.core_id
    }

    /// The display name of this Roon Core (e.g., "Living Room Core").
    pub fn display_name(&self) -> &str {
        &self.inner.display_name
    }

    /// The version string of this Roon Core.
    pub fn display_version(&self) -> &str {
        &self.inner.display_version
    }

    /// Get the Transport service for zone subscription and playback control.
    pub fn transport(&self) -> Transport {
        Transport::new(self.inner.connection.clone())
    }

    /// Get the Browse service for navigating the music library.
    pub fn browse(&self) -> Browse {
        Browse::new(self.inner.connection.clone())
    }

    /// Check if the connection to this core is still alive.
    pub fn is_alive(&self) -> bool {
        self.inner.connection.is_alive()
    }
}
