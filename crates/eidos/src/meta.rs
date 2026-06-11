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
//!
//! # Cross-shape alignment
//!
//! `ArtefactMeta` carries optional fields that mirror provenance and citation
//! fields used by eidos knowledge types. The canonical [`Provenance`] shape
//! lets callers project those older public structs into one vocabulary without
//! inventing missing data.
//!
//! | `Provenance` field | Existing eidos fields |
//! |--------------------|-----------------------|
//! | `actor_id` | `ArtefactMeta::actor_id`, `ArchitectureFact::updated_by` |
//! | `session_id` | `ArtefactMeta::session_id`, `FactProvenance::source_session_id` |
//! | `source_kind` | `VerificationRecord::source` |
//! | `source_locator` | `ArtefactMeta::source_locator`, `Finding::source` |
//! | `evidence_refs` | `ArtefactMeta::evidence_refs`, `ArchitectureFact::evidence` |
//! | `confidence` | `ArtefactMeta::confidence`, `FactProvenance::confidence` |
//! | `supersedes` / `supersede_reason` | `ArtefactMeta` supersession fields |
//! | `generated_at` | `ArtefactMeta::generated_at`, verification/edge timestamps |

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::knowledge::{
    CausalEdge, FactProvenance, VerificationRecord, architecture_fact::ArchitectureFact,
    finding::Finding, format_timestamp,
};

/// Every persistable artefact carries provenance metadata.
///
/// Implement this trait on your artefact struct and call `.stamp()` in the
/// persist path. The stamp is computed at the moment of persistence so that
/// `generated_at` reflects write time, not construction time.
pub trait Stamped {
    /// Returns the artefact's stamp at the moment of persistence.
    fn stamp(&self) -> ArtefactMeta;
}

/// Canonical provenance projection for eidos public knowledge shapes.
///
/// Each field is optional except `evidence_refs` so legacy structs can project
/// only the facts they actually carry. Conversion impls intentionally leave
/// unknown values empty instead of manufacturing source kinds, actors, or
/// timestamps from nearby but semantically different fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Provenance {
    /// Actor or producer responsible for the artefact, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    /// Session that produced or observed the artefact, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Kind of source or verification mechanism, when explicitly known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    /// Stable source locator such as a path, URL, or producer string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_locator: Option<String>,
    /// Evidence identifiers, paths, URLs, or fact references supporting the artefact.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    /// Normalized confidence score in `[0.0, 1.0]`, when the source carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// Identifier of the previous artefact superseded by this one, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    /// Human-readable reason for supersession, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersede_reason: Option<String>,
    /// RFC 3339 timestamp for generation, update, verification, or observation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
}

impl Provenance {
    /// Construct an empty provenance projection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set actor and optional session identity.
    #[must_use]
    pub fn with_actor(mut self, actor_id: impl Into<String>, session_id: Option<String>) -> Self {
        self.actor_id = Some(actor_id.into());
        self.session_id = session_id;
        self
    }

    /// Set source kind and locator.
    #[must_use]
    pub fn with_source(
        mut self,
        kind: Option<impl Into<String>>,
        locator: Option<impl Into<String>>,
    ) -> Self {
        self.source_kind = kind.map(Into::into);
        self.source_locator = locator.map(Into::into);
        self
    }

    /// Set evidence references.
    #[must_use]
    pub fn with_evidence(mut self, refs: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.evidence_refs = refs.into_iter().map(Into::into).collect();
        self
    }

    /// Set normalized confidence.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }

    /// Set supersession metadata.
    #[must_use]
    pub fn with_supersede(
        mut self,
        supersedes: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        self.supersedes = Some(supersedes.into());
        self.supersede_reason = Some(reason.into());
        self
    }

    /// Set the generation or observation timestamp.
    #[must_use]
    pub fn with_generated_at(mut self, generated_at: impl Into<String>) -> Self {
        self.generated_at = Some(generated_at.into());
        self
    }
}

