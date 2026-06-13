//! Durable provenance envelope shared by eval runs and memory benchmark reports.
//!
//! `EvalProvenance` carries a stable run identity, versioning, redacted CLI args,
//! target identity, and opaque audit refs for model/provider/prompt/tool/memory
//! configuration. It intentionally never stores secrets: raw tokens, API keys,
//! and passwords are redacted before serialization, and the original config is
//! replaced by a SHA-256 hash.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::persistence::now_iso8601;

/// Shared provenance envelope for eval runs and benchmark reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EvalProvenance {
    /// Stable identifier for this eval run.
    // kanon:ignore RUST/primitive-for-domain-id — eval_run_id is an opaque external run handle, not an internal domain newtype
    pub eval_run_id: String,
    /// Schema version of this provenance envelope.
    pub schema_version: u32,
    /// Version of the `dokimion` crate that produced the run.
    pub dokimion_version: String,
    /// Git commit SHA of the running binary, when available.
    pub git_sha: Option<String>,
    /// ISO-8601 timestamp when the run started.
    pub started_at: String,
    /// ISO-8601 timestamp when the run finished, if known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub finished_at: Option<String>,
    /// CLI arguments with secret-bearing values redacted.
    pub redacted_args: Vec<String>,
    /// SHA-256 hash of the resolved run configuration.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub config_hash: Option<String>,
    /// Base URL of the target instance.
    pub target_base_url: String,
    /// Target identity (e.g. version from `/api/health`), when available.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_identity: Option<String>,
    /// Opaque model audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_ref: Option<String>,
    /// Opaque provider audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_ref: Option<String>,
    /// Opaque prompt audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prompt_ref: Option<String>,
    /// Opaque tool-surface audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_ref: Option<String>,
    /// Opaque memory-system audit reference.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub memory_ref: Option<String>,
    /// Hash of the scenario suite or benchmark dataset that was executed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scenario_suite_hash: Option<String>,
}

impl EvalProvenance {
    /// Create a new provenance envelope with the given identity and target.
    #[must_use]
    pub fn new(eval_run_id: impl Into<String>, target_base_url: impl Into<String>) -> Self {
        Self {
            eval_run_id: eval_run_id.into(),
            schema_version: 1,
            dokimion_version: format!("dokimion@{}", env!("CARGO_PKG_VERSION")),
            git_sha: None,
            started_at: now_iso8601(),
            finished_at: None,
            redacted_args: Vec::new(),
            config_hash: None,
            target_base_url: target_base_url.into(),
            target_identity: None,
            model_ref: None,
            provider_ref: None,
            prompt_ref: None,
            tool_ref: None,
            memory_ref: None,
            scenario_suite_hash: None,
        }
    }

    /// Mark the run as finished.
    #[must_use]
    pub fn finished(mut self) -> Self {
        self.finished_at = Some(now_iso8601());
        self
    }

    /// Attach a git SHA.
    #[must_use]
    pub fn with_git_sha(mut self, git_sha: impl Into<String>) -> Self {
        self.git_sha = Some(git_sha.into());
        self
    }

    /// Attach redacted CLI args.
    #[must_use]
    pub fn with_redacted_args(mut self, args: &[String]) -> Self {
        self.redacted_args = redact_args(args);
        self
    }

    /// Attach a configuration hash.
    #[must_use]
    pub fn with_config_hash(mut self, hash: impl Into<String>) -> Self {
        self.config_hash = Some(hash.into());
        self
    }

    /// Attach target identity.
    #[must_use]
    pub fn with_target_identity(mut self, identity: impl Into<String>) -> Self {
        self.target_identity = Some(identity.into());
        self
    }

    /// Attach opaque audit refs.
    #[must_use]
    pub fn with_audit_refs(
        mut self,
        model: Option<String>,
        provider: Option<String>,
        prompt: Option<String>,
        tool: Option<String>,
        memory: Option<String>,
    ) -> Self {
        self.model_ref = model;
        self.provider_ref = provider;
        self.prompt_ref = prompt;
        self.tool_ref = tool;
        self.memory_ref = memory;
        self
    }

    /// Attach a scenario-suite or dataset hash.
    #[must_use]
    pub fn with_scenario_suite_hash(mut self, hash: impl Into<String>) -> Self {
        self.scenario_suite_hash = Some(hash.into());
        self
    }
}

/// Generate a stable eval run id without adding broad dependencies.
///
/// The id is deterministic for a given process start and monotonic clock sample,
/// plus the OS process id, which is sufficient to disambiguate concurrent runs
/// on the same host.
#[must_use]
pub fn generate_eval_run_id() -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("er-{pid}-{nanos}")
}

