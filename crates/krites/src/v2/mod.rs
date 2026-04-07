//! Krites v2: clean-room Datalog engine.
//!
//! Feature-gated behind `krites-v2`. When enabled, provides an alternative
//! implementation of the krites [`Db`](crate::Db) facade using a purpose-built
//! Datalog evaluator instead of vendored CozoDB.
//!
//! # Modules
//!
//! - [`value`] — runtime values with eidos-native types at boundary
//! - [`rows`] — query result container
//! - [`error`] — v2-specific error types
//! - [`eval`] — query and mutation evaluator

pub mod error;
pub mod eval;
pub mod parse;
pub mod rows;
pub mod schema;
pub mod storage;
pub mod value;