/// Uniform provenance envelope attached to every persistable fleet artefact.
///
/// # Stability
///
/// WHY(#108): `#[non_exhaustive]` keeps downstream matches and struct literals
/// from breaking when new fields are appended.
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
///
/// # Mnemosyne-aligned optional fields
///
/// The `actor_id`, `session_id`, `supersedes`, `supersede_reason`,
/// `confidence`, `source_kind`, `source_locator`, and `evidence_refs` fields
/// align with the mnemosyne `Annotation` type (kanon). They are all
/// `Option<T>` so existing `ArtefactMeta::new()` callers remain unchanged
/// and existing serialized JSON deserializes cleanly.
///
/// See the [`meta`](crate::meta) module-level docs for the full field mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    /// Unified actor identity. Maps to mnemosyne `Annotation::agent_id` and
    /// episteme `Fact::nous_id`. Use `"<crate>@<version>"` for automated
    /// producers; use the agent ID string for nous-authored artefacts.
    ///
    /// **Mnemosyne analog**: `agent_id` on `Annotation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,

    /// Session that produced this artefact. Enables grouping related
    /// artefacts from a single nous session.
    ///
    /// **Mnemosyne analog**: `session_id` on `Annotation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Reference to the artefact (or annotation) this one supersedes. When
    /// set, `supersede_reason` must also be set.
    ///
    /// **Mnemosyne analog**: `supersedes` on `Annotation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,

    /// Human-readable reason for the supersession. Required when `supersedes`
    /// is `Some`; ignored otherwise.
    ///
    /// **Mnemosyne analog**: `supersede_reason` on `Annotation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersede_reason: Option<String>,

    /// Normalized confidence score in `[0.0, 1.0]`. Carried from episteme
    /// `Fact::confidence`. No direct mnemosyne `Annotation` analog (annotations
    /// use evidence chunk count as a proxy instead).
    ///
    /// **Episteme analog**: `confidence` on `Fact`.
    /// **Mnemosyne analog**: none — aletheia-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,

    /// Source kind that produced the data underlying this artefact
    /// (e.g. `"git_repo"`, `"source_tree"`, `"markdown_tree"`).
    ///
    /// **Mnemosyne analog**: `SourceKind` on `Source` (snake-case string form).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,

    /// Canonical locator for the ingest source (git URL, absolute path, etc.).
    ///
    /// **Mnemosyne analog**: `locator` on `Source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_locator: Option<String>,

    /// Stable IDs of evidence chunks (or facts) supporting this artefact.
    /// On the mnemosyne side these are typed `ChunkId`s; on the eidos side
    /// they are stored as opaque strings for cross-layer portability.
    ///
    /// **Mnemosyne analog**: `evidence_chunks` on `Annotation` (typed `Vec<ChunkId>`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
}

impl ArtefactMeta {
    /// Construct a stamp with required fields; `row_counts` starts empty and
    /// all mnemosyne-aligned optional fields are `None`.
    ///
    /// Chain builder methods to populate optional fields.
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
            actor_id: None,
            session_id: None,
            supersedes: None,
            supersede_reason: None,
            confidence: None,
            source_kind: None,
            source_locator: None,
            evidence_refs: Vec::new(),
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

    /// Set the actor identity and optional session. Aligns with mnemosyne
    /// `Annotation::agent_id` + `session_id`.
    #[must_use]
    pub fn with_actor(mut self, actor_id: impl Into<String>, session_id: Option<String>) -> Self {
        self.actor_id = Some(actor_id.into());
        self.session_id = session_id;
        self
    }

    /// Set the confidence score. Must be in `[0.0, 1.0]`; values outside that
    /// range are clamped at store time by consumers — eidos does not clamp here
    /// to avoid silent data mutation.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence);
        self
    }

    /// Set the ingest source kind and locator. Aligns with mnemosyne
    /// `Source::kind` (as snake-case string) and `Source::locator`.
    #[must_use]
    pub fn with_source(mut self, kind: impl Into<String>, locator: impl Into<String>) -> Self {
        self.source_kind = Some(kind.into());
        self.source_locator = Some(locator.into());
        self
    }

    /// Set evidence references. Aligns with mnemosyne
    /// `Annotation::evidence_chunks` (stored as opaque strings here).
    #[must_use]
    pub fn with_evidence(mut self, refs: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.evidence_refs = refs.into_iter().map(Into::into).collect();
        self
    }

    /// Set the supersede chain. `supersedes` is the reference to the artefact
    /// being replaced; `reason` is required when `supersedes` is `Some` and
    /// is validated by downstream consumers (not enforced here to avoid
    /// requiring `Result` on a builder).
    #[must_use]
    pub fn with_supersede(
        mut self,
        supersedes: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        self.supersedes = Some(supersedes.into());
        self.supersede_reason = Some(reason.into());
        self
    }

    /// Project this artefact envelope into canonical eidos provenance.
    #[must_use]
    pub fn provenance(&self) -> Provenance {
        Provenance::from(self)
    }
}

