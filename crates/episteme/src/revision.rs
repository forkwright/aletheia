//! Retroactive knowledge revision with provenance-based impact analysis.
//!
//! When a canonical fact is proven incorrect, this module traverses the
//! provenance graph to identify all downstream facts, dispatch decisions,
//! and merged PRs that were influenced by the wrong information.
//!
//! WHY: Knowledge graphs accumulate errors. Without revision provenance,
//! confident-but-wrong facts silently corrupt downstream reasoning.
//! A single wrong assumption about a dependency can propagate through
//! hundreds of derived facts.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::id::FactId;

// ---------------------------------------------------------------------------
// Revision types
// ---------------------------------------------------------------------------

/// A request to revise a fact that was proven incorrect.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RevisionRequest {
    /// The fact being revised.
    pub fact_id: FactId,
    /// What was wrong about the original fact.
    pub reason: String,
    /// The corrected information, if available.
    pub correction: Option<String>,
    /// Who initiated the revision.
    pub revised_by: String,
}

/// Severity of a revision's impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RevisionSeverity {
    /// Description was imprecise but conclusions hold.
    Cosmetic,
    /// Architectural decision was based on wrong information.
    Structural,
    /// Security assumption was false — immediate action needed.
    Critical,
}

/// A fact affected by a revision, with its relationship to the revised fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AffectedFact {
    /// The affected fact's ID.
    pub fact_id: FactId,
    /// How many hops from the revised fact (1 = direct dependency).
    pub depth: u32,
    /// The causal chain from the revised fact to this one.
    pub chain: Vec<FactId>,
}

/// Result of a provenance-based impact analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RevisionImpact {
    /// The fact that was revised.
    pub revised_fact: FactId,
    /// All facts directly or transitively derived from the revised fact.
    pub affected_facts: Vec<AffectedFact>,
    /// Total number of affected facts.
    pub total_affected: usize,
    /// Maximum depth in the provenance chain.
    pub max_depth: u32,
    /// Suggested severity based on the number and depth of affected facts.
    pub suggested_severity: RevisionSeverity,
}

// ---------------------------------------------------------------------------
// Impact analysis
// ---------------------------------------------------------------------------

/// Compute the impact of revising a fact by traversing the provenance graph.
///
/// Uses BFS from the revised fact through causal edges to find all
/// transitively affected facts. Returns the full impact analysis.
///
/// `edges` is a map of `source_fact_id → [target_fact_ids]` representing
/// the causal graph (source caused/enabled target).
#[must_use]
pub fn analyze_impact(
    revised_fact: &FactId,
    edges: &HashMap<String, Vec<String>>,
) -> RevisionImpact {
    let mut affected = Vec::new();
    let mut visited = HashSet::new();
    let mut queue: VecDeque<(String, u32, Vec<FactId>)> = VecDeque::new();
    let mut max_depth = 0_u32;

    // Seed BFS from the revised fact's direct effects.
    visited.insert(revised_fact.as_str().to_owned());
    if let Some(targets) = edges.get(revised_fact.as_str()) {
        for target in targets {
            if visited.insert(target.clone()) {
                let chain = vec![revised_fact.clone()];
                queue.push_back((target.clone(), 1, chain));
            }
        }
    }

    while let Some((current, depth, chain)) = queue.pop_front() {
        max_depth = max_depth.max(depth);

        let mut full_chain = chain.clone();
        if let Ok(current_id) = FactId::new(current.as_str()) {
            full_chain.push(current_id.clone());
            affected.push(AffectedFact {
                fact_id: current_id,
                depth,
                chain: full_chain.clone(),
            });
        }

        // Continue BFS to transitive effects.
        if let Some(targets) = edges.get(&current) {
            for target in targets {
                if visited.insert(target.clone()) {
                    queue.push_back((target.clone(), depth + 1, full_chain.clone()));
                }
            }
        }
    }

    let total_affected = affected.len();
    let suggested_severity = classify_severity(total_affected, max_depth);

    RevisionImpact {
        revised_fact: revised_fact.clone(),
        affected_facts: affected,
        total_affected,
        max_depth,
        suggested_severity,
    }
}

