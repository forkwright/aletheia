//! Kimi subprocess LLM provider.
//!
//! Delegates LLM calls to the `kimi` CLI binary, which handles its own
//! OAuth credentials. This avoids routing local CLI-authenticated sessions
//! through API-token based HTTP clients.
//!
//! Gated behind the `kimi-provider` feature flag.

mod parse;
mod process;
mod provider;

pub use provider::{KimiProvider, KimiProviderConfig};
