//! Connectivity algorithms: Connected Components, Strongly Connected Components.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::v2::algo::{build_adjacency, build_rows, collect_nodes};
use crate::v2::error::Result;
use crate::v2::rows::Rows;
use crate::v2::value::Value;

/// Connected Components algorithm (undirected).
///
/// Finds all connected components in an undirected graph using DFS.
/// Output: `(node, component_id)` pairs.
pub struct ConnectedComponents;

impl ConnectedComponents {
    fn dfs(
        &self,
        node: &Value,
        component_id: i64,
        adj: &FxHashMap<Value, Vec<(Value, f64)>>,
        visited: &mut FxHashMap<Value, bool>,
        component_map: &mut FxHashMap<Value, i64>,
    ) {
        visited.insert(node.clone(), true);
        component_map.insert(node.clone(), component_id);

        if let Some(neighbors) = adj.get(node) {
            for (neighbor, _) in neighbors {
                if !visited.contains_key(neighbor) {
                    self.dfs(neighbor, component_id, adj, visited, component_map);
                }
            }
        }
    }
}

impl super::FixedRule for ConnectedComponents {
    fn name(&self) -> &str {
        "connected_components"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        _options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let nodes = collect_nodes(edges);

        // Build undirected adjacency
        let mut adj: FxHashMap<Value, Vec<(Value, f64)>> = FxHashMap::default();
        for (s, t, w) in edges {
            adj.entry(s.clone()).or_insert_with(Vec::new).push((t.clone(), *w));
            adj.entry(t.clone()).or_insert_with(Vec::new).push((s.clone(), *w));
        }

        let mut visited: FxHashMap<Value, bool> = FxHashMap::default();
        let mut component_map: FxHashMap<Value, i64> = FxHashMap::default();
        let mut component_id = 0_i64;

        for node in &nodes {
            if !visited.contains_key(node) {
                self.dfs(node, component_id, &adj, &mut visited, &mut component_map);
                component_id += 1;
            }
        }

        // Build result rows
        let rows: Vec<Vec<Value>> = nodes
            .into_iter()
            .map(|node| {
                let comp = component_map.get(&node).copied().unwrap_or(-1);
                vec![node, Value::from(comp)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "component_id".to_owned()],
            rows,
        ))
    }
}

/// Strongly Connected Components algorithm (Kosaraju's algorithm).
///
/// Finds all strongly connected components in a directed graph.
/// Output: `(node, component_id)` pairs.
pub struct StronglyConnectedComponents;

impl StronglyConnectedComponents {
    fn dfs_first_pass(
        &self,
        node: &Value,
        adj: &FxHashMap<Value, Vec<(Value, f64)>>,
        visited: &mut FxHashMap<Value, bool>,
        finish_order: &mut Vec<Value>,
    ) {
        visited.insert(node.clone(), true);

        if let Some(neighbors) = adj.get(node) {
            for (neighbor, _) in neighbors {
                if !visited.contains_key(neighbor) {
                    self.dfs_first_pass(neighbor, adj, visited, finish_order);
                }
            }
        }

        finish_order.push(node.clone());
    }

    fn dfs_second_pass(
        &self,
        node: &Value,
        rev_adj: &FxHashMap<Value, Vec<(Value, f64)>>,
        visited: &mut FxHashMap<Value, bool>,
        component: &mut Vec<Value>,
    ) {
        visited.insert(node.clone(), true);
        component.push(node.clone());

        if let Some(neighbors) = rev_adj.get(node) {
            for (neighbor, _) in neighbors {
                if !visited.contains_key(neighbor) {
                    self.dfs_second_pass(neighbor, rev_adj, visited, component);
                }
            }
        }
    }
}

