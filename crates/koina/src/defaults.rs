//! Shared configuration defaults referenced by taxis (config loading) and nous (runtime).
//!
//! Define once here. Never hardcode these values in another crate.

/// Default maximum output tokens per LLM response.
pub const MAX_OUTPUT_TOKENS: u32 = 16_384;

/// Default maximum tokens for bootstrap context injection.
pub const BOOTSTRAP_MAX_TOKENS: u32 = 40_000;

/// Default context window budget (tokens).
pub const CONTEXT_TOKENS: u32 = 200_000;

/// Default maximum consecutive tool use iterations per turn.
pub const MAX_TOOL_ITERATIONS: u32 = 200;

/// Default maximum bytes per tool result before truncation.
pub const MAX_TOOL_RESULT_BYTES: u32 = 32_768;

/// Default LLM call timeout in seconds.
pub const TIMEOUT_SECONDS: u32 = 300;

/// Default history budget ratio (fraction of remaining context for conversation history).
pub const HISTORY_BUDGET_RATIO: f64 = 0.6;

/// Default characters-per-token estimate for budget calculations.
pub const CHARS_PER_TOKEN: u32 = 4;

/// Maximum output bytes returned by a single tool call.
pub const MAX_OUTPUT_BYTES: usize = 50 * 1024;
