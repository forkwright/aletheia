//! Session store.
//!
//! Pure-Rust LSM-tree storage via the `fjall` crate.
//!
//! Code that depends on `graphe` should import from this module and never
//! reach into backend-specific sub-modules.

mod fjall_store;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use fjall_store::{
    FinalizeMessage, FinalizeNote, FinalizeToolAuditRecord, FinalizeTurnRequest,
    FinalizeTurnResult, SessionStatusCounts, SessionStore,
};
