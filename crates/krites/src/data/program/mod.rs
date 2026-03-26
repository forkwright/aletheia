//! Datalog program representation.

mod atoms;
mod fixed_rule;
mod input;
mod magic;
mod search;
mod types;

pub(crate) use atoms::*;
pub(crate) use fixed_rule::*;
pub(crate) use input::*;
pub(crate) use magic::*;
pub(crate) use search::*;
pub(crate) use types::*;
