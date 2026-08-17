// krites-exhibit-a: begin (generated -- scripts/measure-krites-provenance.py)
// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.
// krites-exhibit-a: end

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
