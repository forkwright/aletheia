//! aletheia-hermeneus — LLM provider abstraction
//!
//! Hermeneus (Ἑρμηνεύς) — "interpreter." Provides a provider-agnostic interface
//! for LLM interaction. [`anthropic::AnthropicProvider`] is the primary implementation,
//! with the [`provider::LlmProvider`] trait designed for future OpenAI/Ollama backends.
//!
//! ## Key types
//!
//! - [`provider::LlmProvider`] — the core trait all providers implement
//! - [`types::CompletionRequest`] / [`types::CompletionResponse`] — request/response pair
//! - [`types::Content`] / [`types::ContentBlock`] — message content (text, tool use, thinking)
//! - [`types::Role`] / [`types::StopReason`] — enum types with wire-format string conversion
//!
//! Depends only on `aletheia-koina`.

pub mod anthropic;
pub mod error;
pub mod provider;
pub mod types;
