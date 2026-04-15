use std::sync::Arc;

use roon_moo::connection::MooConnection;

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
    // Used by Transport/Browse services in Phase 5+
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

    /// The MOO connection to this core (for service use).
    #[allow(dead_code)]
    pub(crate) fn connection(&self) -> &Arc<MooConnection> {
        &self.inner.connection
    }
}
