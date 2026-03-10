//! Core data types for the Datalog engine.
pub(crate) mod aggr;
pub(crate) mod error;
pub(crate) mod expr;
pub(crate) mod functions;

pub(crate) use error::{DataError, DataResult};
pub(crate) mod json;
pub(crate) mod memcmp;
pub(crate) mod program;
pub(crate) mod relation;
pub(crate) mod symb;
pub(crate) mod tuple;
pub(crate) mod value;

#[cfg(test)]
mod tests;
