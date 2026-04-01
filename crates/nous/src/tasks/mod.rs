//! Task registry with progress streaming and garbage collection.
//!
//! Provides a concurrent task registry for tracking background work across the
//! nous actor system. Each task has a typed identity, status lifecycle, progress
//! broadcast channel, disk-backed output, and cooperative cancellation via
//! `CancellationToken`.

mod gc;
mod output;
mod registry;
mod types;

pub use gc::spawn_gc_task;
pub use output::{OutputError, OutputReader, OutputWriter};
pub use registry::{RegistryError, TaskRegistry, TaskSnapshot};
pub use types::{ProgressEvent, TaskEntry, TaskId, TaskStatus, TaskType, ToolCallSummary};
