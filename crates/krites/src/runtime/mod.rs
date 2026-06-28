//! Runtime execution layer for the Datalog engine.
//!
//! ## Module layout
//!
//! - [`db`]: Core database instance, session management, `NamedRows` result type
//! - [`transact`]: Transaction lifecycle — session creation, storage init, commit
//! - [`exec`]: Query compilation, execution, and result assembly
//! - [`imperative`]: Imperative script execution (loops, branches, control flow)
//! - [`relation`]: Stored relation handles, CRUD, index management
//! - [`hnsw`]: HNSW approximate nearest-neighbor vector index
//! - [`minhash_lsh`]: `MinHash` locality-sensitive hashing index
//! - [`sys`]: System operations (explain, compact, list, rename, triggers)
//! - [`temp_store`]: In-memory tuple store for intermediate query evaluation
//! - [`callback`]: Event callback registry for relation mutation notifications
//! - [`error`]: `RuntimeError` enum with snafu context variants
#[expect(
    clippy::redundant_closure_for_method_calls,
    reason = "engine callback wiring — pedantic style lints"
)]
pub(crate) mod callback;
#[expect(
    dead_code,
    private_interfaces,
    clippy::default_trait_access,
    clippy::inline_always,
    clippy::redundant_closure_for_method_calls,
    clippy::result_large_err,
    clippy::struct_field_names,
    clippy::type_complexity,
    clippy::unnecessary_wraps,
    reason = "engine database core — internal API surface, dead code for optional features"
)]
pub(crate) mod db;
pub(crate) mod error;
#[expect(
    dead_code,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::explicit_iter_loop,
    clippy::if_not_else,
    clippy::indexing_slicing,
    clippy::items_after_statements,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::result_large_err,
    clippy::semicolon_if_nothing_returned,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::unnecessary_semicolon,
    clippy::unnecessary_wraps,
    clippy::unused_self,
    reason = "engine query executor — complex control flow with bounds-checked indexing"
)]
pub(crate) mod exec;
#[expect(
    clippy::as_conversions,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::indexing_slicing,
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::mutable_key_type,
    clippy::range_plus_one,
    clippy::redundant_closure_for_method_calls,
    clippy::ref_option,
    clippy::result_large_err,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unnecessary_wraps,
    reason = "HNSW vector index — unsafe geometry, bounds-checked indexing, numeric casts for index math"
)]
pub(crate) mod hnsw;
#[expect(
    clippy::default_trait_access,
    clippy::explicit_iter_loop,
    clippy::needless_continue,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_else,
    clippy::ref_option,
    clippy::result_large_err,
    clippy::semicolon_if_nothing_returned,
    clippy::too_many_arguments,
    reason = "imperative execution — complex control flow with early returns"
)]
pub(crate) mod imperative;
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::explicit_iter_loop,
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    clippy::mutable_key_type,
    clippy::range_plus_one,
    clippy::redundant_closure_for_method_calls,
    clippy::ref_option,
    clippy::result_large_err,
    clippy::too_many_arguments,
    clippy::uninlined_format_args,
    reason = "MinHash LSH — numeric casts and indexing for hash computation"
)]
pub(crate) mod minhash_lsh;
pub(crate) mod poison;
#[expect(
    clippy::as_conversions,
    clippy::assigning_clones,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::explicit_iter_loop,
    clippy::if_not_else,
    clippy::indexing_slicing,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::result_large_err,
    clippy::semicolon_if_nothing_returned,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::unnecessary_semicolon,
    reason = "relation system — storage key encoding, bounds-checked tuple indexing"
)]
pub(crate) mod relation;
#[expect(
    clippy::as_conversions,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::result_large_err,
    clippy::semicolon_if_nothing_returned,
    clippy::too_many_lines,
    clippy::unnecessary_wraps,
    clippy::unused_self,
    reason = "system relation handlers — internal dispatch, numeric casts for metadata"
)]
pub(crate) mod sys;
#[expect(
    clippy::as_conversions,
    clippy::default_trait_access,
    clippy::explicit_iter_loop,
    clippy::implicit_clone,
    clippy::indexing_slicing,
    clippy::needless_pass_by_value,
    clippy::result_large_err,
    clippy::single_match_else,
    reason = "temp store — bounds-checked indexing for intermediate tuple storage"
)]
pub(crate) mod temp_store;
#[cfg(test)]
mod tests;
#[expect(
    clippy::as_conversions,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::ignored_unit_patterns,
    clippy::indexing_slicing,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_else,
    clippy::result_large_err,
    clippy::semicolon_if_nothing_returned,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    reason = "transaction layer — storage key encoding, bounds-checked indexing"
)]
pub(crate) mod transact;
