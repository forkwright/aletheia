//! Auth store.
//!
//! Pure-Rust LSM-tree storage via the `fjall` crate.
//!
//! Code that depends on `symbolon` should import from this module and
//! never reach into backend-specific sub-modules.

mod fjall_store;
pub(crate) use fjall_store::AuthStore;
