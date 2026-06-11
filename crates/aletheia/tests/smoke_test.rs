//! Smoke tests for the `aletheia` CLI binary.
//!
//! These tests use `assert_cmd` to invoke the compiled binary and verify that
//! every subcommand is reachable, parses arguments, and produces useful output
//! or graceful failures without a live server.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

use assert_cmd::Command;
use predicates::prelude::*;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn aletheia() -> Command {
    Command::cargo_bin("aletheia").expect("aletheia binary must be compiled")
}

// ── Top-level flags ───────────────────────────────────────────────────────────

#[test]
fn top_level_help_exits_zero() {
    aletheia()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("aletheia"));
}

#[test]
fn top_level_version_format() {
    // --version should print "aletheia X.Y.Z"
    aletheia().arg("--version").assert().success().stdout(
        predicate::str::is_match(r"aletheia \d+\.\d+\.\d+").expect("version regex is valid"),
    );
}

#[test]
fn top_level_help_lists_all_subcommands() {
    let expected = [
        "health",
        "backup",
        "maintenance",
        "tls",
        "status",
        "credential",
        "eval",
        "export",
        "tui",
        "migrate-memory",
        "init",
        "import",
        "seed-skills",
        "export-skills",
        "review-skills",
        "completions",
    ];
    let output = aletheia()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help_text = String::from_utf8_lossy(&output);
    for sub in expected {
        assert!(
            help_text.contains(sub),
            "--help output missing subcommand '{sub}'"
        );
    }
}

// ── Subcommand --help ─────────────────────────────────────────────────────────

macro_rules! help_test {
    ($name:ident, $($arg:expr),+) => {
        #[test]
        fn $name() {
            aletheia()
                $(.arg($arg))+
                .arg("--help")
                .assert()
                .success();
        }
    };
}

help_test!(health_help, "health");
help_test!(backup_help, "backup");
help_test!(maintenance_help, "maintenance");
help_test!(maintenance_status_help, "maintenance", "status");
help_test!(maintenance_run_help, "maintenance", "run");
help_test!(tls_help, "tls");
help_test!(tls_generate_help, "tls", "generate");
help_test!(status_help, "status");
help_test!(credential_help, "credential");
help_test!(credential_status_help, "credential", "status");
help_test!(credential_refresh_help, "credential", "refresh");
help_test!(eval_help, "eval");
help_test!(export_help, "export");
help_test!(tui_help, "tui");
help_test!(migrate_memory_help, "migrate-memory");
help_test!(init_help, "init");
help_test!(import_help, "import");
help_test!(seed_skills_help, "seed-skills");
help_test!(export_skills_help, "export-skills");
help_test!(review_skills_help, "review-skills");
help_test!(completions_help, "completions");
help_test!(config_help, "config");
help_test!(config_diff_help, "config", "diff");

// ── Completions (fully offline) ───────────────────────────────────────────────

#[test]
fn completions_bash_exits_zero() {
    aletheia().args(["completions", "bash"]).assert().success();
}

#[test]
fn completions_bash_output_contains_aletheia() {
    aletheia()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("aletheia"));
}

#[test]
fn completions_zsh_exits_zero() {
    aletheia().args(["completions", "zsh"]).assert().success();
}

#[test]
fn completions_fish_exits_zero() {
    aletheia().args(["completions", "fish"]).assert().success();
}

#[test]
fn completions_invalid_shell_fails() {
    aletheia()
        .args(["completions", "not-a-shell"])
        .assert()
        .failure();
}

// ── Health: graceful failure without a server ────────────────────────────────

#[test]
fn health_graceful_failure_no_server() {
    // Port 19999 should have nothing listening in CI. The command must exit
    // with code 0 or 1 (not panic / 101 etc.) and print a useful message.
    let output = aletheia()
        .args(["health", "--url", "http://127.0.0.1:19999"])
        .output()
        .expect("failed to run aletheia health");

    let exit_code = output.status.code().unwrap_or(2);
    assert!(
        exit_code <= 1,
        "health exited with code {exit_code}, expected 0 or 1"
    );

    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(
        combined.to_lowercase().contains("error")
            || combined.to_lowercase().contains("connect")
            || combined.to_lowercase().contains("refused")
            || combined.to_lowercase().contains("unreachable")
            || combined.to_lowercase().contains("failed"),
        "health output should mention a connection problem; got: {combined}"
    );
}

// ── Status: graceful failure without a server ────────────────────────────────

#[test]
fn status_graceful_failure_no_server() {
    let output = aletheia()
        .args(["status", "--url", "http://127.0.0.1:19999"])
        .output()
        .expect("failed to run aletheia status");

    let exit_code = output.status.code().unwrap_or(2);
    assert!(
        exit_code <= 1,
        "status exited with code {exit_code}, expected 0 or 1"
    );
}

// ── Init: non-destructive (isolated temp dir) ────────────────────────────────

#[test]
fn init_with_missing_api_key_fails_usefully() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_path = tmp.path().join("instance");

    // Run with --yes (non-interactive) but no API key set.
    // Should either succeed (unlikely in CI) or fail with a useful message,
    // not panic or exit 101.
    let output = aletheia()
        .args([
            "init",
            "--instance-root",
            instance_path
                .to_str()
                .expect("instance path is valid UTF-8"),
            "--yes",
        ])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("failed to run aletheia init");

    let exit_code = output.status.code().unwrap_or(255);
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));

    assert!(
        exit_code <= 1
            || combined.to_lowercase().contains("api")
            || combined.to_lowercase().contains("key")
            || combined.to_lowercase().contains("credential")
            || combined.to_lowercase().contains("error"),
        "init should exit cleanly or produce a useful error; exit={exit_code}, output={combined}"
    );
}

