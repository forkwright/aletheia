//! aletheia-hermeneus — LLM provider abstraction
//!
//! Hermeneus (Ἑρμηνεύς) — "interpreter." Provides a provider-agnostic interface
//! for LLM interaction. Anthropic as the primary provider, with the trait
//! designed for future OpenAI/Ollama backends.
//!
//! Depends only on `aletheia-koina`.

pub mod anthropic;
pub mod error;
pub mod health;
pub mod provider;
pub mod types;
