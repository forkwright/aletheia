//! Memory-policy action placeholders for the Phase 06b training scaffold.

use serde::{Deserialize, Serialize};

/// A candidate memory-policy action.
///
/// The variants mirror the planned policy vocabulary without encoding the
/// still-missing Phase 05c parameter inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Action {
    /// Keep a memory item regardless of ordinary decay pressure.
    Pin {
        /// Stable memory identifier chosen by the caller.
        memory_id: String,
    },
    /// Remove a memory item from active recall.
    Evict {
        /// Stable memory identifier chosen by the caller.
        memory_id: String,
    },
    /// Merge two memory items into a single survivor.
    Merge {
        /// Identifier of the item being merged away.
        source_id: String,
        /// Identifier of the item that remains after the merge.
        target_id: String,
    },
    /// Compact a scoped set of memory items into a denser representation.
    Compact {
        /// Caller-defined scope such as a session, project, or topic key.
        scope: String,
    },
    /// Lower recall priority without removing the memory item.
    Demote {
        /// Stable memory identifier chosen by the caller.
        memory_id: String,
    },
    /// Leave the memory item unchanged for this step.
    Retain {
        /// Stable memory identifier chosen by the caller.
        memory_id: String,
    },
}
