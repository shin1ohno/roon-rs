//! Play-queue types for `com.roonlabs.transport:2`.
//!
//! A queue subscription yields one [`QueueEvent::Subscribed`] carrying the
//! current contents, then [`QueueEvent::Changed`] for every subsequent edit.
//! The core only sends the delta, so a client is expected to keep its own copy
//! of the queue and apply changes to it in order.

use serde::{Deserialize, Serialize};

use crate::zone::{LineInfo, ThreeLineInfo, TwoLineInfo};

/// One entry in a zone's play queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    /// Stable within a subscription; pass to
    /// [`crate::Transport::play_from_here`] to jump to this item.
    pub queue_item_id: u64,
    /// Track length in seconds.
    #[serde(default)]
    pub length: Option<f64>,
    #[serde(default)]
    pub image_key: Option<String>,
    pub one_line: LineInfo,
    #[serde(default)]
    pub two_line: Option<TwoLineInfo>,
    #[serde(default)]
    pub three_line: Option<ThreeLineInfo>,
}

/// How a [`QueueChange`] modifies the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueOperation {
    Insert,
    Remove,
    /// An operation this crate does not model yet.
    #[serde(other)]
    Unknown,
}

/// An incremental change to a subscribed queue. Apply changes in the order
/// they appear in [`QueueEvent::Changed`].
#[derive(Debug, Clone, Deserialize)]
pub struct QueueChange {
    pub operation: QueueOperation,
    /// Position the operation applies at.
    pub index: usize,
    /// Number of items removed. Present for [`QueueOperation::Remove`].
    #[serde(default)]
    pub count: Option<usize>,
    /// Items to insert at `index`. Present for [`QueueOperation::Insert`].
    #[serde(default)]
    pub items: Option<Vec<QueueItem>>,
}

/// Events yielded by [`crate::Transport::subscribe_queue`].
#[derive(Debug, Clone)]
pub enum QueueEvent {
    /// Initial queue contents, capped at the subscription's `max_item_count`.
    Subscribed(Vec<QueueItem>),
    /// Incremental changes to apply in order.
    Changed(Vec<QueueChange>),
}

pub(crate) fn parse_queue_event(name: &str, body: &serde_json::Value) -> Option<QueueEvent> {
    match name {
        "Subscribed" => serde_json::from_value(body.get("items")?.clone())
            .ok()
            .map(QueueEvent::Subscribed),
        "Changed" => serde_json::from_value(body.get("changes")?.clone())
            .ok()
            .map(QueueEvent::Changed),
        _ => {
            tracing::debug!("unhandled queue event: {name}");
            None
        }
    }
}
