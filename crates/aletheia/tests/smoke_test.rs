//! Smoke tests for the `aletheia` CLI binary.
//!
//! These tests use `assert_cmd` to invoke the compiled binary and verify that
//! every subcommand is reachable, parses arguments, and produces useful output
//! or graceful failures without a live server.

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
    aletheia()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"aletheia \d+\.\d+\.\d+").unwrap());
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

// ── Subcommand --help (all 16) ────────────────────────────────────────────────

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

// ── Health — graceful failure without a server ────────────────────────────────

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

// ── Status — graceful failure without a server ────────────────────────────────

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

// ── Init — non-destructive (isolated temp dir) ────────────────────────────────

#[test]
fn init_with_missing_api_key_fails_usefully() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_path = tmp.path().join("instance");

    // Run with --yes (non-interactive) but no API key set.
    // Should either succeed (unlikely in CI) or fail with a useful message —
    // not panic or exit 101.
    let output = aletheia()
        .args([
            "init",
            "--instance-root",
            instance_path.to_str().unwrap(),
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

// ── Import — missing file produces useful error ───────────────────────────────

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
                .or(predicate::str::contains("cannot")),
        );
}

// ── Seed-skills — missing dir exits non-zero ──────────────────────────────────

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
