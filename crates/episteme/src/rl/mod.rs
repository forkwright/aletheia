//! Reinforcement-learning readiness types for memory policy experiments.
//!
//! This module intentionally defines only stable boundary types. The concrete
//! environment is deferred until the in-tree memory-policy constants have been
//! inventoried into a durable state schema.

/// Memory policy action space placeholders.
pub mod actions;
/// Reward functions and benchmark outcome loading.
pub mod reward;
/// Memory policy state representation.
pub mod state;

pub use actions::Action;
pub use reward::{LongMemEvalReward, MemoryOutcome, RewardFn};
pub use state::{MemoryState, MemoryTransition};
