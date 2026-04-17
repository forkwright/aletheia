//! Datalog query planning, optimization, and evaluation.
//!
//! # Pipeline
//!
//! 1. **Logical plan** (`logical`): input atoms are normalized to disjunctive
//!    normal form (DNF) via De Morgan's laws and negation normal form.
//! 2. **Reorder** (`reorder`): atoms within each rule body are reordered so
//!    that bindings flow left-to-right (positive atoms before negated ones).
//! 3. **Stratification** (`stratify`): the program is split into strata via
//!    Tarjan SCC + generalized Kahn's algorithm. Negation and aggregation
//!    edges create stratum boundaries.
//! 4. **Magic sets** (`magic`): the Supplementary Magic Sets rewrite restricts
//!    rule evaluation to only the tuples relevant to the query entry point.
//! 5. **Compilation** (`compile`): magic-rewritten rules are compiled to a
//!    physical plan tree of relational algebra operators (`ra/`).
//! 6. **Evaluation** (`eval`): semi-naive fixpoint iteration evaluates each
//!    stratum, processing only delta (new) tuples each epoch until no new
//!    facts are derived.
//!
//! # Error handling
//!
//! All errors propagate through [`QueryError`](error::QueryError) with
//! `snafu::Location` tracking. The error type covers compilation, evaluation,
//! stratification, graph traversal, type mismatches, and access control.
#![allow(
    clippy::wildcard_imports,
    reason = "snafu error selectors are imported via glob across query submodules — scoped to engine internals; expectation cannot be expressed because the lint fires only on the lib build, not lib-test"
)]
pub(crate) mod compile;
pub(crate) mod error;
pub(crate) mod eval;
pub(crate) mod graph;
pub(crate) mod logical;
pub(crate) mod magic;
pub(crate) mod ra;
pub(crate) mod reorder;
pub(crate) mod sort;
pub(crate) mod stored;
pub(crate) mod stratify;