/// Compute a hex-encoded SHA-256 hash of a byte slice.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_encode(&digest)
}

/// Compute a hex-encoded SHA-256 hash of a string.
#[must_use]
pub fn sha256_hex_str(s: &str) -> String {
    sha256_hex(s.as_bytes())
}

/// Redact secret-bearing CLI argument values.
///
/// Flags matching `--token`, `--*-token`, `--api-key`, `--*-api-key`, `--key`,
/// `--password`, `--secret`, and env-token patterns have their values replaced
/// with `[REDACTED]`. Bare positional values that look like keys (long base64
/// strings or strings starting with `sk-`) are also redacted.
#[must_use]
pub fn redact_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            out.push("[REDACTED]".to_owned());
            skip_next = false;
            continue;
        }

        if let Some((flag, _value)) = arg.split_once('=')
            && is_secret_flag(flag)
        {
            out.push(format!("{flag}=[REDACTED]"));
            continue;
        }

        if is_secret_flag(arg) {
            out.push(arg.clone());
            skip_next = true;
            continue;
        }

        if looks_like_secret(arg) {
            out.push("[REDACTED]".to_owned());
            continue;
        }

        out.push(arg.clone());
    }

    out
}

fn is_secret_flag(flag: &str) -> bool {
    let lower = flag.to_lowercase();
    let secret_flags: &[&str] = &[
        "--token",
        "--api-key",
        "--key",
        "--password",
        "--secret",
        "--judge-api-key",
    ];
    secret_flags.iter().any(|s| lower == *s)
        || lower.ends_with("-token")
        || lower.ends_with("-api-key")
        || lower.ends_with("-password")
        || lower.ends_with("-secret")
}

fn looks_like_secret(value: &str) -> bool {
    let lower = value.to_lowercase();
    if lower.starts_with("sk-")
        || lower.starts_with("ak-")
        || lower.starts_with("pk-")
        || lower.starts_with("bearer ")
    {
        return true;
    }
    if value.len() < 16 {
        return false;
    }
    value.len() >= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(hex_nibble(b >> 4));
        s.push(hex_nibble(b & 0x0f));
    }
    s
}

fn hex_nibble(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => '?',
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn generate_eval_run_id_is_nonempty() {
        let id = generate_eval_run_id();
        assert!(id.starts_with("er-"));
        assert!(id.len() > 5);
    }

    #[test]
    fn generate_eval_run_id_unique_in_sequence() {
        let a = generate_eval_run_id();
        let b = generate_eval_run_id();
        assert_ne!(a, b, "consecutive IDs should differ by nanosecond");
    }

    #[test]
    fn sha256_hex_stable() {
        let h1 = sha256_hex_str("hello");
        let h2 = sha256_hex_str("hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn redact_args_token_next_value() {
        let args = vec![
            "aletheia".to_owned(),
            "eval".to_owned(),
            "--url".to_owned(),
            "http://localhost".to_owned(),
            "--token".to_owned(),
            "super-secret".to_owned(),
        ];
        let redacted = redact_args(&args);
        assert!(redacted.contains(&"[REDACTED]".to_owned()));
        assert!(!redacted.contains(&"super-secret".to_owned()));
    }

    #[test]
    fn redact_args_token_equals() {
        let args = vec!["--token=super-secret".to_owned()];
        let redacted = redact_args(&args);
        assert_eq!(redacted, vec!["--token=[REDACTED]"]);
    }

    #[test]
    fn redact_args_judge_api_key() {
        let args = vec!["--judge-api-key".to_owned(), "sk-abc123".to_owned()];
        let redacted = redact_args(&args);
        assert_eq!(redacted, vec!["--judge-api-key", "[REDACTED]"]);
    }

    #[test]
    fn redact_args_keeps_safe_values() {
        let args = vec![
            "--url".to_owned(),
            "http://localhost".to_owned(),
            "--timeout".to_owned(),
            "30".to_owned(),
        ];
        let redacted = redact_args(&args);
        assert_eq!(redacted, args);
    }

    #[test]
    fn redact_args_detects_bare_secret() {
        let args = vec!["sk-abc123def456".to_owned()];
        let redacted = redact_args(&args);
        assert_eq!(redacted, vec!["[REDACTED]"]);
    }

    #[test]
    fn provenance_serializes_without_token() {
        let provenance = EvalProvenance::new("er-1", "http://localhost")
            .with_redacted_args(&["--token".to_owned(), "secret".to_owned()]);
        let json = serde_json::to_string(&provenance).unwrap();
        assert!(json.contains("eval_run_id"));
        assert!(json.contains("er-1"));
        assert!(!json.contains("secret"));
    }
}
