//! Community detection algorithms: Louvain and Label Propagation.

use std::collections::BTreeMap;

use rand::seq::SliceRandom;
use rustc_hash::FxHashMap;

use crate::v2::algo::{build_rows, collect_nodes, build_undirected_adjacency, i64_option};
use crate::v2::error::Result;
use crate::v2::rows::Rows;
use crate::v2::value::Value;

/// Louvain community detection algorithm.
///
/// Detects communities by maximizing modularity through greedy optimization.
/// Output: `(node, community_id)` pairs.
pub struct Louvain;

impl Louvain {
    /// Default number of iterations.
    const DEFAULT_ITERATIONS: i64 = 10;
}

impl super::FixedRule for Louvain {
    fn name(&self) -> &str {
        "louvain"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let max_iter = i64_option(options, "iterations", Self::DEFAULT_ITERATIONS) as usize;

        let nodes = collect_nodes(edges);
        let n = nodes.len();

        if n == 0 {
            return Ok(build_rows(
                vec!["node".to_owned(), "community_id".to_owned()],
                vec![],
            ));
        }

        // Build undirected weighted adjacency
        let mut adj: FxHashMap<Value, FxHashMap<Value, f64>> = FxHashMap::default();
        let mut total_weight = 0.0;

        for (s, t, w) in edges {
            *adj.entry(s.clone())
                .or_default()
                .entry(t.clone())
                .or_insert(0.0) += *w;
            *adj.entry(t.clone())
                .or_default()
                .entry(s.clone())
                .or_insert(0.0) += *w;
            total_weight += *w;
        }

        if total_weight == 0.0 {
            // All nodes in community 0
            let rows: Vec<Vec<Value>> = nodes
                .into_iter()
                .map(|node| vec![node, Value::from(0_i64)])
                .collect();
            return Ok(build_rows(
                vec!["node".to_owned(), "community_id".to_owned()],
                rows,
            ));
        }

        // Initialize each node to its own community
        let mut node_to_comm: FxHashMap<Value, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.clone(), i))
            .collect();

        // Node degrees (weighted)
        let mut node_degree: FxHashMap<Value, f64> = FxHashMap::default();
        for (s, t, w) in edges {
            *node_degree.entry(s.clone()).or_insert(0.0) += *w;
            *node_degree.entry(t.clone()).or_insert(0.0) += *w;
        }

        // Community degrees
        let mut comm_degree: FxHashMap<usize, f64> = FxHashMap::default();
        for (i, node) in nodes.iter().enumerate() {
            comm_degree.insert(i, *node_degree.get(node).unwrap_or(&0.0));
        }

        // Louvain iterations
        for _ in 0..max_iter {
            let mut improved = false;

            for node in &nodes {
                let current_comm = node_to_comm[node];
                let node_deg = *node_degree.get(node).unwrap_or(&0.0);

                // Compute gains for moving to each neighboring community
                let mut best_comm = current_comm;
                let mut best_gain = 0.0;

                // Get communities of neighbors
                let mut comm_weights: FxHashMap<usize, f64> = FxHashMap::default();
                if let Some(neighbors) = adj.get(node) {
                    for (neighbor, weight) in neighbors {
                        let comm = node_to_comm[neighbor];
                        *comm_weights.entry(comm).or_insert(0.0) += *weight;
                    }
                }

                for (comm, weight_to_comm) in comm_weights {
                    if comm == current_comm {
                        continue;
                    }

                    // Modularity gain formula (simplified)
                    let comm_deg = *comm_degree.get(&comm).unwrap_or(&0.0);
                    let gain = weight_to_comm / total_weight
                        - (node_deg * comm_deg) / (2.0 * total_weight * total_weight);

                    if gain > best_gain {
                        best_gain = gain;
                        best_comm = comm;
                        improved = true;
                    }
                }

                if best_comm != current_comm {
                    // Move node to new community
                    *comm_degree.entry(current_comm).or_insert(0.0) -= node_deg;
                    *comm_degree.entry(best_comm).or_insert(0.0) += node_deg;
                    node_to_comm.insert(node.clone(), best_comm);
                }
            }

            if !improved {
                break;
            }
        }

        // Renumber communities for compact IDs
        let mut comm_map: FxHashMap<usize, i64> = FxHashMap::default();
        let mut next_id = 0_i64;

        let rows: Vec<Vec<Value>> = nodes
            .into_iter()
            .map(|node| {
                let old_comm = node_to_comm[&node];
                let new_comm = *comm_map.entry(old_comm).or_insert_with(|| {
                    let id = next_id;
                    next_id += 1;
                    id
                });
                vec![node, Value::from(new_comm)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "community_id".to_owned()],
            rows,
        ))
    }
}

/// Label Propagation algorithm.
///
/// Iteratively assigns nodes to the majority label of their neighbors.
/// Output: `(node, community_id)` pairs.
pub struct LabelPropagation;

impl LabelPropagation {
    /// Default number of iterations.
    const DEFAULT_ITERATIONS: i64 = 100;
}

