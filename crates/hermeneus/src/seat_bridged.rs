//! `SeatBridgedProvider` — shared trait for OAuth-seat subprocess providers.
//!
//! Both [`CcProvider`](crate::cc::CcProvider) and
//! [`CodexProvider`](crate::codex::CodexProvider) delegate LLM calls to a
//! local CLI binary that owns the OAuth handshake. This trait captures the
//! common contract so callers can treat them uniformly and avoid duplicating
//! subprocess lifecycle logic. [`KimiProvider`](crate::kimi::KimiProvider) is
//! the same shape (local CLI, own OAuth handshake) but does not implement
//! this trait; [`reject_tool_bearing_request`] is a free function rather than
//! a trait method so all three share it regardless.

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

/// Reject a tool-bearing request before it reaches a seat-bridged CLI subprocess.
///
/// WHY(#4510): seat-bridged CLI subprocess providers (`cc`, `codex`, `kimi`)
/// run their own agentic loop and cannot translate Aletheia `request.tools`
/// into the Organon tool loop. Silently dropping the tool definitions and
/// continuing let tool-required turns complete with the tools invisible to
/// the model — a routing bug indistinguishable from success until a tool
/// call never arrives. There is no compatibility mode: a tool-bearing
/// request must fail before the subprocess is spawned, naming the provider
/// and the missing capability, so the caller routes to a native API
/// provider instead.
///
/// # Errors
///
/// Returns [`crate::error::Error::CapabilityMismatch`] when `tool_count > 0`.
///
/// NOTE: gated on the union of the three seat-bridged provider features. This
/// module is ungated but every caller lives behind one of them, so without
/// this the function is dead code in a default-feature build.
#[cfg(any(
    feature = "cc-provider",
    feature = "codex-provider",
    feature = "kimi-provider"
))]
pub(crate) fn reject_tool_bearing_request(
    provider_name: &str,
    cli_product_name: &str,
    tool_count: usize,
) -> crate::error::Result<()> {
    if tool_count == 0 {
        return Ok(());
    }

    crate::error::CapabilityMismatchSnafu {
        provider: provider_name.to_owned(),
        capability: crate::provider::TOOL_LOOP_CAPABILITY.to_owned(),
        message: format!(
            "{cli_product_name} runs its own agentic loop and cannot execute \
             {tool_count} aletheia-defined tool(s); route this request to a \
             native API provider (e.g. anthropic, openai) instead"
        ),
    }
    .fail()
}

#[cfg(test)]
#[cfg(any(
    feature = "cc-provider",
    feature = "codex-provider",
    feature = "kimi-provider"
))]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn passes_through_when_no_tools_are_requested() {
        assert!(reject_tool_bearing_request("cc", "claude", 0).is_ok());
    }

    #[test]
    fn hard_fails_when_tools_are_requested() {
        match reject_tool_bearing_request("cc", "claude", 2) {
            Err(Error::CapabilityMismatch {
                provider,
                capability,
                message,
                ..
            }) => {
                assert_eq!(provider, "cc");
                assert_eq!(capability, "aletheia organon tool-loop");
                assert!(message.contains("claude"));
                assert!(message.contains('2'));
            }
            other => panic!("expected Error::CapabilityMismatch, got: {other:?}"),
        }
    }
}