// ── Init -> check-config round trip exits 0 ──────────────────────────────────

/// `aletheia init -y` followed by `aletheia check-config` against the same
/// instance must exit 0 — a fresh init should always produce a config that
/// check-config accepts. Regression test for #4240, which fixed the case
/// where init wrote `gateway.auth.mode = "none"` but check-config rejected
/// it as a hard FAIL unless the operator had set `ALETHEIA_ALLOW_AUTH_NONE=1`.
#[test]
fn init_yes_followed_by_check_config_exits_zero() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_path = tmp.path().join("instance");

    let init_out = aletheia()
        .args([
            "init",
            "--instance-root",
            instance_path
                .to_str()
                .expect("instance path is valid UTF-8"),
            "--yes",
        ])
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ALETHEIA_ALLOW_AUTH_NONE")
        .output()
        .expect("failed to run aletheia init");
    assert!(
        init_out.status.success(),
        "init -y must exit 0; got {:?}\nstdout: {}\nstderr: {}",
        init_out.status.code(),
        String::from_utf8_lossy(&init_out.stdout),
        String::from_utf8_lossy(&init_out.stderr),
    );

    let check_out = aletheia()
        .args([
            "-r",
            instance_path
                .to_str()
                .expect("instance path is valid UTF-8"),
            "check-config",
        ])
        .env_remove("ALETHEIA_ALLOW_AUTH_NONE")
        .output()
        .expect("failed to run aletheia check-config");

    let stdout = String::from_utf8_lossy(&check_out.stdout);
    let stderr = String::from_utf8_lossy(&check_out.stderr);
    assert!(
        check_out.status.success(),
        "check-config on fresh init must exit 0; got {:?}\nstdout: {stdout}\nstderr: {stderr}",
        check_out.status.code(),
    );
    assert!(
        stdout.contains("Configuration OK"),
        "check-config output should report Configuration OK; got: {stdout}"
    );
    assert!(
        stdout.contains("[warn] gateway.auth"),
        "check-config must surface the disabled-auth posture as a [warn], not a [FAIL]; got: {stdout}"
    );
}

// ── Import: missing file produces useful error ───────────────────────────────

#[test]
fn import_missing_file_produces_error() {
    aletheia()
        .args(["import", "/nonexistent/path/agent.json"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("No such file")
                .or(predicate::str::contains("not found"))
                .or(predicate::str::contains("error"))
                .or(predicate::str::contains("Error"))
                .or(predicate::str::contains("cannot"))
                .or(predicate::str::contains("unavailable")),
        );
}

// ── Seed-skills: missing dir exits non-zero ──────────────────────────────────

#[test]
fn seed_skills_missing_dir_exits_nonzero() {
    aletheia()
        .args([
            "seed-skills",
            "--dir",
            "/nonexistent/skills/dir",
            "--nous-id",
            "test-agent",
            "--dry-run",
        ])
        .assert()
        .failure();
}

// ── Unknown subcommand exits non-zero ─────────────────────────────────────────

#[test]
fn unknown_subcommand_exits_nonzero() {
    aletheia()
        .arg("totally-unknown-subcommand")
        .assert()
        .failure();
}

// ── No subcommand shows help or starts server (not panics) ────────────────────

#[test]
fn no_args_does_not_panic() {
    // Running without arguments should either start the server (exit blocked by
    // missing config) or print help. It must not exit with code 101 (Rust panic).
    let output = aletheia()
        .output()
        .expect("failed to run aletheia with no args");

    let exit_code = output.status.code().unwrap_or(101);
    assert_ne!(exit_code, 101, "aletheia exited with a panic exit code");
}

// ── Backup verify ────────────────────────────────────────────────────────────

#[test]
fn backup_verify_exits_zero_on_valid_backup() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("db");

    // Create a small fjall database.
    {
        let db = fjall::SingleWriterTxDatabase::builder(&db_path)
            .open()
            .expect("open fjall db");
        let ks = db
            .keyspace("sessions", fjall::KeyspaceCreateOptions::default)
            .expect("create keyspace");
        // WHY: Session uses #[serde(flatten)] for metrics and origin, so fields
        // must be at the top level, not nested.
        let session = serde_json::json!({
            "id": "sess-1",
            "nous_id": "syn",
            "session_key": "default",
            "status": "active",
            "model": null,
            "session_type": "primary",
            "created_at": "2024-01-01T00:00:00.000Z",
            "updated_at": "2024-01-01T00:00:00.000Z",
            "token_count_estimate": 0,
            "message_count": 0,
            "last_input_tokens": 0,
            "bootstrap_hash": null,
            "distillation_count": 0,
            "last_distilled_at": null,
            "computed_context_tokens": 0,
            "parent_session_id": null,
            "thread_id": null,
            "transport": null,
            "display_name": null
        });
        ks.insert("sess-1", serde_json::to_vec(&session).unwrap().as_slice())
            .expect("insert");
        // db is dropped here, but fjall may hold background locks briefly.
    }

    // WHY: copy to a second path so the verifier doesn't contend with any
    // lingering background threads from the creation handle.
    let verify_path = tmp.path().join("verify");
    copy_dir(&db_path, &verify_path);

    let output = aletheia()
        .args(["backup", "verify", verify_path.to_str().unwrap()])
        .output()
        .expect("failed to run aletheia backup verify");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit_code = output.status.code().unwrap_or(255);

    assert_eq!(
        exit_code, 0,
        "backup verify should exit 0 on valid backup\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("PASS"),
        "stdout should contain PASS\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("sessions"),
        "stdout should list sessions partition\nstdout: {stdout}"
    );
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).expect("create_dir_all");
    for entry in std::fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("dir entry");
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).expect("copy");
        }
    }
}