impl From<&ArtefactMeta> for Provenance {
    fn from(meta: &ArtefactMeta) -> Self {
        Self {
            actor_id: meta.actor_id.clone(),
            session_id: meta.session_id.clone(),
            source_kind: meta.source_kind.clone(),
            source_locator: meta.source_locator.clone(),
            evidence_refs: meta.evidence_refs.clone(),
            confidence: meta.confidence.map(f64::from),
            supersedes: meta.supersedes.clone(),
            supersede_reason: meta.supersede_reason.clone(),
            generated_at: Some(meta.generated_at.clone()),
        }
    }
}

impl From<ArtefactMeta> for Provenance {
    fn from(meta: ArtefactMeta) -> Self {
        Self::from(&meta)
    }
}

impl From<&FactProvenance> for Provenance {
    fn from(provenance: &FactProvenance) -> Self {
        Self {
            session_id: provenance.source_session_id.clone(),
            confidence: Some(provenance.confidence),
            ..Self::default()
        }
    }
}

impl From<FactProvenance> for Provenance {
    fn from(provenance: FactProvenance) -> Self {
        Self::from(&provenance)
    }
}

impl From<&VerificationRecord> for Provenance {
    fn from(record: &VerificationRecord) -> Self {
        Self {
            source_kind: Some(record.source.as_str().to_owned()),
            generated_at: Some(format_timestamp(&record.verified_at)),
            ..Self::default()
        }
    }
}

impl From<VerificationRecord> for Provenance {
    fn from(record: VerificationRecord) -> Self {
        Self::from(&record)
    }
}

impl From<&ArchitectureFact> for Provenance {
    fn from(fact: &ArchitectureFact) -> Self {
        Self {
            actor_id: Some(fact.updated_by.clone()),
            session_id: fact.mneme_session.clone(),
            evidence_refs: fact.evidence.clone(),
            generated_at: Some(fact.updated_at.clone()),
            ..Self::default()
        }
    }
}

impl From<ArchitectureFact> for Provenance {
    fn from(fact: ArchitectureFact) -> Self {
        Self::from(&fact)
    }
}

impl From<&Finding> for Provenance {
    fn from(finding: &Finding) -> Self {
        Self {
            source_locator: Some(finding.source.clone()),
            ..Self::default()
        }
    }
}

impl From<Finding> for Provenance {
    fn from(finding: Finding) -> Self {
        Self::from(&finding)
    }
}

impl From<&CausalEdge> for Provenance {
    fn from(edge: &CausalEdge) -> Self {
        Self {
            session_id: edge.evidence_session_id.clone(),
            confidence: Some(edge.confidence),
            generated_at: Some(format_timestamp(&edge.timestamp)),
            ..Self::default()
        }
    }
}

impl From<CausalEdge> for Provenance {
    fn from(edge: CausalEdge) -> Self {
        Self::from(&edge)
    }
}

/// Types that can project into canonical eidos provenance.
pub trait ProvenanceProject {
    /// Return a canonical provenance projection for this value.
    fn provenance(&self) -> Provenance;
}

