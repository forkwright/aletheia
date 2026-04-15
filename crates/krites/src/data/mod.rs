//! Core data types for the Datalog engine.
//!
//! This module defines the value representation ([`value::DataValue`]),
//! expression evaluation ([`expr`]), scalar functions ([`functions`]),
//! aggregation operators ([`aggr`]), relation metadata ([`relation`]),
//! binary key encoding ([`memcmp`]), and the Datalog program AST ([`program`]).
#![expect(clippy::wildcard_imports, reason = "error selectors and re-exports used pervasively across data module")]
pub(crate) mod aggr;
pub(crate) mod error;
pub(crate) mod expr;
pub(crate) mod functions;

pub(crate) mod json;
pub(crate) mod memcmp;
pub(crate) mod program;
pub(crate) mod relation;
pub(crate) mod symb;
pub(crate) mod tuple;
pub(crate) mod value;

#[cfg(test)]
mod tests;
