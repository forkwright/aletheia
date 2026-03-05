//! aletheia-hermeneus — LLM provider abstraction
//!
//! Hermeneus (Ἑρμηνεύς) — "interpreter." Provides a provider-agnostic interface
//! for LLM interaction. Anthropic as the primary provider, with the trait
//! designed for future OpenAI/Ollama backends.
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
/// Provider-agnostic types for LLM requests and responses ([`CompletionRequest`](types::CompletionRequest), [`CompletionResponse`](types::CompletionResponse)).
pub mod types;
