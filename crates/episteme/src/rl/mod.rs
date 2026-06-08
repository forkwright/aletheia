//! Reinforcement-learning readiness types for memory policy experiments.
//!
//! The benchmark reward surface is wired and evaluated against real benchmark
//! outcomes, but the learned policy / training loop remains a Phase-06b
//! boundary.

/// Memory policy action space placeholders.
pub mod actions;
/// Reward functions and benchmark outcome loading.
pub mod reward;
/// Memory policy state representation.
pub mod state;

pub use actions::Action;
pub use reward::{LongMemEvalReward, MemoryOutcome, RewardFn};
pub use state::{MemoryState, MemoryTransition};
