//! Datalog query parsing.

mod atoms;
mod fixed_rules;
mod program;

pub(crate) use program::parse_query;
