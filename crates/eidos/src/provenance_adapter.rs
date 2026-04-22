//! Cross-layer provenance translation between eidos and the mnemosyne JSON shape.
//!
//! # Purpose
//!
//! kanon's mnemosyne layer and aletheia's eidos layer share overlapping
//! provenance concepts but live in separate crates with different type shapes.
//! This module provides a canonical translation surface so aletheia consumers
//! can read/write the mnemosyne JSON wire format without importing mnemosyne.
//!
//! # Translation model
//!
//! - [`from_mnemosyne_annotation`]: parse a mnemosyne `Annotation` JSON blob
//!   (captured as [`MnemosyneAnnotationView`]) into an [`ArtefactMeta`].
//! - [`to_mnemosyne_compatible`]: project an [`ArtefactMeta`] into a
//!   [`MnemosyneView`] — a lightweight eidos-owned struct that mirrors the
//!   mnemosyne `Annotation` JSON shape for downstream interop.
//!
//! # Round-trip fidelity
//!
//! The following fields survive a full `ArtefactMeta → MnemosyneView →
//! (re-parse as MnemosyneAnnotationView) → ArtefactMeta` round-trip:
//!
//! | Field | Direction | Notes |
//! |---|---|---|
//! | `actor_id` ↔ `agent_id` | both | renamed on each side |
//! | `session_id` ↔ `session_id` | both | identical semantics |
//! | `generated_at` ↔ `created_at` | both | renamed on each side |
//! | `supersedes` ↔ `supersedes` | both | identical |
//! | `supersede_reason` ↔ `supersede_reason` | both | identical |
//! | `evidence_refs` ↔ `evidence_chunks` | both | strings vs typed ChunkIds |
//! | `producer` → `agent_id` (fallback) | meta→view only | when `actor_id` is None |
//! | `source_kind` ↔ `source_kind` | both | aletheia extension; no annotation analog |
//! | `source_locator` ↔ `source_locator` | both | aletheia extension; no annotation analog |
//! | `confidence` | meta→view only | mnemosyne annotations carry no confidence scalar |
//! | `schema_version` | meta→view only | no mnemosyne analog; see kanon issue filed from #3796 |
//! | `row_counts` | meta only | artefact-specific; no annotation analog |
//! | `claim` ↔ _(none)_ | view→meta | stored in `MnemosyneAnnotationView` but not in `ArtefactMeta` |
//! | `topic` ↔ _(none)_ | view→meta | mnemosyne-specific; dropped on aletheia side |
//!
//! Fields not present on the target side are silently dropped — no information
//! is fabricated. Callers that need lossless bidirectional transfer should
//! store both the `ArtefactMeta` and the original `MnemosyneAnnotationView`.

use serde::{Deserialize, Serialize};

use crate::meta::ArtefactMeta;

// ── MnemosyneAnnotationView ────────────────────────────────────────────────

/// Lightweight mirror of the mnemosyne `Annotation` JSON wire format.
///
/// This struct allows eidos to parse incoming mnemosyne annotation payloads
/// without depending on the `mnemosyne` crate. Field names and serde
/// attributes mirror the mnemosyne wire format exactly.
///
/// All fields except `agent_id` and `created_at` are `Option` to tolerate
/// partial annotations from older kanon versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MnemosyneAnnotationView {
    /// Identity of the agent that wrote the annotation (e.g. `claude-opus-4-7`).
    pub agent_id: String,
    /// ISO 8601 UTC timestamp of creation.
    pub created_at: String,
    /// Free-form claim text (20-2000 chars enforced by mnemosyne at write time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<String>,
    /// Optional session identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Evidence chunk IDs (opaque strings on the eidos side).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_chunks: Vec<String>,
    /// Topic tag (controlled vocabulary on the mnemosyne side).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Annotation being superseded (opaque ID string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    /// Required reason when `supersedes` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersede_reason: Option<String>,
}

// ── MnemosyneView ──────────────────────────────────────────────────────────

/// Projection of an [`ArtefactMeta`] into the mnemosyne `Annotation` JSON
/// shape. Intended for aletheia-side tooling that must emit annotation-shaped
/// JSON to the mnemosyne wire format.
///
/// `#[non_exhaustive]` per kanon #108 — field additions must not break
/// downstream `match`/struct-literal sites.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MnemosyneView {
    /// Actor identity — maps from `ArtefactMeta::actor_id`, falling back to
    /// `ArtefactMeta::producer` when `actor_id` is not set.
    pub agent_id: String,
    /// Creation timestamp — maps from `ArtefactMeta::generated_at`.
    pub created_at: String,
    /// Session identifier, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Evidence references — maps from `ArtefactMeta::evidence_refs`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_chunks: Vec<String>,
    /// Supersede target, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    /// Supersede reason, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersede_reason: Option<String>,
    /// Source kind (aletheia extension; not present in canonical mnemosyne
    /// `Annotation` wire format).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    /// Source locator (aletheia extension; not present in canonical mnemosyne
    /// `Annotation` wire format).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_locator: Option<String>,
    /// Confidence score (aletheia extension; no annotation analog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Schema version (aletheia extension; kanon issue filed for alignment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
}

