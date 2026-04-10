//! Causal edge store: in-memory index for causal relationships between facts.
//!
//! `CausalStore` holds a directed graph of [`CausalEdge`] values and exposes
//! two traversal operations:
//!
//! - [`CausalStore::trace_causes`] — walk backwards from a fact to find all
//!   facts that contributed to it (upstream causes).
//! - [`CausalStore::trace_effects`] — walk forwards from a fact to find all
//!   facts it caused or enabled (downstream effects).
//!
//! Both traversals are depth-first with cycle detection. Transitive confidence
//! is the product of individual edge confidences along the chain.
//!
//! Causal edges are heuristically extracted during session finalization.
//! See the crate-private `extract_causal_edges` helper for the extraction
//! logic.

use std::collections::{HashMap, HashSet};

use snafu::Snafu;

use crate::knowledge::{CausalEdge, CausalRelationType, TemporalOrdering};
use eidos::id::{CausalEdgeId, FactId};

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors from the causal edge store.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum CausalError {
    /// An edge with the same ID already exists in the store.
    #[snafu(display("causal edge already exists: {id}"))]
    DuplicateEdge {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The requested fact ID has no causal edges in the store.
    #[snafu(display("no causal edges found for fact: {fact_id}"))]
    FactNotFound {
        fact_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

// ── Traversal result ──────────────────────────────────────────────────────────

/// A single step in a causal chain traversal.
#[derive(Debug, Clone)]
pub struct CausalChainNode {
    /// The fact at this position in the chain.
    pub fact_id: FactId,
    /// The edge that connects this node to the previous one.
    ///
    /// `None` for the root node (the fact the traversal started from).
    pub via_edge: Option<CausalEdge>,
    /// Cumulative confidence along the path from the root to this node.
    ///
    /// Product of all edge confidences on the path. Root node has confidence 1.0.
    pub chain_confidence: f64,
    /// Depth from the root (0 = root).
    pub depth: usize,
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// In-memory index of causal edges between facts.
///
/// Maintains two adjacency maps for O(1) edge lookup in both directions:
/// - `causes_of`: `target_id` → edges where `target_id` is the effect
/// - `effects_of`: `source_id` → edges where `source_id` is the cause
///
/// Edges are stored by ID in a flat map; the adjacency maps hold IDs only.
#[derive(Debug, Default)]
pub struct CausalStore {
    edges: HashMap<CausalEdgeId, CausalEdge>,
    /// Maps an effect fact to the IDs of edges that caused it.
    causes_of: HashMap<FactId, Vec<CausalEdgeId>>,
    /// Maps a cause fact to the IDs of edges it produced.
    effects_of: HashMap<FactId, Vec<CausalEdgeId>>,
}

impl CausalStore {
    /// Create an empty causal store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a causal edge to the store.
    ///
    /// # Errors
    /// Returns [`CausalError::DuplicateEdge`] if an edge with the same ID
    /// already exists.
    pub fn add_edge(&mut self, edge: CausalEdge) -> Result<(), CausalError> {
        if self.edges.contains_key(&edge.id) {
            return DuplicateEdgeSnafu {
                id: edge.id.as_str().to_owned(),
            }
            .fail();
        }
        let edge_id = edge.id.clone();
        let source = edge.source_id.clone();
        let target = edge.target_id.clone();
        self.edges.insert(edge_id.clone(), edge);
        self.causes_of
            .entry(target)
            .or_default()
            .push(edge_id.clone());
        self.effects_of.entry(source).or_default().push(edge_id);
        Ok(())
    }

    /// Return all edges in the store as an unordered slice reference.
    pub fn all_edges(&self) -> impl Iterator<Item = &CausalEdge> {
        self.edges.values()
    }

    /// Look up a single edge by ID.
    #[must_use]
    pub fn get_edge(&self, id: &CausalEdgeId) -> Option<&CausalEdge> {
        self.edges.get(id)
    }

    /// Return all edges where `fact_id` is the effect (i.e. its causes).
    #[must_use]
    pub fn direct_causes(&self, fact_id: &FactId) -> Vec<&CausalEdge> {
        self.causes_of
            .get(fact_id)
            .map(|ids| ids.iter().filter_map(|id| self.edges.get(id)).collect())
            .unwrap_or_default()
    }

    /// Return all edges where `fact_id` is the cause (i.e. its effects).
    #[must_use]
    pub fn direct_effects(&self, fact_id: &FactId) -> Vec<&CausalEdge> {
        self.effects_of
            .get(fact_id)
            .map(|ids| ids.iter().filter_map(|id| self.edges.get(id)).collect())
            .unwrap_or_default()
    }

    /// Walk backwards from `fact_id`, returning all facts that (transitively)
    /// caused or enabled it.
    ///
    /// The result is depth-first, root-first. The first node is the starting
    /// fact itself with `chain_confidence = 1.0` and `via_edge = None`.
    ///
    /// Cycle detection is performed via a visited set; cycles are silently
    /// skipped rather than surfaced as errors because the store may contain
    /// heuristically extracted edges that form loops.
    #[must_use]
    pub fn trace_causes(&self, fact_id: &FactId) -> Vec<CausalChainNode> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let root = CausalChainNode {
            fact_id: fact_id.clone(),
            via_edge: None,
            chain_confidence: 1.0,
            depth: 0,
        };
        self.dfs_causes(root, &mut visited, &mut result);
        result
    }

    /// Walk forwards from `fact_id`, returning all facts that this fact
    /// (transitively) caused or enabled.
    ///
    /// Same structure as [`trace_causes`](Self::trace_causes): root-first,
    /// depth-first, cycle-safe.
    #[must_use]
    pub fn trace_effects(&self, fact_id: &FactId) -> Vec<CausalChainNode> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let root = CausalChainNode {
            fact_id: fact_id.clone(),
            via_edge: None,
            chain_confidence: 1.0,
            depth: 0,
        };
        self.dfs_effects(root, &mut visited, &mut result);
        result
    }

    fn dfs_causes(
        &self,
        node: CausalChainNode,
        visited: &mut HashSet<FactId>,
        out: &mut Vec<CausalChainNode>,
    ) {
        if !visited.insert(node.fact_id.clone()) {
            return; // cycle — already seen this node
        }
        let parent_confidence = node.chain_confidence;
        let parent_depth = node.depth;
        let parent_id = node.fact_id.clone();
        out.push(node);
        for edge in self.direct_causes(&parent_id) {
            let child = CausalChainNode {
                fact_id: edge.source_id.clone(),
                via_edge: Some(edge.clone()),
                chain_confidence: parent_confidence * edge.confidence,
                depth: parent_depth + 1,
            };
            self.dfs_causes(child, visited, out);
        }
    }

    fn dfs_effects(
        &self,
        node: CausalChainNode,
        visited: &mut HashSet<FactId>,
        out: &mut Vec<CausalChainNode>,
    ) {
        if !visited.insert(node.fact_id.clone()) {
            return; // cycle — already seen this node
        }
        let parent_confidence = node.chain_confidence;
        let parent_depth = node.depth;
        let parent_id = node.fact_id.clone();
        out.push(node);
        for edge in self.direct_effects(&parent_id) {
            let child = CausalChainNode {
                fact_id: edge.target_id.clone(),
                via_edge: Some(edge.clone()),
                chain_confidence: parent_confidence * edge.confidence,
                depth: parent_depth + 1,
            };
            self.dfs_effects(child, visited, out);
        }
    }
}

// ── Heuristic extraction ──────────────────────────────────────────────────────

/// Causal cue patterns with their associated relation type.
///
/// Each entry is (`keyword`, `relation_type`, `confidence`). The keyword
/// is matched case-insensitively against the combined session text.
/// Confidence reflects the extraction heuristic quality for that cue.
const CAUSAL_CUES: &[(&str, CausalRelationType, f64)] = &[
    ("because", CausalRelationType::Caused, 0.6),
    ("caused by", CausalRelationType::Caused, 0.75),
    ("therefore", CausalRelationType::Caused, 0.6),
    ("as a result", CausalRelationType::Caused, 0.65),
    ("led to", CausalRelationType::Caused, 0.7),
    ("resulted in", CausalRelationType::Caused, 0.7),
    ("so that", CausalRelationType::Enabled, 0.55),
    ("enabled", CausalRelationType::Enabled, 0.65),
    ("allowed", CausalRelationType::Enabled, 0.55),
    ("prevented", CausalRelationType::Prevented, 0.65),
    ("blocked", CausalRelationType::Prevented, 0.6),
    ("stopped", CausalRelationType::Prevented, 0.6),
    ("correlated with", CausalRelationType::Correlated, 0.5),
    ("associated with", CausalRelationType::Correlated, 0.45),
];

/// Detect the strongest causal cue in `text`.
///
/// Returns `Some((CausalRelationType, confidence))` for the first
/// (highest-confidence) matching cue, or `None` if no cues match.
///
/// WHY: we return on the first match rather than collecting all matches
/// because a single sentence rarely expresses two distinct causal relations;
/// the first match is used to gate edge creation. If multiple relations are
/// needed, callers should segment text before calling.
#[must_use]
pub(crate) fn detect_causal_cue(text: &str) -> Option<(CausalRelationType, f64)> {
    let lower = text.to_lowercase();
    CAUSAL_CUES
        .iter()
        .find(|(cue, _, _)| lower.contains(cue))
        .map(|(_, rel, conf)| (*rel, *conf))
}

/// Extract causal edges from session text given a source and target fact ID.
///
/// Scans `session_text` for causal signal words. If a cue is found, builds a
/// [`CausalEdge`] linking `source_id` → `target_id` with the cue's confidence
/// and the heuristically detected relation type. Returns an empty `Vec` when
/// no causal language is detected.
///
/// The caller is responsible for assigning semantically meaningful source/target
/// fact IDs. This function only handles the text → edge mapping.
///
/// # Errors
/// Never fails — returns `Vec` rather than `Result` because extraction is
/// best-effort; failure to detect causality is not an error condition.
#[must_use]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "documented hook for RefinedExtraction.causal_signal consumers; exercised from tests until first in-tree caller lands")
)]
pub(crate) fn extract_causal_edges(
    session_text: &str,
    source_id: FactId,
    target_id: FactId,
    session_id: Option<&str>,
) -> Vec<CausalEdge> {
    let Some((relation_type, confidence)) = detect_causal_cue(session_text) else {
        return vec![];
    };

    // WHY: ulid gives monotonically sortable IDs for edges within a session.
    let edge_id_str = koina::ulid::Ulid::new().to_string();
    let Ok(edge_id) = CausalEdgeId::new(edge_id_str) else {
        // ID generation failed (empty string) — should never happen with ulid.
        return vec![];
    };

    let edge = CausalEdge {
        id: edge_id,
        source_id,
        target_id,
        relationship_type: relation_type,
        // WHY: heuristic extraction cannot determine temporal ordering from
        // text alone without temporal NLP. Default to Before (cause precedes
        // effect), which is the standard assumption and correct for most cases.
        ordering: TemporalOrdering::Before,
        confidence,
        evidence_session_id: session_id.map(str::to_owned),
        timestamp: jiff::Timestamp::now(),
    };

    vec![edge]
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions on collections with known length")]
mod tests {
    use super::*;

    fn fact_id(s: &str) -> FactId {
        FactId::new(s).expect("valid fact id")
    }

    fn edge_id(s: &str) -> CausalEdgeId {
        CausalEdgeId::new(s).expect("valid edge id")
    }

    fn make_edge(id: &str, src: &str, tgt: &str, conf: f64) -> CausalEdge {
        CausalEdge {
            id: edge_id(id),
            source_id: fact_id(src),
            target_id: fact_id(tgt),
            relationship_type: CausalRelationType::Caused,
            ordering: TemporalOrdering::Before,
            confidence: conf,
            evidence_session_id: None,
            timestamp: jiff::Timestamp::now(),
        }
    }

    #[test]
    fn add_and_retrieve_edge() {
        let mut store = CausalStore::new();
        let edge = make_edge("e1", "fact-a", "fact-b", 0.8);
        store.add_edge(edge).expect("first insert ok");
        assert_eq!(store.all_edges().count(), 1);
    }

    #[test]
    fn duplicate_edge_rejected() {
        let mut store = CausalStore::new();
        let e1 = make_edge("e1", "fact-a", "fact-b", 0.8);
        let e2 = make_edge("e1", "fact-a", "fact-b", 0.5);
        store.add_edge(e1).expect("first insert ok");
        assert!(matches!(
            store.add_edge(e2),
            Err(CausalError::DuplicateEdge { .. })
        ));
    }

    #[test]
    fn trace_causes_single_hop() {
        let mut store = CausalStore::new();
        // fact-a caused fact-b
        store
            .add_edge(make_edge("e1", "fact-a", "fact-b", 0.8))
            .expect("ok");
        let chain = store.trace_causes(&fact_id("fact-b"));
        // chain: [fact-b (root), fact-a (cause)]
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].fact_id, fact_id("fact-b"));
        assert!(chain[0].via_edge.is_none());
        assert_eq!(chain[1].fact_id, fact_id("fact-a"));
        assert!((chain[1].chain_confidence - 0.8).abs() < 1e-9);
    }

    #[test]
    fn trace_effects_single_hop() {
        let mut store = CausalStore::new();
        // fact-a caused fact-b
        store
            .add_edge(make_edge("e1", "fact-a", "fact-b", 0.9))
            .expect("ok");
        let chain = store.trace_effects(&fact_id("fact-a"));
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].fact_id, fact_id("fact-a"));
        assert_eq!(chain[1].fact_id, fact_id("fact-b"));
        assert!((chain[1].chain_confidence - 0.9).abs() < 1e-9);
    }

    #[test]
    fn chain_confidence_is_product() {
        let mut store = CausalStore::new();
        // fact-a (0.8) → fact-b (0.5) → fact-c
        store
            .add_edge(make_edge("e1", "fact-a", "fact-b", 0.8))
            .expect("ok");
        store
            .add_edge(make_edge("e2", "fact-b", "fact-c", 0.5))
            .expect("ok");
        let chain = store.trace_effects(&fact_id("fact-a"));
        let fact_c_node = chain.iter().find(|n| n.fact_id == fact_id("fact-c"));
        let conf = fact_c_node.expect("fact-c in chain").chain_confidence;
        assert!((conf - 0.4).abs() < 1e-9, "expected 0.8*0.5=0.4, got {conf}");
    }

    #[test]
    fn cycle_detection_does_not_loop() {
        let mut store = CausalStore::new();
        // a → b → a (cycle)
        store
            .add_edge(make_edge("e1", "fact-a", "fact-b", 0.7))
            .expect("ok");
        store
            .add_edge(make_edge("e2", "fact-b", "fact-a", 0.7))
            .expect("ok");
        // Should terminate without infinite recursion.
        let chain = store.trace_effects(&fact_id("fact-a"));
        assert!(chain.len() <= 3, "cycle must be cut off; got {}", chain.len());
    }

    #[test]
    fn detect_causal_cue_because() {
        let (rel, conf) = detect_causal_cue("it failed because the config was wrong")
            .expect("cue found");
        assert_eq!(rel, CausalRelationType::Caused);
        assert!(conf > 0.5);
    }

    #[test]
    fn detect_causal_cue_prevented() {
        let (rel, _conf) =
            detect_causal_cue("rate limiting prevented the cascade").expect("cue found");
        assert_eq!(rel, CausalRelationType::Prevented);
    }

    #[test]
    fn detect_causal_cue_none() {
        assert!(detect_causal_cue("the sky is blue").is_none());
    }

    #[test]
    fn extract_causal_edges_no_cue() {
        let edges = extract_causal_edges(
            "the sky is blue",
            fact_id("fact-a"),
            fact_id("fact-b"),
            None,
        );
        assert!(edges.is_empty());
    }

    #[test]
    fn extract_causal_edges_with_cue() {
        let edges = extract_causal_edges(
            "the build failed because the dependency was missing",
            fact_id("fact-a"),
            fact_id("fact-b"),
            Some("session-123"),
        );
        assert_eq!(edges.len(), 1);
        let edge = &edges[0];
        assert_eq!(edge.source_id, fact_id("fact-a"));
        assert_eq!(edge.target_id, fact_id("fact-b"));
        assert_eq!(edge.relationship_type, CausalRelationType::Caused);
        assert_eq!(edge.evidence_session_id.as_deref(), Some("session-123"));
    }

    #[test]
    fn causal_relation_type_roundtrip() {
        for rel in [
            CausalRelationType::Caused,
            CausalRelationType::Enabled,
            CausalRelationType::Prevented,
            CausalRelationType::Correlated,
        ] {
            let s = rel.as_str();
            let parsed: CausalRelationType = s.parse().expect("roundtrip");
            assert_eq!(rel, parsed, "roundtrip failed for {s}");
        }
    }
}
