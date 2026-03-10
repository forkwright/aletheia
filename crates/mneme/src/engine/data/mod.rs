//! Core data types for the Datalog engine.
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod aggr;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod expr;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod functions;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod json;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod memcmp;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod program;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod relation;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod symb;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod tuple;
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
pub(crate) mod value;

#[cfg(test)]
#[allow(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::restriction
)]
mod tests;