// ── Adapter functions ──────────────────────────────────────────────────────

/// Build an [`ArtefactMeta`] from a mnemosyne annotation view.
///
/// The `producer` field is set to `"mnemosyne-import@0"` as a sentinel
/// because mnemosyne annotations carry no schema version or crate-name
/// concept. Callers that know the originating crate should overwrite
/// `producer` after this call.
///
/// The `generated_at` field is set from `annotation.created_at`.
/// The `schema_version` field is set to `0` (unknown from mnemosyne side).
///
/// # Field mapping
///
/// | Mnemosyne `MnemosyneAnnotationView` | `ArtefactMeta` |
/// |---|---|
/// | `agent_id` | `actor_id` |
/// | `created_at` | `generated_at` |
/// | `session_id` | `session_id` |
/// | `evidence_chunks` | `evidence_refs` |
/// | `supersedes` | `supersedes` |
/// | `supersede_reason` | `supersede_reason` |
/// | `claim` | dropped |
/// | `topic` | dropped |
#[tracing::instrument(skip(annotation), fields(agent_id = %annotation.agent_id))]
pub fn from_mnemosyne_annotation(annotation: &MnemosyneAnnotationView) -> ArtefactMeta {
    ArtefactMeta {
        producer: "mnemosyne-import@0".to_owned(),
        schema_version: 0,
        generated_at: annotation.created_at.clone(),
        row_counts: std::collections::BTreeMap::new(),
        actor_id: Some(annotation.agent_id.clone()),
        session_id: annotation.session_id.clone(),
        supersedes: annotation.supersedes.clone(),
        supersede_reason: annotation.supersede_reason.clone(),
        confidence: None,
        source_kind: None,
        source_locator: None,
        evidence_refs: annotation.evidence_chunks.clone(),
    }
}

