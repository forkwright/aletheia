// krites-exhibit-a: begin (generated -- scripts/measure-krites-provenance.py)
// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.
// krites-exhibit-a: end

//! Core data types for the Datalog engine.
//!
//! This module defines the value representation ([`value::DataValue`]),
//! expression evaluation ([`expr`]), scalar functions ([`functions`]),
//! aggregation operators ([`aggr`]), relation metadata ([`relation`]),
//! binary key encoding ([`memcmp`]), and the Datalog program AST ([`program`]).
#![allow(
    clippy::wildcard_imports,
    reason = "error selectors and re-exports used pervasively across data module; expectation cannot be expressed because the lint fires only on the lib build, not lib-test"
)]
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
