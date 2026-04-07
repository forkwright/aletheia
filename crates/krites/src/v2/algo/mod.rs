//! Graph algorithms for krites v2 fixed rules.
//!
//! Fixed rules are invoked via the `<~` syntax in Datalog queries.
//! All algorithms operate on `(source, target, weight)` edge triples
//! and return [`Rows`] with algorithm-specific columns.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::v2::error::{AlgorithmSnafu, Result};
use crate::v2::rows::Rows;
use crate::v2::value::Value;

// ---------------------------------------------------------------------------
// Algorithm modules
// ---------------------------------------------------------------------------

pub mod centrality;
pub mod clustering;
pub mod community;
pub mod connectivity;
pub mod pagerank;
pub mod path;
pub mod spanning;
pub mod traversal;

// ---------------------------------------------------------------------------
// FixedRule trait
// ---------------------------------------------------------------------------

/// Trait for fixed-rule graph algorithms.
///
/// Each algorithm is a struct implementing this trait, registered in the
/// [`FixedRuleRegistry`] and invoked via `<~` syntax in Datalog.
pub trait FixedRule: Send + Sync {
    /// Algorithm name (used in `<~ name` syntax).
    fn name(&self) -> &str;

    /// Return the output arity (number of columns) for this algorithm.
    ///
    /// The arity may depend on options (e.g., path algorithms returning
    /// different column sets based on `format` option).
    fn arity(&self, options: &BTreeMap<String, Value>) -> Result<usize>;

    /// Execute the algorithm on the given edge set.
    ///
    /// # Arguments
    /// * `edges` - Source, target, weight triples from the input relation.
    /// * `options` - Algorithm-specific options from the query.
    ///
    /// # Returns
    /// Result rows with algorithm-specific columns.
    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows>;
}

// ---------------------------------------------------------------------------
// FixedRuleRegistry
// ---------------------------------------------------------------------------

/// Registry of fixed-rule algorithms.
///
/// Maps algorithm names to their implementations. Use [`Self::with_defaults()`]
/// to register all built-in algorithms.
pub struct FixedRuleRegistry {
    rules: FxHashMap<String, Box<dyn FixedRule>>,
}

impl Default for FixedRuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FixedRuleRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rules: FxHashMap::default(),
        }
    }

    /// Create a registry with all built-in algorithms registered.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_defaults();
        registry
    }

    /// Register all default algorithms.
    fn register_defaults(&mut self) {
        // PageRank
        self.register(Box::new(pagerank::PageRank));

        // Community detection
        self.register(Box::new(community::Louvain));
        self.register(Box::new(community::LabelPropagation));

        // Path algorithms
        self.register(Box::new(path::BfsPath));
        self.register(Box::new(path::DijkstraPath));
        self.register(Box::new(path::AStarPath));
        self.register(Box::new(path::YenKShortest));

        // Centrality
        self.register(Box::new(centrality::DegreeCentrality));
        self.register(Box::new(centrality::ClosenessCentrality));
        self.register(Box::new(centrality::BetweennessCentrality));

        // Traversal
        self.register(Box::new(traversal::DfsTraversal));
        self.register(Box::new(traversal::BfsTraversal));
        self.register(Box::new(traversal::RandomWalk));

        // Spanning tree/forest
        self.register(Box::new(spanning::PrimMst));
        self.register(Box::new(spanning::KruskalMsf));

        // Connectivity
        self.register(Box::new(connectivity::ConnectedComponents));
        self.register(Box::new(connectivity::StronglyConnectedComponents));

        // Clustering
        self.register(Box::new(clustering::KCore));
        self.register(Box::new(clustering::ClusteringCoefficients));
        self.register(Box::new(clustering::TopSort));
    }

    /// Register a fixed rule.
    pub fn register(&mut self, rule: Box<dyn FixedRule>) {
        self.rules.insert(rule.name().to_owned(), rule);
    }

    /// Get a registered rule by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&dyn FixedRule> {
        self.rules.get(name).map(|b| b.as_ref())
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Extract a required option value.
pub(crate) fn require_option<'a>(
    options: &'a BTreeMap<String, Value>,
    name: &str,
    algorithm: &str,
) -> Result<&'a Value> {
    options.get(name).ok_or_else(|| {
        AlgorithmSnafu {
            algorithm: algorithm.to_owned(),
            message: format!("missing required option: {name}"),
        }
        .build()
    })
}

