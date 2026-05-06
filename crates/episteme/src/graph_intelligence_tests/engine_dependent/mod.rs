//! Engine-dependent integration tests for graph intelligence.
//!
//! Split from the monolithic `engine_dependent.rs` (920 lines) to satisfy
//! `RUST/file-too-long`. The `engine_tests` submodule is feature-gated on
//! `mneme-engine`; the standalone `louvain_*`, `pagerank_*`, `bfs_*`, and
//! `graph_dirty_*` tests at the bottom live in `top_level`.

#![cfg_attr(
    feature = "mneme-engine",
    expect(clippy::expect_used, reason = "test assertions")
)]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]

#[cfg(feature = "mneme-engine")]
mod engine_tests_part1;
#[cfg(feature = "mneme-engine")]
mod engine_tests_part2;
mod top_level;
