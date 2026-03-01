# Upstream Review

Reviewed sources absorbed into Aletheia workspace crates.

## CozoDB (mneme-engine)

| Field | Value |
|-------|-------|
| Repository | https://github.com/cozodb/cozo |
| Version absorbed | 0.7.6 (cozo-core) |
| Last upstream commit | 2024-12-04 |
| Upstream status | Inactive (no commits since Dec 2024) |
| License | MPL-2.0 |
| Absorbed into | `crates/mneme-engine/` |

### Relevant Issues

- **#298** — rayon 1.11 breaks graph_builder compilation. Pinned rayon to =1.10.0.
- **#287** — env_logger in non-dev dependencies. Moved to dev-dependencies in absorption.

### Relevant PRs

No unmerged PRs contain fixes needed for absorption.

### Branches

No feature branches reviewed — main branch at tag v0.7.6 is the absorption baseline.

### Disposition

Upstream is inactive. No divergence risk. Future CozoDB development (if any) would need manual review for cherry-pick into mneme-engine.

## graph_builder (graph-builder)

| Field | Value |
|-------|-------|
| Repository | https://github.com/neo4j-labs/graph |
| Version absorbed | 0.4.1 (graph_builder sub-crate) |
| Upstream status | Inactive |
| License | MIT |
| Absorbed into | `crates/graph-builder/` |

### Relevant Issues

- **graph#138** — rayon 1.11 type mismatch in EdgeList::edges(). Pinned rayon to =1.10.0.

### Disposition

Upstream inactive. graph_builder 0.4.1 is the final version used by CozoDB. The `graph` facade crate (0.3.1) remains a crates.io dependency for PageRank.
