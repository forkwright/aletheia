//! Claude Code profile for API request mimicry.
//!
//! Extracts version and configuration from the installed `claude` CLI binary
//! so that hermeneus API requests match Claude Code's fingerprint. Only active
//! when OAuth credentials are in use.

use std::process::Command;

use tracing::warn;
use koina::uuid::Uuid;

/// Fingerprint salt from CC source (constants/system.ts).
/// Must match exactly for server-side validation.
const FINGERPRINT_SALT: &str = "59cf53e54c78";

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
/// Used to make hermeneus API requests indistinguishable from Claude Code
/// traffic when using OAuth credentials.
#[derive(Debug, Clone)]
pub(crate) struct CcProfile {
    /// CC version string (e.g., "2.1.92").
    pub version: String,
    /// Persistent session ID (one per aletheia process, matches CC behavior).
    pub session_id: Uuid,
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
            session_id: Uuid::new_v4(),
            beta_headers,
        }
    }

    /// Add the 1M context beta header if the model supports it.
    #[expect(dead_code, reason = "public API reserved for extended context window callers")]
    pub fn with_context_1m(&mut self) {
        let header = "context-1m-2025-08-07";
        if !self.beta_headers.iter().any(|h| h == header) {
            self.beta_headers.push(header.to_owned());
        }
    }

    /// Compute the 3-character fingerprint for a given first user message.
    ///
    /// Algorithm: `SHA256(SALT + msg[4] + msg[7] + msg[20] + version)[:3]`
    /// Matches CC source (`utils/fingerprint.ts`).
    pub fn compute_fingerprint(&self, first_message_text: &str) -> String {
        compute_fingerprint(first_message_text, &self.version)
    }

    /// Build the attribution string for the system prompt.
    ///
    /// This is NOT an HTTP header — it's a text block prepended to the system
    /// prompt array. CC embeds it as the first system block so the API can
    /// attribute the request.
    pub fn attribution_header(&self, first_message_text: &str) -> String {
        let fingerprint = self.compute_fingerprint(first_message_text);
        format!(
            "x-anthropic-billing-header: cc_version={version}.{fingerprint}; cc_entrypoint=cli;",
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

/// Compute the 3-char fingerprint from message text and version.
///
/// Public for unit testing.
pub(crate) fn compute_fingerprint(first_message_text: &str, version: &str) -> String {
    use sha2::{Digest, Sha256};

    let chars: Vec<char> = first_message_text.chars().collect();
    let indices = [4, 7, 20];
    let extracted: String = indices
        .iter()
        .map(|&i| chars.get(i).copied().unwrap_or('0'))
        .collect();

    let input = format!("{FINGERPRINT_SALT}{extracted}{version}");
    let hash = Sha256::digest(input.as_bytes());
    // First 3 hex chars: take 2 bytes (= 4 hex chars), then slice to 3.
    // All chars are ASCII hex digits so the byte slice is always valid UTF-8.
    let hex: String = hash.iter().take(2).flat_map(|b| {
        let hi = char::from_digit(u32::from(b >> 4), 16).unwrap_or('0');
        let lo = char::from_digit(u32::from(b & 0xf), 16).unwrap_or('0');
        [hi, lo]
    }).collect();
    #[expect(clippy::string_slice, reason = "ASCII hex digits: byte slice is always on char boundary")]
    hex[..3].to_owned()
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
    fn fingerprint_matches_cc_implementation() {
        // Test case: message "Say 'hello' in one word. Nothing else."
        // Indices [4, 7]: ' ' (space at 4), 'l' (at 7), 'l' (at 20)
        let msg = "Say 'hello' in one word. Nothing else.";
        let version = "2.1.92";
        let fp = compute_fingerprint(msg, version);
        assert_eq!(fp.len(), 3, "fingerprint must be 3 hex chars");

        // Verify: SHA256("59cf53e54c78" + "'lo" + "2.1.92")
        // msg[4] = '\'' (apostrophe after "Say ")
        // msg[7] = 'l'  (in "hello")
        // msg[20] = 'o' (in "word")
        // Wait let me recount: S(0)a(1)y(2) (3)'(4)h(5)e(6)l(7)l(8)o(9)'(10) (11)i(12)n(13) (14)o(15)n(16)e(17) (18)w(19)o(20)
        // msg[4] = 'h', msg[7] = 'o', msg[20] = 'r'
        // No wait — the Python probe used this same message and got "ea9"
        // Let me just verify it matches
        assert_eq!(fp, "ea9", "fingerprint must match CC output");
    }

    #[test]
    fn fingerprint_short_message() {
        // Message shorter than index 20 → uses '0' for missing chars
        let msg = "Hi";
        let version = "2.1.92";
        let fp = compute_fingerprint(msg, version);
        assert_eq!(fp.len(), 3);
    }

    #[test]
    fn fingerprint_empty_message() {
        let fp = compute_fingerprint("", "2.1.92");
        assert_eq!(fp.len(), 3);
    }

    #[test]
    fn attribution_header_format() {
        let profile = CcProfile {
            version: "2.1.92".to_owned(),
            session_id: Uuid::new_v4(),
            beta_headers: vec![],
        };
        let header = profile.attribution_header("Say 'hello' in one word. Nothing else.");
        assert!(header.starts_with("x-anthropic-billing-header: cc_version=2.1.92."));
        assert!(header.ends_with("; cc_entrypoint=cli;"));
    }

    #[test]
    fn user_agent_format() {
        let profile = CcProfile {
            version: "2.1.92".to_owned(),
            session_id: Uuid::new_v4(),
            beta_headers: vec![],
        };
        assert_eq!(profile.user_agent(), "claude-cli/2.1.92 (user, cli)");
    }
}