impl<T> ProvenanceProject for T
where
    for<'a> Provenance: From<&'a T>,
{
    fn provenance(&self) -> Provenance {
        Provenance::from(self)
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    /// Assert two `ArtefactMeta` values are equal field-by-field. We do not
    /// derive `Eq` because `f32` doesn't implement `Eq`; we use approximate
    /// equality for `confidence` here.
    fn assert_meta_eq(a: &ArtefactMeta, b: &ArtefactMeta) {
        assert_eq!(a.producer, b.producer, "producer");
        assert_eq!(a.schema_version, b.schema_version, "schema_version");
        assert_eq!(a.generated_at, b.generated_at, "generated_at");
        assert_eq!(a.row_counts, b.row_counts, "row_counts");
        assert_eq!(a.actor_id, b.actor_id, "actor_id");
        assert_eq!(a.session_id, b.session_id, "session_id");
        assert_eq!(a.supersedes, b.supersedes, "supersedes");
        assert_eq!(a.supersede_reason, b.supersede_reason, "supersede_reason");
        assert_eq!(a.source_kind, b.source_kind, "source_kind");
        assert_eq!(a.source_locator, b.source_locator, "source_locator");
        assert_eq!(a.evidence_refs, b.evidence_refs, "evidence_refs");
        // f32 comparison: must be identical bit-for-bit after serde round-trip.
        assert_eq!(a.confidence, b.confidence, "confidence");
    }

    #[test]
    fn artefact_meta_new_round_trip_serde() {
        let meta = ArtefactMeta::new("mneme@0.21.1", 1, "2026-04-22T00:00:00Z");
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: ArtefactMeta = serde_json::from_str(&json).expect("deserialize");
        assert_meta_eq(&meta, &back);
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

    #[test]
    fn artefact_meta_legacy_json_deserializes_cleanly() {
        // Simulates JSON produced before the mnemosyne-aligned fields were added.
        let legacy = r#"{
            "producer": "mneme@0.21.0",
            "schema_version": 1,
            "generated_at": "2026-04-01T00:00:00Z",
            "row_counts": {"facts": 100}
        }"#;
        let meta: ArtefactMeta = serde_json::from_str(legacy).expect("deserialize legacy JSON");
        assert_eq!(meta.producer, "mneme@0.21.0");
        assert_eq!(meta.schema_version, 1);
        assert!(meta.actor_id.is_none(), "actor_id defaults to None");
        assert!(meta.session_id.is_none(), "session_id defaults to None");
        assert!(meta.supersedes.is_none(), "supersedes defaults to None");
        assert!(
            meta.supersede_reason.is_none(),
            "supersede_reason defaults to None"
        );
        assert!(meta.confidence.is_none(), "confidence defaults to None");
        assert!(meta.source_kind.is_none(), "source_kind defaults to None");
        assert!(
            meta.source_locator.is_none(),
            "source_locator defaults to None"
        );
        assert!(
            meta.evidence_refs.is_empty(),
            "evidence_refs defaults to empty"
        );
    }

    #[test]
    fn artefact_meta_with_actor_sets_fields() {
        let meta = ArtefactMeta::new("mneme@0.21.1", 1, "2026-04-22T00:00:00Z")
            .with_actor("claude-opus-4-7", Some("session-abc".to_owned()));
        assert_eq!(meta.actor_id.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(meta.session_id.as_deref(), Some("session-abc"));
    }

    #[test]
    fn artefact_meta_with_confidence_sets_field() {
        let meta =
            ArtefactMeta::new("mneme@0.21.1", 1, "2026-04-22T00:00:00Z").with_confidence(0.87);
        assert_eq!(meta.confidence, Some(0.87_f32));
    }

    #[test]
    fn artefact_meta_with_source_sets_fields() {
        let meta = ArtefactMeta::new("mneme@0.21.1", 1, "2026-04-22T00:00:00Z")
            .with_source("git_repo", "https://github.com/example/repo");
        assert_eq!(meta.source_kind.as_deref(), Some("git_repo"));
        assert_eq!(
            meta.source_locator.as_deref(),
            Some("https://github.com/example/repo")
        );
    }

    #[test]
    fn artefact_meta_with_evidence_sets_refs() {
        let refs = ["chunk-id-1", "chunk-id-2"];
        let meta = ArtefactMeta::new("mneme@0.21.1", 1, "2026-04-22T00:00:00Z").with_evidence(refs);
        assert_eq!(meta.evidence_refs, &["chunk-id-1", "chunk-id-2"]);
    }

    #[test]
    fn artefact_meta_with_supersede_sets_both_fields() {
        let meta = ArtefactMeta::new("mneme@0.21.1", 1, "2026-04-22T00:00:00Z")
            .with_supersede("old-artefact-id", "schema upgraded to v2");
        assert_eq!(meta.supersedes.as_deref(), Some("old-artefact-id"));
        assert_eq!(
            meta.supersede_reason.as_deref(),
            Some("schema upgraded to v2")
        );
    }

    #[test]
    fn artefact_meta_full_optional_fields_round_trip_serde() {
        let meta = ArtefactMeta::new("mneme@0.21.1", 1, "2026-04-22T00:00:00Z")
            .with_actor("claude-opus-4-7", Some("sess-1".to_owned()))
            .with_confidence(0.75)
            .with_source("git_repo", "https://git.example.com/repo")
            .with_evidence(["c1".to_owned(), "c2".to_owned()])
            .with_supersede("prev-id", "data corrected");
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: ArtefactMeta = serde_json::from_str(&json).expect("deserialize");
        assert_meta_eq(&meta, &back);
    }

    #[test]
    fn artefact_meta_minimal_serializes_without_optional_keys() {
        let meta = ArtefactMeta::new("test@1.0.0", 1, "2026-04-22T00:00:00Z");
        let json = serde_json::to_string(&meta).expect("serialize");
        assert!(
            !json.contains("actor_id"),
            "actor_id should be absent when None"
        );
        assert!(
            !json.contains("confidence"),
            "confidence should be absent when None"
        );
        assert!(
            !json.contains("evidence_refs"),
            "evidence_refs should be absent when empty"
        );
        assert!(
            !json.contains("supersedes"),
            "supersedes should be absent when None"
        );
    }
}
