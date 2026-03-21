//! Runtime execution layer for the Datalog engine.
pub(crate) mod callback;
pub(crate) mod db;
pub(crate) mod error;
pub(crate) mod exec;
pub(crate) mod hnsw;
pub(crate) mod imperative;
pub(crate) mod minhash_lsh;
pub(crate) mod relation;
pub(crate) mod sys;
pub(crate) mod temp_store;
#[cfg(test)]
mod tests;
pub(crate) mod transact;