/// Extract an f64 option with default.
pub(crate) fn f64_option(options: &BTreeMap<String, Value>, name: &str, default: f64) -> f64 {
    options
        .get(name)
        .and_then(Value::to_f64)
        .unwrap_or(default)
}

/// Extract an i64 option with default.
pub(crate) fn i64_option(options: &BTreeMap<String, Value>, name: &str, default: i64) -> i64 {
    options
        .get(name)
        .and_then(Value::as_int)
        .unwrap_or(default)
}

/// Extract a string option.
pub(crate) fn string_option(options: &BTreeMap<String, Value>, name: &str) -> Option<Arc<str>> {
    options.get(name).and_then(Value::as_str).map(Arc::from)
}

/// Build a Rows result from headers and data.
pub(crate) fn build_rows(headers: Vec<String>, rows: Vec<Vec<Value>>) -> Rows {
    Rows { headers, rows }
}

/// Collect unique nodes from edge list.
pub(crate) fn collect_nodes(edges: &[(Value, Value, f64)]) -> Vec<Value> {
    let mut nodes: Vec<Value> = edges
        .iter()
        .flat_map(|(s, t, _)| [s.clone(), t.clone()])
        .collect();
    nodes.sort();
    nodes.dedup();
    nodes
}

/// Build adjacency list from edge list.
pub(crate) fn build_adjacency(
    edges: &[(Value, Value, f64)],
) -> FxHashMap<Value, Vec<(Value, f64)>> {
    let mut adj = FxHashMap::default();
    for (s, t, w) in edges {
        adj.entry(s.clone()).or_insert_with(Vec::new).push((t.clone(), *w));
    }
    adj
}

/// Build undirected adjacency list (adds reverse edges).
pub(crate) fn build_undirected_adjacency(
    edges: &[(Value, Value, f64)],
) -> FxHashMap<Value, Vec<(Value, f64)>> {
    let mut adj = FxHashMap::default();
    for (s, t, w) in edges {
        adj.entry(s.clone()).or_insert_with(Vec::new).push((t.clone(), *w));
        adj.entry(t.clone()).or_insert_with(Vec::new).push((s.clone(), *w));
    }
    adj
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn registry_new_empty() {
        let registry = FixedRuleRegistry::new();
        assert!(registry.get("pagerank").is_none());
    }

    #[test]
    fn registry_with_defaults() {
        let registry = FixedRuleRegistry::with_defaults();
        assert!(registry.get("pagerank").is_some());
        assert!(registry.get("louvain").is_some());
        assert!(registry.get("bfs").is_some());
        assert!(registry.get("dijkstra").is_some());
        assert!(registry.get("degree").is_some());
        assert!(registry.get("dfs").is_some());
        assert!(registry.get("prim").is_some());
        assert!(registry.get("connected_components").is_some());
        assert!(registry.get("kcore").is_some());
    }

    #[test]
    fn registry_register_custom() {
        struct TestRule;
        impl FixedRule for TestRule {
            fn name(&self) -> &str {
                "test_rule"
            }
            fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
                Ok(1)
            }
            fn run(
                &self,
                _edges: &[(Value, Value, f64)],
                _options: &BTreeMap<String, Value>,
            ) -> Result<Rows> {
                Ok(Rows::empty(vec!["col".to_owned()]))
            }
        }

        let mut registry = FixedRuleRegistry::new();
        registry.register(Box::new(TestRule));
        assert!(registry.get("test_rule").is_some());
    }

    #[test]
    fn collect_nodes_basic() {
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
        ];
        let nodes = collect_nodes(&edges);
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn build_adjacency_basic() {
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("a"), Value::from("c"), 2.0),
        ];
        let adj = build_adjacency(&edges);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[&Value::from("a")].len(), 2);
    }
}
