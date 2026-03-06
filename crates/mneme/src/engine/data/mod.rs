/*
 * Copyright 2022, The Cozo Project Authors.
 *
 * This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
 * If a copy of the MPL was not distributed with this file,
 * You can obtain one at https://mozilla.org/MPL/2.0/.
 */

// Vendored from CozoDB v0.7.6. Suppress all clippy lints.
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod error;
pub(crate) mod aggr;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod expr;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod functions;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod json;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod memcmp;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod program;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod relation;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod symb;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod tuple;
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
pub(crate) mod value;

#[cfg(test)]
#[allow(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
mod tests;