/// Project an [`ArtefactMeta`] into a [`MnemosyneView`].
///
/// The `agent_id` in the view is sourced from `meta.actor_id` when present,
/// falling back to `meta.producer` so the view is always populated.
///
/// Fields that are artefact-specific (`row_counts`) have no annotation
/// analog and are dropped.
///
/// # Field mapping
///
/// | `ArtefactMeta` | `MnemosyneView` |
/// |---|---|
/// | `actor_id` (or `producer` as fallback) | `agent_id` |
/// | `generated_at` | `created_at` |
/// | `session_id` | `session_id` |
/// | `evidence_refs` | `evidence_chunks` |
/// | `supersedes` | `supersedes` |
/// | `supersede_reason` | `supersede_reason` |
/// | `source_kind` | `source_kind` |
/// | `source_locator` | `source_locator` |
/// | `confidence` | `confidence` |
/// | `schema_version` | `schema_version` |
/// | `row_counts` | dropped |
#[tracing::instrument(skip(meta), fields(producer = %meta.producer))]
pub fn to_mnemosyne_compatible(meta: &ArtefactMeta) -> MnemosyneView {
    let agent_id = meta
        .actor_id
        .clone()
        .unwrap_or_else(|| meta.producer.clone());

    MnemosyneView {
        agent_id,
        created_at: meta.generated_at.clone(),
        session_id: meta.session_id.clone(),
        evidence_chunks: meta.evidence_refs.clone(),
        supersedes: meta.supersedes.clone(),
        supersede_reason: meta.supersede_reason.clone(),
        source_kind: meta.source_kind.clone(),
        source_locator: meta.source_locator.clone(),
        confidence: meta.confidence,
        schema_version: Some(meta.schema_version),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    // ── Fixtures ─────────────────────────────────────────────────────────────

    fn make_annotation() -> MnemosyneAnnotationView {
        MnemosyneAnnotationView {
            agent_id: "claude-opus-4-7".to_owned(),
            created_at: "2026-04-22T10:00:00Z".to_owned(),
            claim: Some("The pattern races under parallel push.".to_owned()),
            session_id: Some("sess-abc".to_owned()),
            evidence_chunks: vec!["chunk-1".to_owned(), "chunk-2".to_owned()],
            topic: Some("concurrency".to_owned()),
            supersedes: Some("old-annotation-id".to_owned()),
            supersede_reason: Some("prior claim was incomplete".to_owned()),
        }
    }

    fn make_full_meta() -> ArtefactMeta {
        ArtefactMeta::new("mneme@0.21.1", 2, "2026-04-22T10:00:00Z")
            .with_actor("claude-opus-4-7", Some("sess-abc".to_owned()))
            .with_confidence(0.85)
            .with_source("git_repo", "https://git.example.com/x")
            .with_evidence(["chunk-1".to_owned(), "chunk-2".to_owned()])
            .with_supersede("old-id", "data corrected")
            .with_count("facts", 10)
    }

    // ── from_mnemosyne_annotation ─────────────────────────────────────────────

    #[test]
    fn from_annotation_maps_required_fields() {
        let ann = make_annotation();
        let meta = from_mnemosyne_annotation(&ann);

        assert_eq!(
            meta.actor_id.as_deref(),
            Some("claude-opus-4-7"),
            "actor_id"
        );
        assert_eq!(meta.generated_at, "2026-04-22T10:00:00Z", "generated_at");
        assert_eq!(meta.session_id.as_deref(), Some("sess-abc"), "session_id");
        assert_eq!(meta.evidence_refs, &["chunk-1", "chunk-2"], "evidence_refs");
        assert_eq!(
            meta.supersedes.as_deref(),
            Some("old-annotation-id"),
            "supersedes"
        );
        assert_eq!(
            meta.supersede_reason.as_deref(),
            Some("prior claim was incomplete"),
            "supersede_reason"
        );
    }

    #[test]
    fn from_annotation_uses_sentinel_producer() {
        let ann = make_annotation();
        let meta = from_mnemosyne_annotation(&ann);
        assert_eq!(meta.producer, "mnemosyne-import@0");
        assert_eq!(meta.schema_version, 0);
    }

    #[test]
    fn from_annotation_drops_claim_and_topic() {
        // claim and topic have no ArtefactMeta field; verify no panic/error.
        let ann = make_annotation();
        let meta = from_mnemosyne_annotation(&ann);
        // no way to assert absence — just confirm round-trip doesn't crash and
        // does not fabricate data in unexpected fields.
        assert!(
            meta.confidence.is_none(),
            "confidence not imported from annotation"
        );
        assert!(
            meta.source_kind.is_none(),
            "source_kind not imported from annotation"
        );
    }

    // ── to_mnemosyne_compatible ───────────────────────────────────────────────

    #[test]
    fn to_view_maps_all_supported_fields() {
        let meta = make_full_meta();
        let view = to_mnemosyne_compatible(&meta);

        assert_eq!(view.agent_id, "claude-opus-4-7", "agent_id from actor_id");
        assert_eq!(view.created_at, "2026-04-22T10:00:00Z", "created_at");
        assert_eq!(view.session_id.as_deref(), Some("sess-abc"), "session_id");
        assert_eq!(
            view.evidence_chunks,
            &["chunk-1", "chunk-2"],
            "evidence_chunks"
        );
        assert_eq!(view.supersedes.as_deref(), Some("old-id"), "supersedes");
        assert_eq!(
            view.supersede_reason.as_deref(),
            Some("data corrected"),
            "supersede_reason"
        );
        assert_eq!(view.source_kind.as_deref(), Some("git_repo"), "source_kind");
        assert_eq!(
            view.source_locator.as_deref(),
            Some("https://git.example.com/x"),
            "source_locator"
        );
        assert_eq!(view.confidence, Some(0.85_f32), "confidence");
        assert_eq!(view.schema_version, Some(2), "schema_version");
    }

    #[test]
    fn to_view_falls_back_to_producer_when_actor_id_absent() {
        let meta = ArtefactMeta::new("graphe@0.21.1", 1, "2026-04-22T00:00:00Z");
        let view = to_mnemosyne_compatible(&meta);
        assert_eq!(view.agent_id, "graphe@0.21.1");
    }

    #[test]
    fn to_view_drops_row_counts() {
        let meta =
            ArtefactMeta::new("graphe@0.21.1", 1, "2026-04-22T00:00:00Z").with_count("facts", 99);
        let view = to_mnemosyne_compatible(&meta);
        let json = serde_json::to_string(&view).expect("serialize");
        assert!(
            !json.contains("row_counts"),
            "row_counts should not appear in MnemosyneView"
        );
        assert!(
            !json.contains("facts"),
            "row_count keys should not appear in MnemosyneView"
        );
    }

    // ── Round-trip: ArtefactMeta → MnemosyneView → re-parse → ArtefactMeta ───

    #[test]
    fn round_trip_preserves_all_shared_fields() {
        let original = make_full_meta();

        // Step 1: project to mnemosyne JSON shape.
        let view = to_mnemosyne_compatible(&original);
        let view_json = serde_json::to_string(&view).expect("serialize view");

        // Step 2: re-parse as MnemosyneAnnotationView (mnemosyne consumer perspective).
        let ann: MnemosyneAnnotationView =
            serde_json::from_str(&view_json).expect("deserialize as annotation view");

        // Step 3: convert back to ArtefactMeta.
        let recovered = from_mnemosyne_annotation(&ann);

        assert_eq!(
            recovered.actor_id.as_deref(),
            original.actor_id.as_deref(),
            "actor_id survives round-trip"
        );
        assert_eq!(
            recovered.generated_at, original.generated_at,
            "generated_at survives round-trip"
        );
        assert_eq!(
            recovered.session_id.as_deref(),
            original.session_id.as_deref(),
            "session_id survives round-trip"
        );
        assert_eq!(
            recovered.evidence_refs, original.evidence_refs,
            "evidence_refs survive round-trip"
        );
        assert_eq!(
            recovered.supersedes.as_deref(),
            original.supersedes.as_deref(),
            "supersedes survives round-trip"
        );
        assert_eq!(
            recovered.supersede_reason.as_deref(),
            original.supersede_reason.as_deref(),
            "supersede_reason survives round-trip"
        );
    }

    #[test]
    fn round_trip_via_serde_json_string_preserves_view_fields() {
        let meta = make_full_meta();
        let view = to_mnemosyne_compatible(&meta);
        let json = serde_json::to_string(&view).expect("serialize");
        let back: MnemosyneView = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(view.agent_id, back.agent_id);
        assert_eq!(view.created_at, back.created_at);
        assert_eq!(view.session_id, back.session_id);
        assert_eq!(view.evidence_chunks, back.evidence_chunks);
        assert_eq!(view.supersedes, back.supersedes);
        assert_eq!(view.supersede_reason, back.supersede_reason);
        assert_eq!(view.source_kind, back.source_kind);
        assert_eq!(view.source_locator, back.source_locator);
        assert_eq!(view.confidence, back.confidence);
        assert_eq!(view.schema_version, back.schema_version);
    }

    // ── Coverage: every field maps or is documented as side-specific ──────────

    #[test]
    fn all_artefact_meta_optional_fields_covered_by_adapter() {
        // Construct ArtefactMeta with every optional field populated and verify
        // to_mnemosyne_compatible produces a non-default MnemosyneView.
        let meta = make_full_meta();
        let view = to_mnemosyne_compatible(&meta);

        assert!(view.session_id.is_some(), "session_id must appear in view");
        assert!(
            !view.evidence_chunks.is_empty(),
            "evidence_chunks must appear in view"
        );
        assert!(view.supersedes.is_some(), "supersedes must appear in view");
        assert!(
            view.supersede_reason.is_some(),
            "supersede_reason must appear in view"
        );
        assert!(
            view.source_kind.is_some(),
            "source_kind must appear in view"
        );
        assert!(
            view.source_locator.is_some(),
            "source_locator must appear in view"
        );
        assert!(view.confidence.is_some(), "confidence must appear in view");
        assert!(
            view.schema_version.is_some(),
            "schema_version must appear in view"
        );
        // row_counts is explicitly side-specific; covered by to_view_drops_row_counts.
    }

    #[test]
    fn all_annotation_view_fields_covered_by_adapter() {
        // Construct MnemosyneAnnotationView with every field and verify
        // from_mnemosyne_annotation doesn't panic or silently corrupt.
        let ann = make_annotation();
        let meta = from_mnemosyne_annotation(&ann);

        assert!(meta.actor_id.is_some(), "actor_id mapped");
        assert!(!meta.generated_at.is_empty(), "generated_at mapped");
        assert!(meta.session_id.is_some(), "session_id mapped");
        assert!(!meta.evidence_refs.is_empty(), "evidence_refs mapped");
        assert!(meta.supersedes.is_some(), "supersedes mapped");
        assert!(meta.supersede_reason.is_some(), "supersede_reason mapped");
        // claim and topic are side-specific and do not appear in ArtefactMeta.
    }

    // ── Backward-compat: new() callers still compile and produce correct struct ─

    #[test]
    fn artefact_meta_new_callers_remain_unchanged() {
        // Simulates existing callers that only use the three required args.
        let meta =
            ArtefactMeta::new("graphe@0.21.1", 3, "2026-04-22T12:00:00Z").with_count("edges", 500);
        assert_eq!(meta.producer, "graphe@0.21.1");
        assert_eq!(meta.schema_version, 3);
        assert_eq!(meta.row_counts.get("edges").copied(), Some(500));
        assert!(meta.actor_id.is_none());
        assert!(meta.confidence.is_none());
    }
}
