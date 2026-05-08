//! Working-memory subsystem for agent-curated checkpoint persistence.
//!
//! Provides [`FjallWorkingCheckpointStore`], a fjall-backed implementation of
//! [`organon::types::WorkingCheckpointStore`] that agents write to via the
//! `update_working_checkpoint` tool and hooks read from for turn-start
//! injection.

mod store;

pub use store::FjallWorkingCheckpointStore;
