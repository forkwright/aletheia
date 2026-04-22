//! Uniform provenance metadata for fleet artefacts.
//!
//! # Design
//!
//! Every persistable artefact in the fleet carries a [`ArtefactMeta`] stamp
//! that records *who produced it*, *what schema version it targets*, *when it
//! was generated*, and *how many rows/items of each kind it contains*.
//!
//! The [`Stamped`] trait is the single call-site contract: call `.stamp()`
//! on an artefact just before writing it to disk or a store; serialize the
//! returned [`ArtefactMeta`] alongside the artefact.
//!
//! # Adding a new artefact type
//!
//! 1. `impl Stamped for MyType` in the crate that owns `MyType`.
//! 2. In the persist path, call `let meta = artefact.stamp();` and write it
//!    beside the payload (sibling JSON file, sidecar column, or envelope field).
//! 3. Increment `schema_version` only when the on-disk schema changes in a
//!    backwards-incompatible way.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ── Stamped trait ─────────────────────────────────────────────────────────────

/// Every persistable artefact carries provenance metadata.
///
/// Implement this trait on your artefact struct and call `.stamp()` in the
/// persist path. The stamp is computed at the moment of persistence so that
/// `generated_at` reflects write time, not construction time.
pub trait Stamped {
    /// Returns the artefact's stamp at the moment of persistence.
    fn stamp(&self) -> ArtefactMeta;
}

// ── ArtefactMeta ──────────────────────────────────────────────────────────────

/// Uniform provenance envelope attached to every persistable fleet artefact.
///
/// # Stability
///
/// `#[non_exhaustive]` ensures downstream `match` on field subsets or struct
/// literals do not break when new fields are appended. Per kanon #108 lesson:
/// a simple field addition to a widely-used struct broke 90 sites without this
/// guard.
///
/// # Field conventions
///
/// - `producer`: `"<crate_name>@<version>"` where version is the Cargo package
///   version at compile time (use `env!("CARGO_PKG_VERSION")`).
/// - `schema_version`: monotonic per artefact type; increment only on
///   backwards-incompatible on-disk schema changes.
/// - `generated_at`: RFC 3339 / ISO 8601 timestamp string.
/// - `row_counts`: named counts for the artefact's primary collections
///   (e.g. `"messages"`, `"edges"`, `"scenarios"`). Use canonical names
///   per artefact type so tooling can aggregate across producers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ArtefactMeta {
    /// Crate + version that produced this artefact, e.g. `"graphe@0.21.1"`.
    pub producer: String,
    /// Monotonic schema version for this artefact type.
    pub schema_version: u32,
    /// RFC 3339 timestamp of when the artefact was stamped.
    pub generated_at: String,
    /// Named row/item counts for the primary collections in this artefact.
    pub row_counts: BTreeMap<String, u64>,
}

impl ArtefactMeta {
    /// Construct a stamp with required fields; `row_counts` starts empty.
    ///
    /// Chain [`with_count`](Self::with_count) calls to populate row counts.
    #[must_use]
    pub fn new(
        producer: impl Into<String>,
        schema_version: u32,
        generated_at: impl Into<String>,
    ) -> Self {
        Self {
            producer: producer.into(),
            schema_version,
            generated_at: generated_at.into(),
            row_counts: BTreeMap::new(),
        }
    }

    /// Builder-style append for a single named row count.
    ///
    /// Calling this multiple times with the same `name` overwrites the
    /// previous value; last write wins.
    #[must_use]
    pub fn with_count(mut self, name: impl Into<String>, count: u64) -> Self {
        self.row_counts.insert(name.into(), count);
        self
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn artefact_meta_new_round_trip_serde() {
        let meta = ArtefactMeta::new("mneme@0.21.1", 1, "2026-04-22T00:00:00Z");
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: ArtefactMeta = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(meta, back, "round-trip must be identical");
    }

    #[test]
    fn artefact_meta_builder_appends_counts() {
        let meta = ArtefactMeta::new("graphe@0.21.1", 2, "2026-04-22T00:00:00Z")
            .with_count("messages", 42)
            .with_count("sessions", 3);
        assert_eq!(
            meta.row_counts.get("messages").copied(),
            Some(42),
            "messages count should be 42"
        );
        assert_eq!(
            meta.row_counts.get("sessions").copied(),
            Some(3),
            "sessions count should be 3"
        );
        assert_eq!(meta.row_counts.len(), 2);
    }

    #[test]
    fn artefact_meta_producer_convention_matches_pattern() {
        // Convention: "<crate_name>@<version>"
        let producer = "mneme@0.21.1";
        let meta = ArtefactMeta::new(producer, 1, "2026-04-22T00:00:00Z");
        assert!(
            meta.producer.contains('@'),
            "producer must contain '@' separator"
        );
        let (crate_name, version) = meta.producer.split_once('@').expect("split at '@'");
        assert!(!crate_name.is_empty(), "crate name must not be empty");
        assert!(!version.is_empty(), "version must not be empty");
    }

    #[test]
    fn artefact_meta_with_count_overwrites_on_duplicate() {
        let meta = ArtefactMeta::new("test@1.0.0", 1, "2026-04-22T00:00:00Z")
            .with_count("items", 10)
            .with_count("items", 99);
        assert_eq!(
            meta.row_counts.get("items").copied(),
            Some(99),
            "last write wins"
        );
    }

    #[test]
    fn artefact_meta_empty_row_counts_serializes() {
        let meta = ArtefactMeta::new("eval@0.21.1", 1, "2026-04-22T00:00:00Z");
        let json = serde_json::to_string(&meta).expect("serialize");
        assert!(
            json.contains("row_counts"),
            "row_counts field must be present"
        );
        let back: ArtefactMeta = serde_json::from_str(&json).expect("deserialize");
        assert!(back.row_counts.is_empty(), "row_counts should be empty");
    }
}
