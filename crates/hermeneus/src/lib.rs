//! aletheia-hermeneus — Anthropic-native LLM provider
//!
//! Hermeneus (Ἑρμηνεύς) — "interpreter." Anthropic-native types and client
//! for LLM interaction. Other providers implement adapters that map to/from
//! the Anthropic type system.
//!
//! Depends only on `aletheia-koina`.

/// Anthropic Messages API client with streaming, retries, and cost estimation.
pub mod anthropic;
/// Hermeneus-specific error types for provider, API, and authentication failures.
pub mod error;
/// Provider health state machine (Up / Degraded / Down) with automatic recovery.
pub mod health;
pub mod metrics;
/// [`LlmProvider`](provider::LlmProvider), [`ProviderConfig`](provider::ProviderConfig), and [`ProviderRegistry`](provider::ProviderRegistry).
pub mod provider;
/// Anthropic-native types for LLM requests and responses ([`CompletionRequest`](types::CompletionRequest), [`CompletionResponse`](types::CompletionResponse)).
pub mod types;
