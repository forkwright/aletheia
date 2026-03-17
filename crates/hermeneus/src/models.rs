//! Anthropic model identifiers and metadata.
//!
//! Canonical constants for model names, context window sizes, and pricing
//! rates. Other crates should import from here instead of hardcoding model
//! strings. Update these when Anthropic publishes new models or retires
//! old ones.

/// Claude Opus 4 (latest alias).
pub const CLAUDE_OPUS_4: &str = "claude-opus-4-6";

/// Claude Opus 4 (pinned 2025-05-14 snapshot).
pub const CLAUDE_OPUS_4_20250514: &str = "claude-opus-4-20250514";

/// Claude Sonnet 4 (latest alias).
pub const CLAUDE_SONNET_4: &str = "claude-sonnet-4-6";

/// Claude Sonnet 4 (pinned 2025-05-14 snapshot).
pub const CLAUDE_SONNET_4_20250514: &str = "claude-sonnet-4-20250514";

/// Claude Haiku 4.5 (latest alias).
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";

/// Claude Haiku 4.5 (pinned 2025-10-01 snapshot).
pub const CLAUDE_HAIKU_4_5_20251001: &str = "claude-haiku-4-5-20251001";

/// Context window size shared by all current Claude models (tokens).
pub const CONTEXT_WINDOW_TOKENS: u32 = 200_000;

/// Cost per million input tokens (USD) for Opus models.
pub const OPUS_INPUT_COST_PER_MTOK: f64 = 15.0;

/// Cost per million output tokens (USD) for Opus models.
pub const OPUS_OUTPUT_COST_PER_MTOK: f64 = 75.0;

/// Cost per million input tokens (USD) for Sonnet models.
pub const SONNET_INPUT_COST_PER_MTOK: f64 = 3.0;

/// Cost per million output tokens (USD) for Sonnet models.
pub const SONNET_OUTPUT_COST_PER_MTOK: f64 = 15.0;

/// Cost per million input tokens (USD) for Haiku models.
pub const HAIKU_INPUT_COST_PER_MTOK: f64 = 0.8;

/// Cost per million output tokens (USD) for Haiku models.
pub const HAIKU_OUTPUT_COST_PER_MTOK: f64 = 4.0;
