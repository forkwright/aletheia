//! Session store.
//!
//! Pure-Rust LSM-tree storage via the `fjall` crate.
//!
//! Code that depends on `graphe` should import from this module and never
//! reach into backend-specific sub-modules.

mod fjall_store;
pub use fjall_store::{FinalizeMessage, FinalizeTurnRequest, FinalizeTurnResult, SessionStore};
