//! Datalog query parsing.
//!
//! Assembles rule definitions, fixed rules, constant rules, and query options
//! into an [`InputProgram`]. Split into submodules:
//!
//! - [`atoms`]: rule heads, body atoms, disjunctions, unification
//! - [`fixed_rules`]: built-in algorithm bindings and constant rule construction
//! - [`program`]: top-level query assembly and option parsing

mod atoms;
mod fixed_rules;
mod options;
mod program;

pub(crate) use program::parse_query;
