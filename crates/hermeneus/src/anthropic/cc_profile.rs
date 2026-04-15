//! Claude Code profile for OAuth-gated API access.
//!
//! Extracts version and the minimum beta-header set from the installed
//! `claude` CLI binary so that hermeneus requests look enough like Claude
//! Code traffic to unlock Sonnet/Opus on OAuth tokens. Only active when
//! OAuth credentials are in use.
//!
//! # Sovereignty (#3409)
//!
//! The upstream CC format includes a per-conversation fingerprint
//! (`SHA256(SALT + msg[4] + msg[7] + msg[20] + version)`) in both the
//! attribution string and a persistent `X-Claude-Code-Session-Id` header,
//! letting Anthropic correlate requests back to specific operator prompts
//! and sessions. Aletheia stays off that attribution surface:
//!
//! - Attribution fingerprint replaced by the stable literal `000`.
//! - Session-id header randomized per-request at the client layer
//!   (see `client::build_headers`).
//! - Per-process session UUID on this profile is removed entirely.
//!
//! The `cc_version`, `cc_entrypoint`, `anthropic-beta`, and `User-Agent`
//! values are preserved because Anthropic gates Sonnet/Opus access on a
//! well-formed attribution block and the beta set.

use std::process::Command;

use tracing::warn;

/// Core beta headers that CC sends for non-Haiku 1P models.
/// Derived from CC source (constants/betas.ts, utils/betas.ts).
const CORE_BETAS: &[&str] = &[
    "claude-code-20250219",
    "interleaved-thinking-2025-05-14",
    "context-management-2025-06-27",
    "prompt-caching-scope-2026-01-05",
    "oauth-2025-04-20",
    "redact-thinking-2026-02-12",
];

/// Profile of the installed Claude Code binary.
///
/// Used to satisfy the OAuth-tier gates on Anthropic's first-party API
/// without leaking operator-identifying fingerprints (#3409).
#[derive(Debug, Clone)]
pub(crate) struct CcProfile {
    /// CC version string (e.g., "2.1.92").
    pub version: String,
    /// Beta headers to send (comma-joined in the `anthropic-beta` header).
    pub beta_headers: Vec<String>,
}

impl CcProfile {
    /// Build a profile by extracting the version from the installed `claude` binary.
    ///
    /// Falls back to a hardcoded version if the binary is unavailable.
    pub fn from_installed_cli() -> Self {
        let version = detect_cc_version().unwrap_or_else(|| {
            warn!("could not detect claude CLI version, using fallback");
            "2.1.92".to_owned()
        });

        let beta_headers = CORE_BETAS.iter().map(|&s| s.to_owned()).collect();

        Self {
            version,
            beta_headers,
        }
    }

    /// Add the 1M context beta header if the model supports it.
    #[expect(
        dead_code,
        reason = "public API reserved for extended context window callers"
    )]
    pub fn with_context_1m(&mut self) {
        let header = "context-1m-2025-08-07";
        if !self.beta_headers.iter().any(|h| h == header) {
            self.beta_headers.push(header.to_owned());
        }
    }

    /// Build the sovereignty-scrubbed attribution string (#3409).
    ///
    /// This is NOT an HTTP header — it's a text block prepended to the system
    /// prompt array. CC embeds it as the first system block so the API can
    /// attribute the request. The 3-char slot that upstream CC fills with a
    /// per-conversation fingerprint is pinned to `000` here so Anthropic
    /// cannot correlate attribution back to operator prompts.
    ///
    /// The `first_message_text` parameter is retained for API compatibility
    /// and ignored on purpose.
    pub fn attribution_header(&self, _first_message_text: &str) -> String {
        format!(
            "x-anthropic-billing-header: cc_version={version}.000; cc_entrypoint=cli;",
            version = self.version,
        )
    }

    /// User-Agent string matching CC format.
    pub fn user_agent(&self) -> String {
        format!("claude-cli/{} (user, cli)", self.version)
    }

    /// Comma-joined beta headers for the `anthropic-beta` HTTP header.
    pub fn beta_header_value(&self) -> String {
        self.beta_headers.join(",")
    }
}

/// Detect the installed Claude Code version by running `claude --version`.
fn detect_cc_version() -> Option<String> {
    let output = Command::new("claude").arg("--version").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Format: "2.1.92 (Claude Code)" — extract the version number.
    stdout
        .split_whitespace()
        .next()
        .filter(|v| v.contains('.'))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_header_format() {
        let profile = CcProfile {
            version: "2.1.92".to_owned(),
            beta_headers: vec![],
        };
        let header = profile.attribution_header("Say 'hello' in one word. Nothing else.");
        assert!(header.starts_with("x-anthropic-billing-header: cc_version=2.1.92."));
        assert!(header.ends_with("; cc_entrypoint=cli;"));
    }

    /// Sovereignty (#3409): attribution must NOT vary with the user's
    /// first-message content. The upstream fingerprint is replaced by `000`
    /// so Anthropic cannot correlate attribution back to operator prompts.
    #[test]
    fn attribution_header_strips_operator_fingerprint() {
        let profile = CcProfile {
            version: "2.1.92".to_owned(),
            beta_headers: vec![],
        };
        let first = profile.attribution_header("first message with distinctive content");
        let second = profile.attribution_header("entirely different wording here");
        assert_eq!(
            first, second,
            "attribution must not fingerprint operator content"
        );
        assert!(
            first.contains(".000;"),
            "fingerprint slot must be stable placeholder, got: {first}"
        );
    }

    #[test]
    fn user_agent_format() {
        let profile = CcProfile {
            version: "2.1.92".to_owned(),
            beta_headers: vec![],
        };
        assert_eq!(profile.user_agent(), "claude-cli/2.1.92 (user, cli)");
    }
}
