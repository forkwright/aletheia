#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use crate::codex::parse;

use super::*;

const ETXTBSY: i32 = 26;

fn write_script(name: &str, body: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let final_path = std::env::temp_dir().join(format!(
        "hermeneus_codex_test_{name}_{}_{nonce}.sh",
        std::process::id()
    ));
    let tmp_path = final_path.with_extension("sh.tmp");
    let script = format!("#!/bin/sh\n{body}\n");
    {
        use std::io::Write;
        let mut file = fs::File::create(&tmp_path).unwrap();
        file.write_all(script.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }
    fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755)).unwrap();
    fs::rename(&tmp_path, &final_path).unwrap();

    for _ in 0..200 {
        match std::process::Command::new(&final_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                let _ = child.kill();
                let _ = child.wait();
                return final_path;
            }
            Err(e) if e.raw_os_error() == Some(ETXTBSY) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return final_path,
        }
    }

    final_path
}

fn make_temp_dir(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "hermeneus_codex_test_{name}_{}_{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn scrub_codex_auth_env_marks_openai_key_for_removal() {
    let mut cmd = tokio::process::Command::new("codex");
    cmd.env("OPENAI_API_KEY", "raw-api-key");

    scrub_codex_auth_env(&mut cmd);

    let envs: Vec<_> = cmd
        .as_std_mut()
        .get_envs()
        .filter_map(|(key, value)| {
            key.to_str()
                .filter(|name| *name == "OPENAI_API_KEY")
                .map(|name| (name.to_owned(), value.map(std::borrow::ToOwned::to_owned)))
        })
        .collect();

    assert_eq!(envs, vec![("OPENAI_API_KEY".to_owned(), None)]);
}

#[test]
fn compose_stdin_with_system_prompt() {
    let input = compose_stdin(Some("Be terse."), "Say hi.").unwrap();
    assert_eq!(input, "System:\nBe terse.\n\nUser:\nSay hi.");
}

#[test]
fn compose_stdin_rejects_oversized_system_prompt() {
    let big_prompt = "x".repeat(MAX_SYSTEM_PROMPT_BYTES + 1);
    let err = compose_stdin(Some(&big_prompt), "hello").unwrap_err();
    assert!(
        err.to_string()
            .contains("system prompt exceeds maximum size")
    );
}

#[tokio::test]
async fn read_bounded_accepts_output_within_limits() {
    let text = b"one\ntwo\n";
    let output = read_bounded(text.as_slice(), "stdout").await.unwrap();
    assert_eq!(output, text);
}

#[tokio::test]
async fn read_bounded_rejects_oversized_output_by_bytes() {
    let text = vec![b'x'; MAX_OUTPUT_BYTES + 1];
    let err = read_bounded(text.as_slice(), "stdout").await.unwrap_err();
    assert!(err.to_string().contains("byte limit"));
}

#[tokio::test]
async fn read_bounded_rejects_too_many_lines() {
    let text = "\n".repeat(MAX_OUTPUT_LINES + 1);
    let err = read_bounded(text.as_bytes(), "stdout").await.unwrap_err();
    assert!(err.to_string().contains("line limit"));
}

#[tokio::test]
async fn run_completion_spawn_failure_reports_binary_path() {
    let binary = PathBuf::from("/nonexistent/path/to/codex-binary");
    let err = run_completion(&binary, None, None, "hello", Duration::from_secs(5))
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("/nonexistent/path/to/codex-binary"));
    assert!(msg.contains("provider init failed"));
}

