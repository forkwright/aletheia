#![deny(missing_docs)] // kanon:ignore TESTING/no-tests
//! aletheia-hermeneus: Anthropic-native LLM provider
//!
//! Hermeneus (Ἑρμηνεύς): "interpreter." Anthropic-native types and client
//! for LLM interaction. Other providers implement adapters that map to/from
//! the Anthropic type system.
//!
//! Depends only on `aletheia-koina`.

/// Anthropic Messages API client with streaming, retries, and cost estimation.
pub mod anthropic;
/// Claude Code subprocess provider: delegates LLM calls to the `claude` CLI.
///
/// Bypasses attestation-based API blocking by routing through CC's own
/// authentication. Gated behind the `cc-provider` feature flag.
#[cfg(feature = "cc-provider")]
pub mod cc;
/// Circuit breaker (Closed / Open / HalfOpen) with exponential backoff for LLM provider health.
pub mod circuit_breaker;
/// Codex subprocess provider: delegates LLM calls to the `codex` CLI.
///
/// Uses Codex CLI OAuth credentials and is gated behind the `codex-provider`
/// feature flag.
#[cfg(feature = "codex-provider")]
pub mod codex;
/// Complexity-based model routing: scores queries and routes to Haiku/Sonnet/Opus.
pub mod complexity;
/// Adaptive concurrency limiter (AIMD) for LLM calls based on response latency.
pub mod concurrency;
/// Hermeneus-specific error types for provider, API, and authentication failures.
pub mod error;
/// Model fallback chain: retries alternative models on transient failures.
pub mod fallback;
/// Provider health state machine (Up / Degraded / Down) with automatic recovery.
pub mod health;
/// Kimi subprocess provider: delegates LLM calls to the `kimi` CLI.
///
/// Uses the CLI's local OAuth credential store. Gated behind the
/// `kimi-provider` feature flag.
#[cfg(feature = "kimi-provider")]
pub mod kimi;
/// Doom-loop detection for multi-tool MCP dispatch (Phase 7).
pub mod loop_detector;
/// Prometheus metrics for LLM request counts, latency, and token usage.
pub mod metrics;
/// Model constants and API configuration defaults.
pub mod models;
/// OpenAI Chat Completions-compatible HTTP client. Bridges aletheia to any
/// endpoint that speaks the OpenAI wire format: local llama.cpp / ollama /
/// vllm for air-gapped operation, plus OpenAI and other cloud alternatives.
pub mod openai;
/// [`LlmProvider`](provider::LlmProvider), [`ProviderConfig`](provider::ProviderConfig), and [`ProviderRegistry`](provider::ProviderRegistry).
pub mod provider;
/// Shared retry backoff helpers for provider implementations.
pub(crate) mod retry;
/// Provider retry attempts and exponential backoff policy.
pub use retry::RetryPolicy;
/// Session-scoped secret vault and credential substitution.
///
/// Provides [`SecretVault`](secret::SecretVault) for storing named secrets and
/// [`substitute_in_json`](secret::substitute_in_json) for replacing
/// `{{secret:<name>}}` / `$SECRET(<name>)` placeholders in tool arguments.
pub mod secret;
/// Shared mock provider for tests across the workspace.
#[cfg(any(test, feature = "test-utils"))]
#[expect(
    clippy::expect_used,
    reason = "test-only code, panicking on poisoned mutex is correct"
)]
pub mod test_utils;
/// Anthropic-native types for LLM requests and responses ([`CompletionRequest`](types::CompletionRequest), [`CompletionResponse`](types::CompletionResponse)).
pub mod types;

/// [`SeatBridgedProvider`](seat_bridged::SeatBridgedProvider) trait for CLI-subprocess OAuth-seat providers.
///
/// Shared contract for providers that delegate LLM calls to a local CLI binary
/// (e.g. `claude`, `codex`) which owns the OAuth credential handshake.
pub mod seat_bridged;
