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
        "poiesis",
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
help_test!(poiesis_help, "poiesis");
help_test!(poiesis_create_help, "poiesis", "create");
help_test!(poiesis_list_help, "poiesis", "list");
help_test!(poiesis_get_help, "poiesis", "get");
help_test!(poiesis_preview_help, "poiesis", "preview");
help_test!(poiesis_qa_help, "poiesis", "qa");
help_test!(poiesis_lint_help, "poiesis", "lint");
help_test!(poiesis_verify_help, "poiesis", "verify");
help_test!(poiesis_run_help, "poiesis", "run");

// ── Poiesis: end-to-end, no server or instance required ──────────────────────

#[test]
fn poiesis_list_components_lists_the_shipped_packs() {
    // forkwright/aletheia#7172 acceptance: `poiesis list-components` must
    // work end-to-end and enumerate the shipped Deck component packs.
    aletheia()
        .arg("poiesis")
        .arg("list-components")
        .assert()
        .success()
        .stdout(predicate::str::contains("title"))
        .stdout(predicate::str::contains("bullet"))
        .stdout(predicate::str::contains("chart"));
}

#[test]
fn poiesis_list_json_is_parseable() {
    let output = aletheia()
        .args(["poiesis", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value =
        serde_json::from_slice(&output).expect("poiesis list --json must emit valid JSON");
    let components = parsed
        .get("components")
        .and_then(serde_json::Value::as_array)
        .expect("components must be an array");
    assert!(
        components.iter().any(|v| v.as_str() == Some("title")),
        "expected 'title' in {components:?}"
    );
}

#[test]
fn poiesis_get_unknown_component_fails_loud() {
    aletheia()
        .args(["poiesis", "get", "no-such-component"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown component"));
}

#[test]
fn poiesis_get_known_component_prints_schema() {
    aletheia()
        .args(["poiesis", "get", "title"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"schema\""));
}

#[tokio::test]
async fn poiesis_preview_renders_a_pdf_end_to_end() {
    // forkwright/aletheia#7172 acceptance: `poiesis preview` must work
    // end-to-end. The default built-in Typst template needs no `--data`.
    let dir = tempfile::tempdir().expect("create temp dir");
    let out_path = dir.path().join("preview.pdf");
    aletheia()
        .args([
            "poiesis",
            "preview",
            "--out",
            out_path.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    let bytes = tokio::fs::read(&out_path)
        .await
        .expect("preview must write the output file");
    assert!(
        bytes.starts_with(b"%PDF"),
        "preview output must be a real PDF"
    );
}

#[test]
fn poiesis_preview_rejects_unknown_template() {
    // forkwright/aletheia#7172 required-fixes: a `--template` slug the
    // engine does not recognize must fail loud, naming the slug, rather
    // than silently falling back to a default.
    aletheia()
        .args(["poiesis", "preview", "--template", "no-such-template"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no-such-template"));
}

#[tokio::test]
async fn poiesis_create_scaffolds_a_project_to_a_directory() {
    // forkwright/aletheia#7172: canon `create` is `scaffold_report`, not a
    // render -- this proves the CLI -> tool wiring writes real files to
    // `--dir`, decoded from the tool's base64 manifest.
    let dir = tempfile::tempdir().expect("create temp dir");
    aletheia()
        .args([
            "poiesis",
            "create",
            "--slug",
            "quarterly-review",
            "--format",
            "typst",
        ])
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("wrote"));
    let report_path = dir.path().join("report.typ");
    let bytes = tokio::fs::read(&report_path)
        .await
        .expect("create --dir must write the scaffolded report.typ");
    assert!(!bytes.is_empty(), "scaffolded report.typ must not be empty");
}

#[tokio::test]
async fn poiesis_run_renders_a_document_end_to_end() {
    // WHY --format odt: `generate_document`'s docx/html/md/latex/epub/pdf
    // paths all shell out to a system Pandoc install (`poiesis-doc`'s
    // `pandoc` feature); odt is the one format rendered by a pure-Rust
    // writer (`poiesis-text::OdtRenderer`), so this proves the CLI -> tool
    // wiring end-to-end without depending on box-specific tooling.
    let dir = tempfile::tempdir().expect("create temp dir");
    let content_path = dir.path().join("blocks.json");
    tokio::fs::write(
        &content_path,
        r#"[{"type":"heading","level":1,"text":"Hello"},{"type":"paragraph","text":"World"}]"#,
    )
    .await
    .expect("write content fixture");
    let out_path = dir.path().join("out.odt");

    aletheia()
        .args(["poiesis", "run", "--format", "odt"])
        .arg("--content")
        .arg(&content_path)
        .arg("--out")
        .arg(&out_path)
        .assert()
        .success();
    let bytes = tokio::fs::read(&out_path)
        .await
        .expect("run must write the output file");
    assert!(
        bytes.starts_with(b"PK"),
        "ODT output must be a real zip-based document"
    );
}

#[tokio::test]
async fn poiesis_run_rejects_non_json_content() {
    // forkwright/aletheia#7172 required-fixes: malformed `--content` must
    // fail loud naming `--content`, not fail deep inside the render tool.
    let dir = tempfile::tempdir().expect("create temp dir");
    let content_path = dir.path().join("blocks.txt");
    tokio::fs::write(&content_path, "not json at all")
        .await
        .expect("write malformed content fixture");

    aletheia()
        .args(["poiesis", "run", "--format", "odt"])
        .arg("--content")
        .arg(&content_path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--content"));
}

#[tokio::test]
async fn poiesis_qa_passes_clean_prose() {
    // WHY this exact fixture: `qa_gate`'s prose lint requires a lead
    // section (`## Summary` et al.) and a closing section (`## Appendix`
    // et al.) in addition to no banned words/citations/theme-token
    // violations -- see `poiesis_lint::structure::check_sections`.
    let dir = tempfile::tempdir().expect("create temp dir");
    let prose_path = dir.path().join("prose.txt");
    tokio::fs::write(
        &prose_path,
        "## Summary\n\nThis is a perfectly ordinary sentence about the project status.\n\n## Appendix\n",
    )
    .await
    .expect("write prose fixture");

    aletheia()
        .args(["poiesis", "qa", "--prose"])
        .arg(&prose_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"has_issues\":false"));
}

#[tokio::test]
async fn poiesis_qa_reports_prose_findings() {
    // forkwright/aletheia#7172 required-fixes: `qa` must exit non-zero when
    // `QaReport.has_issues` is true, not just print the report and exit 0.
    let dir = tempfile::tempdir().expect("create temp dir");
    let prose_path = dir.path().join("prose.txt");
    tokio::fs::write(
        &prose_path,
        "This report will delve into the quarterly numbers.",
    )
    .await
    .expect("write prose fixture");

    aletheia()
        .args(["poiesis", "qa", "--prose"])
        .arg(&prose_path)
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"has_issues\":true"));
}

#[tokio::test]
async fn poiesis_lint_reports_banned_word_findings() {
    // forkwright/aletheia#7172 required-fixes: `lint` (the standalone
    // decomposition of `qa`'s prose half) must exit non-zero on findings.
    let dir = tempfile::tempdir().expect("create temp dir");
    let prose_path = dir.path().join("prose.txt");
    tokio::fs::write(
        &prose_path,
        "This report will delve into the quarterly numbers.",
    )
    .await
    .expect("write prose fixture");

    aletheia()
        .args(["poiesis", "lint", "--prose"])
        .arg(&prose_path)
        .assert()
        .failure()
        .stdout(predicate::str::contains("delve"));
}

#[tokio::test]
async fn poiesis_verify_rejects_malformed_manifest() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let manifest_path = dir.path().join("manifest.json");
    tokio::fs::write(&manifest_path, "not json at all")
        .await
        .expect("write malformed manifest fixture");

    aletheia()
        .args(["poiesis", "verify", "--manifest"])
        .arg(&manifest_path)
        .assert()
        .failure();
}

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

/// `check-config` must reject an invalid `credential.source` rather than
/// reporting `Configuration OK`. Regression test for #5770: `check-config`
/// and server startup kept independent section lists, and check-config's
/// omitted `credential` — so an operator got a clean pre-flight report and
/// then a failed start on the very same file.
///
/// This asserts the failure, not the fix's shape: it fires against the old
/// hand-maintained list and is silent once both paths share one derived list.
#[test]
fn check_config_rejects_invalid_credential_source() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_path = tmp.path().join("instance");
    let instance_arg = instance_path
        .to_str()
        .expect("instance path is valid UTF-8")
        .to_owned();

    let init_out = aletheia()
        .args(["init", "--instance-root", &instance_arg, "--yes"])
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ALETHEIA_ALLOW_AUTH_NONE")
        .output()
        .expect("failed to run aletheia init");
    assert!(
        init_out.status.success(),
        "init -y must exit 0; got {:?}\nstderr: {}",
        init_out.status.code(),
        String::from_utf8_lossy(&init_out.stderr),
    );

    let config_file = instance_path.join("config").join("aletheia.toml");
    let config = std::fs::read_to_string(&config_file).expect("read generated config");
    // WHY: "env-only" is syntactically fine and deserializes cleanly — only
    // validate_credential rejects it. That isolates the section-list gap from
    // parse errors, which check-config already caught.
    let patched = config.replace(
        "[credential]\nsource = \"auto\"",
        "[credential]\nsource = \"env-only\"",
    );
    assert_ne!(
        patched, config,
        "init should write [credential] source = \"auto\"; the test's patch target has moved"
    );
    let mut handle =
        std::fs::File::create(&config_file).expect("reopen config for the invalid-source patch");
    std::io::Write::write_all(&mut handle, patched.as_bytes())
        .expect("write config with invalid credential.source");
    drop(handle);

    let check_out = aletheia()
        .args(["-r", &instance_arg, "check-config"])
        .env_remove("ALETHEIA_ALLOW_AUTH_NONE")
        .output()
        .expect("failed to run aletheia check-config");

    let stdout = String::from_utf8_lossy(&check_out.stdout);
    let stderr = String::from_utf8_lossy(&check_out.stderr);
    assert!(
        !check_out.status.success(),
        "check-config must fail on an invalid credential.source; got {:?}\nstdout: {stdout}\nstderr: {stderr}",
        check_out.status.code(),
    );
    assert!(
        !stdout.contains("Configuration OK"),
        "check-config must not report Configuration OK on an invalid credential.source; got: {stdout}"
    );
    assert!(
        stdout.contains("[FAIL] credential"),
        "check-config should name the failing section; got: {stdout}"
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

// ── Eval: --json stdout is parseable JSON, not tracing output ───────────────

/// Regression test for #5397: eval report rendering used `tracing::info!` for
/// `--json` output. On the plain CLI eval path no tracing subscriber is
/// installed (only the `server` subcommand initializes one), so the report
/// was silently dropped instead of reaching stdout as parseable JSON.
#[test]
fn eval_json_stdout_is_parseable_json_with_no_log_prefix() {
    // Port 19999 has nothing listening; the scenario fails fast on connection
    // refused, but the report must still be written to stdout as raw JSON.
    let output = aletheia()
        .args([
            "eval",
            "--url",
            "http://127.0.0.1:19999",
            "--scenario",
            "health-returns-ok",
            "--timeout",
            "2",
            "--json",
        ])
        .output()
        .expect("failed to run aletheia eval --json");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.trim().is_empty(),
        "eval --json must write the report to stdout, got empty output"
    );
    assert!(
        !stdout.contains("INFO") && !stdout.contains("WARN") && !stdout.contains("ERROR"),
        "eval --json stdout must not carry a tracing log-level prefix; got: {stdout}"
    );

    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("eval --json stdout is not parseable JSON: {e}\nstdout: {stdout}")
    });
    assert!(
        parsed.get("results").is_some(),
        "parsed eval report JSON should contain a results field; got: {parsed}"
    );
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
