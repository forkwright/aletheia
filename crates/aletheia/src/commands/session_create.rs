//! `aletheia session-create <nous-id> [--key <session-key>]`: create a session
//! directly in the local graphe store, bypassing the HTTP API.

use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use mneme::store::SessionStore;
use mneme::types::parse_session_or_agent_id;
use taxis::loader::load_config;
use taxis::oikos::Oikos;

use crate::error::Result;

const DEFAULT_SESSION_KEY: &str = "main";
const MAX_IDENTIFIER_BYTES: usize = 256;

#[derive(Debug, Clone, Args)]
pub(crate) struct SessionCreateArgs {
    /// Nous agent identifier to bind the session to.
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
    pub nous_id: String,

    /// Client-chosen key for session deduplication.
    #[arg(long, default_value = DEFAULT_SESSION_KEY)]
    pub key: String,
}

pub(crate) fn run(instance_root: Option<&PathBuf>, args: &SessionCreateArgs) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    if !oikos.root().exists() {
        snafu::whatever!(
            "instance not found at {}\n  \
             Use -r /path/to/instance or set ALETHEIA_ROOT.",
            oikos.root().display()
        );
    }

    validate_identifier(&args.nous_id, "nous_id")?;
    validate_identifier(&args.key, "key")?;

    let config = load_config(&oikos).with_whatever_context(|_| "failed to load aletheia config")?;

    let agent_exists = config.agents.list.iter().any(|a| a.id == args.nous_id);
    if !agent_exists {
        snafu::whatever!("nous agent '{}' not found in configuration", args.nous_id);
    }

    let resolved = taxis::config::resolve_nous(&config, &args.nous_id);
    let model = resolved.model.primary.to_string();

    let db_path = oikos.sessions_db();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_whatever_context(|_| format!("failed to create data dir {}", parent.display()))?;
    }

    let store = SessionStore::open(&db_path).with_whatever_context(|_| {
        format!("failed to open session store at {}", db_path.display())
    })?;

    let id = koina::id::SessionId::new().to_string();

    match store.create_session(&id, &args.nous_id, &args.key, None, Some(&model)) {
        Ok(session) => {
            let output = serde_json::json!({
                "id": session.id,
                "nous_id": session.nous_id,
                "session_key": session.session_key,
                "status": session.status.as_str(),
                "model": session.model,
                "message_count": session.metrics.message_count,
                "token_count_estimate": session.metrics.token_count_estimate,
                "created_at": session.created_at,
                "updated_at": session.updated_at,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&output)
                    .whatever_context("failed to serialize output")?
            );
            Ok(())
        }
        Err(e) if is_unique_constraint_violation(&e) => {
            snafu::whatever!(
                "a session with key '{}' already exists for agent '{}'",
                args.key,
                args.nous_id
            )
        }
        Err(e) => Err(crate::error::Error::msg(format!(
            "failed to create session: {e}"
        ))),
    }
}

fn validate_identifier(value: &str, field: &str) -> Result<()> {
    if value.is_empty() {
        snafu::whatever!("{field} must not be empty");
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        snafu::whatever!("{field} exceeds maximum length of {MAX_IDENTIFIER_BYTES} bytes");
    }
    parse_session_or_agent_id(value)
        .with_whatever_context(|_| format!("{field} uses a reserved internal prefix"))?;
    Ok(())
}

fn is_unique_constraint_violation(err: &mneme::error::Error) -> bool {
    err.is_unique_constraint_violation()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test fixture setup uses sync std::fs to write config files before exercising run()"
)]
mod tests {
    use super::*;

    #[test]
    fn validate_identifier_accepts_non_empty() {
        assert!(validate_identifier("syn", "nous_id").is_ok());
        assert!(validate_identifier("main", "key").is_ok());
    }

    #[test]
    fn validate_identifier_rejects_empty() {
        let result = validate_identifier("", "nous_id");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("must not be empty"), "got: {msg}");
    }

    #[test]
    fn validate_identifier_rejects_too_long() {
        let long = "x".repeat(MAX_IDENTIFIER_BYTES + 1);
        let result = validate_identifier(&long, "key");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("exceeds maximum length"), "got: {msg}");
    }

    #[test]
    fn run_rejects_reserved_cross_key_without_persisting() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(
            root.join("config/aletheia.toml"),
            r#"
[[agents.list]]
id = "alice"
name = "Alice"
workspace = "/tmp/alice"
"#,
        )
        .unwrap();

        let args = SessionCreateArgs {
            nous_id: "alice".to_owned(),
            key: "cross:victim".to_owned(),
        };

        let result = run(Some(&root.to_path_buf()), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("reserved internal prefix"), "got: {msg}");

        let store = SessionStore::open(&Oikos::from_root(root).sessions_db()).unwrap();
        let persisted = store.find_session("alice", "cross:victim").unwrap();
        assert!(
            persisted.is_none(),
            "CLI session-create must not persist reserved session keys"
        );
    }

    #[test]
    fn run_creates_session_and_prints_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(
            root.join("config/aletheia.toml"),
            r#"
[agents.defaults.model]
primary = "mock-model"

[[agents.list]]
id = "alice"
name = "Alice"
workspace = "/tmp/alice"
"#,
        )
        .unwrap();

        let args = SessionCreateArgs {
            nous_id: "alice".to_owned(),
            key: "cli-test-key".to_owned(),
        };

        run(Some(&root.to_path_buf()), &args).unwrap();

        // Verify the session exists in the store.
        let store = SessionStore::open(&Oikos::from_root(root).sessions_db()).unwrap();
        let session = store
            .find_session("alice", "cli-test-key")
            .unwrap()
            .expect("session should exist");
        assert_eq!(session.nous_id, "alice");
        assert_eq!(session.session_key, "cli-test-key");
        assert_eq!(session.status, mneme::types::SessionStatus::Active);
        assert_eq!(session.model.as_deref(), Some("mock-model"));
    }

    #[test]
    fn run_rejects_unknown_nous() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(
            root.join("config/aletheia.toml"),
            r#"
[[agents.list]]
id = "alice"
name = "Alice"
workspace = "/tmp/alice"
"#,
        )
        .unwrap();

        let args = SessionCreateArgs {
            nous_id: "bob".to_owned(),
            key: "main".to_owned(),
        };

        let result = run(Some(&root.to_path_buf()), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not found in configuration"), "got: {msg}");
    }

    #[test]
    fn run_returns_conflict_on_duplicate_key() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(
            root.join("config/aletheia.toml"),
            r#"
[[agents.list]]
id = "alice"
name = "Alice"
workspace = "/tmp/alice"
"#,
        )
        .unwrap();

        let args = SessionCreateArgs {
            nous_id: "alice".to_owned(),
            key: "dup-key".to_owned(),
        };

        run(Some(&root.to_path_buf()), &args).unwrap();

        let result = run(Some(&root.to_path_buf()), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("already exists"),
            "expected conflict error, got: {msg}"
        );
    }
}
