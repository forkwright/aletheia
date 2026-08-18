// krites-exhibit-a: begin (generated -- scripts/measure-krites-provenance.py)
// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.
// krites-exhibit-a: end

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
