#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

//! In-tree tests for `crate::knowledge`.
//!
//! Split from the monolithic `knowledge_test.rs` (2196 lines) to satisfy
//! `RUST/file-too-long`. Each submodule is a coherent subject.

use super::*;

mod entities_facts;
mod ids;
mod path_validation;
mod scopes;
mod validate_memory_path;
mod verification;

pub(super) fn test_timestamp(s: &str) -> jiff::Timestamp {
    parse_timestamp(s).expect("valid test timestamp")
}