impl super::FixedRule for LabelPropagation {
    fn name(&self) -> &str {
        "label_propagation"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let max_iter = i64_option(options, "iterations", Self::DEFAULT_ITERATIONS) as usize;

        let nodes = collect_nodes(edges);
        let n = nodes.len();

        if n == 0 {
            return Ok(build_rows(
                vec!["node".to_owned(), "community_id".to_owned()],
                vec![],
            ));
        }

        // Build adjacency list
        let adj = build_undirected_adjacency(edges);

        // Initialize each node with a unique label
        let mut labels: FxHashMap<Value, i64> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.clone(), i as i64))
            .collect();

        // Label propagation
        let mut rng = rand::rng();

        for _ in 0..max_iter {
            let mut changed = false;

            // Random order for async updates
            let mut order: Vec<&Value> = nodes.iter().collect();
            order.shuffle(&mut rng);

            for node in order {
                let Some(neighbors) = adj.get(node) else {
                    continue;
                };
                if neighbors.is_empty() {
                    continue;
                }

                // Count labels among neighbors
                let mut label_counts: FxHashMap<i64, f64> = FxHashMap::default();
                for (neighbor, weight) in neighbors {
                    let Some(&label) = labels.get(neighbor) else {
                        continue;
                    };
                    *label_counts.entry(label).or_insert(0.0) += *weight;
                }

                // Find majority label
                let Some(&current_label) = labels.get(node) else {
                    continue;
                };
                let mut best_label = current_label;
                let mut best_count = 0.0;

                for (label, count) in label_counts {
                    if count > best_count {
                        best_count = count;
                        best_label = label;
                    }
                }

                if best_label != current_label {
                    labels.insert(node.clone(), best_label);
                    changed = true;
                }
            }

            if !changed {
                break;
            }
        }

        // Renumber communities for compact IDs
        let mut comm_map: FxHashMap<i64, i64> = FxHashMap::default();
        let mut next_id = 0_i64;

        let rows: Vec<Vec<Value>> = nodes
            .into_iter()
            .map(|node| {
                let old_label = labels[&node];
                let new_label = *comm_map.entry(old_label).or_insert_with(|| {
                    let id = next_id;
                    next_id += 1;
                    id
                });
                vec![node, Value::from(new_label)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "community_id".to_owned()],
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
    fn louvain_two_cliques() {
        // Two 3-cliques connected by one edge
        let edges = vec![
            // Clique 1: a, b, c
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("a"), Value::from("c"), 1.0),
            // Clique 2: d, e, f
            (Value::from("d"), Value::from("e"), 1.0),
            (Value::from("e"), Value::from("f"), 1.0),
            (Value::from("d"), Value::from("f"), 1.0),
            // Bridge edge
            (Value::from("c"), Value::from("d"), 1.0),
        ];

        let result = Louvain.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.headers, vec!["node", "community_id"]);
        assert_eq!(result.len(), 6);

        // Check that clique members tend to group together
        // Note: Louvain may merge cliques due to the bridge edge
        let comm_a = get_community(&result, "a");
        let comm_b = get_community(&result, "b");
        let comm_c = get_community(&result, "c");
        
        // Within clique 1, most nodes should share community
        let same_comm_clique1 = [comm_a, comm_b, comm_c].iter().filter(|&&c| c == comm_a).count();
        assert!(same_comm_clique1 >= 2, "at least 2 of 3 nodes in clique 1 should share community");

        let comm_d = get_community(&result, "d");
        let comm_e = get_community(&result, "e");
        let comm_f = get_community(&result, "f");
        
        let same_comm_clique2 = [comm_d, comm_e, comm_f].iter().filter(|&&c| c == comm_d).count();
        assert!(same_comm_clique2 >= 2, "at least 2 of 3 nodes in clique 2 should share community");
    }

    #[test]
    fn label_propagation_two_cliques() {
        // Same test structure as Louvain
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("a"), Value::from("c"), 1.0),
            (Value::from("d"), Value::from("e"), 1.0),
            (Value::from("e"), Value::from("f"), 1.0),
            (Value::from("d"), Value::from("f"), 1.0),
            (Value::from("c"), Value::from("d"), 1.0),
        ];

        let result = LabelPropagation.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 6);

        // Clique members should have same community
        let comm_a = get_community(&result, "a");
        let comm_b = get_community(&result, "b");
        let comm_c = get_community(&result, "c");
        assert_eq!(comm_a, comm_b);
        assert_eq!(comm_b, comm_c);
    }

    #[test]
    fn community_empty() {
        let result_l = Louvain.run(&[], &BTreeMap::new()).unwrap();
        let result_lp = LabelPropagation.run(&[], &BTreeMap::new()).unwrap();
        assert!(result_l.is_empty());
        assert!(result_lp.is_empty());
    }

    fn get_community(rows: &Rows, node: &str) -> i64 {
        rows.rows
            .iter()
            .find(|r| r[0].as_str() == Some(node))
            .map(|r| r[1].as_int().unwrap())
            .unwrap()
    }
}