impl super::FixedRule for StronglyConnectedComponents {
    fn name(&self) -> &str {
        "strongly_connected_components"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        _options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let nodes = collect_nodes(edges);

        if nodes.is_empty() {
            return Ok(build_rows(
                vec!["node".to_owned(), "component_id".to_owned()],
                vec![],
            ));
        }

        // Build forward and reverse adjacency
        let adj = build_adjacency(edges);
        let mut rev_adj: FxHashMap<Value, Vec<(Value, f64)>> = FxHashMap::default();
        for (s, t, w) in edges {
            rev_adj.entry(t.clone()).or_insert_with(Vec::new).push((s.clone(), *w));
        }

        // First DFS pass to get finish order
        let mut visited: FxHashMap<Value, bool> = FxHashMap::default();
        let mut finish_order: Vec<Value> = Vec::new();

        for node in &nodes {
            if !visited.contains_key(node) {
                self.dfs_first_pass(node, &adj, &mut visited, &mut finish_order);
            }
        }

        // Second DFS pass on transposed graph in reverse finish order
        visited.clear();
        let mut component_map: FxHashMap<Value, i64> = FxHashMap::default();
        let mut component_id = 0_i64;

        for node in finish_order.iter().rev() {
            if !visited.contains_key(node) {
                let mut component: Vec<Value> = Vec::new();
                self.dfs_second_pass(node, &rev_adj, &mut visited, &mut component);

                for n in component {
                    component_map.insert(n, component_id);
                }
                component_id += 1;
            }
        }

        // Build result rows
        let rows: Vec<Vec<Value>> = nodes
            .into_iter()
            .map(|node| {
                let comp = component_map.get(&node).copied().unwrap_or(-1);
                vec![node, Value::from(comp)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "component_id".to_owned()],
            rows,
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::v2::algo::FixedRule;

    #[test]
    fn connected_components_disconnected() {
        // Two disconnected components
        let edges = vec![
            // Component 1: a-b, b-c
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            // Component 2: x-y, y-z
            (Value::from("x"), Value::from("y"), 1.0),
            (Value::from("y"), Value::from("z"), 1.0),
            // Isolated node: solo (not in edges, but will be collected)
        ];

        let result = ConnectedComponents.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.headers, vec!["node", "component_id"]);
        assert_eq!(result.len(), 6); // a,b,c,x,y,z

        // Check that nodes in same component have same ID
        let get_comp = |n: &str| {
            result
                .rows
                .iter()
                .find(|r| r[0].as_str() == Some(n))
                .map(|r| r[1].as_int().unwrap())
                .unwrap()
        };

        let comp_a = get_comp("a");
        let comp_b = get_comp("b");
        let comp_c = get_comp("c");
        assert_eq!(comp_a, comp_b);
        assert_eq!(comp_b, comp_c);

        let comp_x = get_comp("x");
        let comp_y = get_comp("y");
        let comp_z = get_comp("z");
        assert_eq!(comp_x, comp_y);
        assert_eq!(comp_y, comp_z);

        // Different components should have different IDs
        assert_ne!(comp_a, comp_x);
    }

    #[test]
    fn scc_cycle() {
        // Cycle: a -> b -> c -> a (one SCC)
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("c"), Value::from("a"), 1.0),
        ];

        let result = StronglyConnectedComponents.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 3);

        // All nodes should be in the same SCC
        let comps: std::collections::HashSet<_> = result
            .rows
            .iter()
            .map(|r| r[1].as_int().unwrap())
            .collect();
        assert_eq!(comps.len(), 1);
    }

    #[test]
    fn scc_dag() {
        // DAG with multiple SCCs (each node is its own SCC)
        // a -> b -> c
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
        ];

        let result = StronglyConnectedComponents.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 3);

        // Each node is its own SCC
        let comps: std::collections::HashSet<_> = result
            .rows
            .iter()
            .map(|r| r[1].as_int().unwrap())
            .collect();
        assert_eq!(comps.len(), 3);
    }

    #[test]
    fn scc_complex() {
        // Complex graph:
        // SCC1: a <-> b
        // SCC2: c (self-loop or just single)
        // a -> c (connection between SCCs)
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("a"), 1.0),
            (Value::from("a"), Value::from("c"), 1.0),
        ];

        let result = StronglyConnectedComponents.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 3);

        let get_comp = |n: &str| {
            result
                .rows
                .iter()
                .find(|r| r[0].as_str() == Some(n))
                .map(|r| r[1].as_int().unwrap())
                .unwrap()
        };

        // a and b should be in the same SCC
        assert_eq!(get_comp("a"), get_comp("b"));
        // c should be in a different SCC
        assert_ne!(get_comp("a"), get_comp("c"));
    }

    #[test]
    fn connectivity_empty() {
        let result_cc = ConnectedComponents.run(&[], &BTreeMap::new()).unwrap();
        let result_scc = StronglyConnectedComponents.run(&[], &BTreeMap::new()).unwrap();
        assert!(result_cc.is_empty());
        assert!(result_scc.is_empty());
    }
}
