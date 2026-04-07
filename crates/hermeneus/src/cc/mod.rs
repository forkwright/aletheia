//! Claude Code subprocess LLM provider.
//!
//! Delegates LLM calls to the `claude` CLI binary, which handles
//! authentication (OAuth + attestation) correctly. This bypasses
//! attestation-based API blocking that prevents direct OAuth calls
//! from non-CC clients.
//!
//! Gated behind the `cc-provider` feature flag.

mod parse;
mod process;
mod provider;

pub use provider::{CcProvider, CcProviderConfig};
