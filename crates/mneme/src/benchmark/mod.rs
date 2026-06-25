//! Benchmark support for memory evaluation.
//!
//! This module exposes shared primitives so that `dokimion`, the `aletheia`
//! CLI, and future in-process harnesses all speak the same isolation and
//! evidence vocabulary.

pub mod error;
pub mod isolation;
