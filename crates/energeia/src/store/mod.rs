//! State persistence layer for energeia using fjall (LSM-tree key-value store).
//!
//! Replaces phronesis's SQLite `StateDb` with byte-prefixed key-value patterns
//! that fit aletheia's storage architecture. All records are serialized via
//! MessagePack for compact binary storage.
//!
//! # Module layout
//!
//! - [`records`] — record types (`DispatchRecord`, `SessionRecord`, etc.)
//! - [`schema`] — key encoding/decoding functions
//! - [`queries`] — prefix scan implementations for list/query operations

pub mod records;
pub(crate) mod schema;

#[cfg(feature = "storage-fjall")]
pub(crate) mod queries;

#[cfg(feature = "storage-fjall")]
mod fjall_store;

#[cfg(feature = "storage-fjall")]
pub use fjall_store::EnergeiaStore;
#[cfg(feature = "storage-fjall")]
pub(crate) use fjall_store::{
    SCAN_LIMIT_CI_VALIDATIONS, SCAN_LIMIT_DISPATCHES, SCAN_LIMIT_SESSIONS,
};