/// Classify revision severity based on blast radius.
fn classify_severity(total_affected: usize, max_depth: u32) -> RevisionSeverity {
    if total_affected > 50 || max_depth > 5 {
        RevisionSeverity::Critical
    } else if total_affected > 10 || max_depth > 3 {
        RevisionSeverity::Structural
    } else {
        RevisionSeverity::Cosmetic
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_edges(pairs: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
        let mut edges: HashMap<String, Vec<String>> = HashMap::new();
        for (src, dst) in pairs {
            edges
                .entry(src.to_string())
                .or_default()
                .push(dst.to_string());
        }
        edges
    }

    #[test]
    fn no_downstream() {
        let edges = make_edges(&[]);
        let fact = FactId::new("fact-001").unwrap();
        let impact = analyze_impact(&fact, &edges);
        assert_eq!(impact.total_affected, 0);
        assert_eq!(impact.max_depth, 0);
        assert_eq!(impact.suggested_severity, RevisionSeverity::Cosmetic);
    }

    #[test]
    fn direct_effects_only() {
        let edges = make_edges(&[("fact-001", "fact-002"), ("fact-001", "fact-003")]);
        let fact = FactId::new("fact-001").unwrap();
        let impact = analyze_impact(&fact, &edges);
        assert_eq!(impact.total_affected, 2);
        assert_eq!(impact.max_depth, 1);
    }

    #[test]
    fn transitive_chain() {
        let edges = make_edges(&[
            ("fact-001", "fact-002"),
            ("fact-002", "fact-003"),
            ("fact-003", "fact-004"),
        ]);
        let fact = FactId::new("fact-001").unwrap();
        let impact = analyze_impact(&fact, &edges);
        assert_eq!(impact.total_affected, 3);
        assert_eq!(impact.max_depth, 3);
    }

    #[test]
    fn diamond_graph_no_double_count() {
        // fact-001 → fact-002 → fact-004
        // fact-001 → fact-003 → fact-004
        let edges = make_edges(&[
            ("fact-001", "fact-002"),
            ("fact-001", "fact-003"),
            ("fact-002", "fact-004"),
            ("fact-003", "fact-004"),
        ]);
        let fact = FactId::new("fact-001").unwrap();
        let impact = analyze_impact(&fact, &edges);
        // fact-004 should be counted once, not twice.
        assert_eq!(impact.total_affected, 3);
    }

    #[test]
    fn cycle_handling() {
        // fact-001 → fact-002 → fact-003 → fact-001 (cycle)
        let edges = make_edges(&[
            ("fact-001", "fact-002"),
            ("fact-002", "fact-003"),
            ("fact-003", "fact-001"),
        ]);
        let fact = FactId::new("fact-001").unwrap();
        let impact = analyze_impact(&fact, &edges);
        // Should not infinite-loop. Visited set prevents re-traversal.
        assert_eq!(impact.total_affected, 2);
    }

    #[test]
    fn severity_classification() {
        assert_eq!(classify_severity(5, 2), RevisionSeverity::Cosmetic);
        assert_eq!(classify_severity(15, 2), RevisionSeverity::Structural);
        assert_eq!(classify_severity(5, 4), RevisionSeverity::Structural);
        assert_eq!(classify_severity(60, 1), RevisionSeverity::Critical);
        assert_eq!(classify_severity(5, 6), RevisionSeverity::Critical);
    }

    #[test]
    fn affected_fact_chain_recorded() {
        let edges = make_edges(&[("fact-001", "fact-002"), ("fact-002", "fact-003")]);
        let fact = FactId::new("fact-001").unwrap();
        let impact = analyze_impact(&fact, &edges);

        let fact_003 = impact
            .affected_facts
            .iter()
            .find(|a| a.fact_id.as_str() == "fact-003")
            .unwrap();
        assert_eq!(fact_003.depth, 2);
        assert_eq!(fact_003.chain.len(), 3); // fact-001 → fact-002 → fact-003
    }
}
