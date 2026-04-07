# Task: Implement graph algorithms for krites v2

## Context

Building the krites v2 clean-room Datalog engine. The core is complete (value, rows, storage, schema, parser, evaluator). This task implements the 24 graph algorithms invoked via the `<~` fixed rule syntax.

## Standards

Read the AGENTS.md file in the repo root. Skip the Setup section.

## What to Build

### 1. FixedRule trait (`crates/krites/src/v2/algo/mod.rs`)

```rust
use std::collections::BTreeMap;
use crate::v2::value::Value;
use crate::v2::rows::Rows;
use crate::v2::error::Result;

pub trait FixedRule: Send + Sync {
    fn name(&self) -> &str;
    fn arity(&self, options: &BTreeMap<String, Value>) -> Result<usize>;
    fn run(&self, edges: &[(Value, Value, f64)], options: &BTreeMap<String, Value>) -> Result<Rows>;
}

pub struct FixedRuleRegistry { /* HashMap<String, Box<dyn FixedRule>> */ }

impl FixedRuleRegistry {
    pub fn new() -> Self;
    pub fn with_defaults() -> Self;  // registers all 24 algorithms
    pub fn register(&mut self, rule: Box<dyn FixedRule>);
    pub fn get(&self, name: &str) -> Option<&dyn FixedRule>;
}
```

### 2. Algorithms (one file per category)

All algorithms take edges as `(source, target, weight)` triples and return `Rows`.

**`algo/pagerank.rs`** — PageRank
- Input: edges + damping (default 0.85) + iterations (default 20) + epsilon (default 1e-6)
- Output: `(node, rank)` pairs

**`algo/community.rs`** — Louvain community detection, LabelPropagation
- Louvain: modularity-based community assignment
- LabelProp: iterative neighbor-majority label assignment
- Output: `(node, community_id)`

**`algo/path.rs`** — BFS shortest path, Dijkstra, A*, K-shortest (Yen)
- BFS: unweighted shortest path
- Dijkstra: weighted shortest path
- A*: heuristic-guided (use Dijkstra fallback if no heuristic)
- Yen: K shortest paths
- Output: `(node, ...)` path or `(path_id, node, step)` for K-shortest

**`algo/centrality.rs`** — Degree, Closeness, Betweenness
- Degree: count of edges per node
- Closeness: inverse sum of shortest distances
- Betweenness: fraction of shortest paths through node
- Output: `(node, centrality_score)`

**`algo/traversal.rs`** — DFS, BFS, RandomWalk
- DFS/BFS: output visited node order from start
- RandomWalk: random neighbor selection for N steps
- Output: `(step, node)` or `(node, depth)`

**`algo/spanning.rs`** — Prim MST, Kruskal MSF
- Output: `(source, target, weight)` edges in the tree

**`algo/connectivity.rs`** — ConnectedComponents, StronglyConnectedComponents
- Output: `(node, component_id)`

**`algo/clustering.rs`** — KCore, ClusteringCoefficients, TopSort
- KCore: k-core decomposition
- ClusteringCoeff: local clustering coefficient per node
- TopSort: topological ordering (DAG)
- Output varies per algorithm

### 3. Tests

At minimum test each algorithm category with a small graph:
- PageRank on a 3-node cycle
- BFS shortest path on a 5-node chain
- Connected components on a disconnected graph
- Community detection on two cliques connected by one edge
- Degree centrality on a star graph

## Constraints

- Code goes in `crates/krites/src/v2/algo/`
- Register `algo` module in `crates/krites/src/v2/mod.rs`
- No external graph library dependencies — implement from standard algorithms
- Each algorithm is a struct implementing `FixedRule`
- Feature-gated: `krites-v2`

## Validation Gate

```bash
cargo check -p aletheia-krites --features krites-v2
cargo test -p aletheia-krites --features krites-v2 -- v2::algo
```

## Completion

1. `git add -A`
2. `git commit -m "feat(krites): v2 graph algorithms — 24 FixedRule implementations"`
3. `git push origin feat/krites-v2-algorithms`
4. `gh pr create --title "feat(krites): v2 graph algorithms (24 FixedRule implementations)" --body "PageRank, Louvain, BFS/Dijkstra/A*/Yen, degree/closeness/betweenness centrality, DFS/BFS/RandomWalk, Prim/Kruskal, connected/strongly-connected components, KCore, clustering coefficients, TopSort, LabelPropagation. Part of #2284"`
