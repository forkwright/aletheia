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
    AppendCommandLifecycleRecord, FinalizeMessage, FinalizeNote, FinalizeToolAuditRecord,
    FinalizeTurnRequest, FinalizeTurnResult, SchemaManifest, SessionStatusCounts, SessionStore,
};
// WHY(#4414): the four portability-only types below are gated behind
// `#[cfg(feature = "portability")]` in fjall_store.rs; this re-export must
// carry the same gate or a consumer that depends on graphe with
// `default-features = false` and never opts into `portability` (episteme
// does exactly this) fails to compile this module at all -- caught by the
// new episteme gliner/nuextract feature-gate CI step, which is the first
// build in this repo's history to compile graphe with portability off.
#[cfg(feature = "portability")]
pub use fjall_store::{
    ImportCommandLifecycleRecord, ImportSessionBundle, ImportSessionBundleResult,
    ImportSessionNote, ImportSessionWorkingState,
};
