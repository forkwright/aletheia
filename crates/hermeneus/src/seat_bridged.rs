//! `SeatBridgedProvider` — shared trait for OAuth-seat subprocess providers.
//!
//! Both [`CcProvider`](crate::cc::CcProvider) and
//! [`CodexProvider`](crate::codex::CodexProvider) delegate LLM calls to a
//! local CLI binary that owns the OAuth handshake. This trait captures the
//! common contract so callers can treat them uniformly and avoid duplicating
//! subprocess lifecycle logic.

use std::path::PathBuf;
use std::time::Duration;

/// Contract for providers that bridge to an OAuth seat via a local CLI subprocess.
///
/// Implementors spawn a child process, feed it the prompt, collect output, and
/// map the result to Hermeneus types. Authentication is fully owned by the CLI;
/// the provider never touches OAuth tokens.
///
/// The trait is intentionally thin: it surfaces only the fields needed for
/// diagnostics and configuration. Actual completion calls go through the
/// [`LlmProvider`](crate::provider::LlmProvider) trait as usual.
pub trait SeatBridgedProvider: crate::provider::LlmProvider {
    /// Path to the CLI binary used for subprocess invocations.
    fn cli_binary(&self) -> &PathBuf;

    /// Maximum wall-clock time before killing the subprocess.
    fn subprocess_timeout(&self) -> Duration;

    /// Name of the CLI product (e.g. `"claude"`, `"codex"`).
    ///
    /// Used for log messages and error context.
    fn cli_product_name(&self) -> &'static str;
}
