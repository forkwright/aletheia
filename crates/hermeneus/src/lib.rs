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
/// Circuit breaker (Closed / Open / HalfOpen) with exponential backoff for LLM provider health.
pub mod circuit_breaker;
/// Adaptive concurrency limiter (AIMD) for LLM calls based on response latency.
pub mod concurrency;
/// Hermeneus-specific error types for provider, API, and authentication failures.
pub mod error;
/// Model fallback chain: retries alternative models on transient failures.
pub mod fallback;
/// Provider health state machine (Up / Degraded / Down) with automatic recovery.
pub mod health;
/// OpenAI-compatible local LLM provider for vLLM and similar servers.
///
/// Gated behind the `local-llm` feature flag.
#[cfg(feature = "local-llm")]
pub mod local;
/// Prometheus metrics for LLM request counts, latency, and token usage.
pub mod metrics;
/// Model constants and API configuration defaults.
pub mod models;
/// [`LlmProvider`](provider::LlmProvider), [`ProviderConfig`](provider::ProviderConfig), and [`ProviderRegistry`](provider::ProviderRegistry).
pub mod provider;
/// Shared mock provider for tests across the workspace.
#[cfg(any(test, feature = "test-utils"))]
#[expect(
    clippy::expect_used,
    reason = "test-only code, panicking on poisoned mutex is correct"
)]
pub mod test_utils;
/// Anthropic-native types for LLM requests and responses ([`CompletionRequest`](types::CompletionRequest), [`CompletionResponse`](types::CompletionResponse)).
pub mod types;
