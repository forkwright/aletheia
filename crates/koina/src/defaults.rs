//! Shared configuration defaults referenced by taxis (config loading) and nous (runtime).
//!
//! Define once here. Never hardcode these values in another crate.

/// Default configuration file path relative to instance root.
pub const DEFAULT_CONFIG_PATH: &str = "config/aletheia.toml";

/// Default LLM model identifier.
///
/// Single source of truth for the model every aletheia subsystem defaults to
/// when no explicit model is configured: `aletheia init` scaffold, `add-nous`
/// CLI default, runtime spawn fallback (`SONNET_MODEL`), pylon request
/// fallback, `agent_io` export fallback, melete distillation default, taxis
/// `ModelSpec` default, and theatron wizard model picker.
///
/// Defining the default in two places (formerly `DEFAULT_MODEL` and
/// `DEFAULT_MODEL_SHORT`, #4235) routed `aletheia init` to one model and
/// runtime spawn/distillation to a different one — a silent downgrade
/// invisible at config time. Keep this as the only model default constant in
/// the workspace; `crates/koina/tests/model_default_consistency.rs` walks the
/// source tree and fails loudly if a second `DEFAULT_MODEL*` constant
/// reappears.
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

/// Default nous-agent identifier created by `aletheia init -y` and assumed by
/// CLI subcommands that take `--nous-id`. Single source of truth so that
/// `init`'s scaffolded agent and `ingest`'s default flag value cannot drift
/// (#4245).
pub const DEFAULT_AGENT_ID: &str = "pronoea";

/// Default maximum output tokens per LLM response.
pub const MAX_OUTPUT_TOKENS: u32 = 16_384;

/// Default maximum tokens for bootstrap context injection.
pub const BOOTSTRAP_MAX_TOKENS: u32 = 40_000;

/// Default context window budget (tokens).
pub const CONTEXT_TOKENS: u32 = 200_000;

/// Default context window budget for Opus models (1M token context window).
pub const OPUS_CONTEXT_TOKENS: u32 = 1_000_000;

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

/// Default limit for consecutive no-progress turns before the mistake brake fires.
pub const DEFAULT_CONSECUTIVE_MISTAKE_LIMIT: u32 = 5;