#[tokio::test]
async fn run_completion_success_collects_jsonl_output() {
    let script = write_script(
        "completion_ok",
        r#"test "$1" = "exec" || exit 31
test "$2" = "--dangerously-bypass-approvals-and-sandbox" || exit 32
test "$3" = "--skip-git-repo-check" || exit 33
test "$4" = "--color" || exit 34
test "$5" = "never" || exit 35
test "$6" = "--json" || exit 36
test "$7" = "-" || exit 37
test -z "${OPENAI_API_KEY+x}" || exit 38
input="$(cat)"
test "$input" = "prompt text" || exit 39
printf '{"type":"item.completed","item":{"type":"agent_message","text":"codex says hello"}}\n'
printf '{"type":"turn.completed","usage":{"input_tokens":4,"cached_input_tokens":1,"output_tokens":2}}\n'"#,
    );

    let output = run_completion(&script, None, None, "prompt text", Duration::from_secs(10))
        .await
        .unwrap();

    assert!(output.stdout.contains(r#""type":"item.completed""#));
    assert!(output.stdout.contains("codex says hello"));
    assert!(output.stdout.contains(r#""type":"turn.completed""#));
    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_completion_uses_configured_working_directory() {
    let workdir = make_temp_dir("completion_cwd");
    let expected = std::fs::canonicalize(&workdir)
        .unwrap()
        .display()
        .to_string();
    let script = write_script(
        "completion_cwd",
        r#"input="$(cat)"
test "$input" = "prompt text" || exit 39
cwd="$(pwd -P)"
printf '{"type":"item.completed","item":{"type":"agent_message","text":"%s"}}\n' "$cwd"
printf '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}\n'"#,
    );

    let output = run_completion(
        &script,
        Some(&workdir),
        None,
        "prompt text",
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    let parsed = parse::parse_output(&output.stdout).unwrap();

    assert_eq!(parsed.text, expected);
    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&workdir);
}

#[test]
fn parse_output_jsonl_fixture_captures_text_and_usage() {
    let parsed = parse::parse_output(
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"hello "}}
{"type":"item.completed","item":{"type":"agent_message","text":"world"}}
{"type":"turn.completed","usage":{"input_tokens":12,"cached_input_tokens":5,"output_tokens":7}}"#,
    )
    .unwrap();

    assert_eq!(parsed.text, "hello world");
    assert_eq!(parsed.usage.input_tokens, 12);
    assert_eq!(parsed.usage.output_tokens, 7);
    assert_eq!(parsed.usage.cache_read_tokens, 5);
    assert_eq!(parsed.usage.cache_write_tokens, 0);
}

#[tokio::test]
async fn run_completion_with_system_prompt_feeds_stdin() {
    let script = write_script(
        "completion_sys",
        r#"input="$(cat)"
test "$input" = "System:
Be precise.

User:
prompt" || exit 41
printf 'sys ok\n'"#,
    );

    let output = run_completion(
        &script,
        None,
        Some("Be precise."),
        "prompt",
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    assert_eq!(output.stdout, "sys ok\n");
    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_completion_nonzero_exit_with_stderr_captured() {
    let script = write_script(
        "completion_fail",
        r"cat > /dev/null
printf 'not logged in\n' >&2
exit 1",
    );

    let err = run_completion(&script, None, None, "prompt", Duration::from_secs(10))
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("not logged in"));
    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_completion_timeout_returns_error() {
    let script = write_script("completion_sleep", "sleep 30");

    let err = run_completion(&script, None, None, "prompt", Duration::from_millis(100))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("timed out"));
    let _ = fs::remove_file(&script);
}

/// Return `true` if the process with `pid` still exists (Linux-only).
#[cfg(target_os = "linux")]
fn is_process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn run_completion_subprocess_killed_on_future_drop() {
    // WHY(#4884): kill_on_drop ensures the Codex subprocess terminates when
    // the caller's future is dropped (actor cancellation, timeout path, etc.).
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let pid_path = std::env::temp_dir().join(format!(
        "hermeneus_codex_killondrop_{}_{nonce}.txt",
        std::process::id()
    ));
    let pid_path_str = pid_path.display().to_string();
    let script = write_script(
        "kill_on_drop",
        &format!("echo $$ > {pid_path_str}\nsleep 30"),
    );

    let pid_path_clone = pid_path.clone();
    let binary = script.clone();
    let handle = tokio::spawn(async move {
        run_completion(&binary, None, None, "prompt", Duration::from_secs(30)).await
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if pid_path_clone.exists() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for subprocess PID file"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let pid: u32 = fs::read_to_string(&pid_path_clone)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    handle.abort();
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(
        !is_process_alive(pid),
        "Codex subprocess (pid={pid}) should be dead after future drop"
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_file(&pid_path);
}
