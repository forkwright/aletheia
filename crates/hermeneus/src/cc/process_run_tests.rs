#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use super::*;

/// Linux `ETXTBSY` errno — `execve(2)` returns this when the target file
/// has an open writable descriptor anywhere in the kernel.
const ETXTBSY: i32 = 26;

/// WHY: stage the script at a temp sibling, rename into place, and probe
/// the final path so a real spawn cannot race ETXTBSY.
fn write_script(name: &str, body: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let final_path = std::env::temp_dir().join(format!(
        "hermeneus_test_{name}_{}_{nonce}.sh",
        std::process::id()
    ));
    let tmp_path = final_path.with_extension("sh.tmp");
    let script = format!("#!/bin/sh\n{body}\n");
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp_path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        f.sync_all().unwrap();
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
                // Kill + reap immediately; we only care that execve passed.
                let _ = child.kill();
                let _ = child.wait();
                return final_path;
            }
            Err(e) if e.raw_os_error() == Some(ETXTBSY) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                // Any other error (ENOENT, EACCES, …) is not our race —
                // return the path and let the caller surface the error
                // with its own context.
                return final_path;
            }
        }
    }
    final_path
}

fn make_temp_dir(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "hermeneus_test_{name}_{}_{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[tokio::test]
async fn run_completion_spawn_failure_reports_binary_path() {
    // WHY: a missing or non-executable binary must name the bad path.
    let binary = PathBuf::from("/nonexistent/path/to/claude-binary");
    let err = run_completion(
        &binary,
        None,
        "claude-test-model",
        None,
        "hello",
        0,
        Duration::from_secs(5),
    )
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("/nonexistent/path/to/claude-binary"),
        "error must include the binary path, got: {msg}"
    );
    assert!(
        msg.contains("provider init failed"),
        "error must be ProviderInit variant, got: {msg}"
    );
}

#[tokio::test]
async fn run_completion_tolerates_nonzero_max_tokens() {
    // WHY(#4158): `claude` has no max-output-token flag, so non-zero `max_tokens`
    // must be ignored rather than rejected.
    let script = write_script(
        "completion_max_tokens_ok",
        r#"cat > /dev/null
printf '{"type":"result","subtype":"success","result":"ok","is_error":false}\n'"#,
    );

    let output = run_completion(
        &script,
        None,
        "claude-test-model",
        None,
        "hello",
        1024,
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    assert_eq!(output.result_text, "ok");
    assert!(!output.is_error);

    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_completion_success_collects_output() {
    // WHY: end-to-end subprocess path verifies stdin, stdout, and delta capture.
    let script = write_script(
        "completion_ok",
        // Discard all args and stdin; emit a two-event stream.
        r#"cat > /dev/null
printf '{"type":"assistant","message":{"type":"text","text":"hello "}}\n'
printf '{"type":"assistant","message":{"type":"text","text":"world"}}\n'
printf '{"type":"result","subtype":"success","result":"hello world","is_error":false,"session_id":"s1","cost_usd":0.001,"duration_ms":200}\n'"#,
    );

    let output = run_completion(
        &script,
        None,
        "claude-test-model",
        None,
        "prompt text",
        0,
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    assert_eq!(output.result_text, "hello world");
    assert!(!output.is_error);
    assert_eq!(output.session_id.as_deref(), Some("s1"));
    assert_eq!(output.cost_usd, Some(0.001));
    assert_eq!(output.duration_ms, Some(200));

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
        r#"cat > /dev/null
cwd="$(pwd -P)"
printf '{"type":"result","subtype":"success","result":"%s","is_error":false}\n' "$cwd""#,
    );

    let output = run_completion(
        &script,
        Some(&workdir),
        "claude-test-model",
        None,
        "prompt text",
        0,
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    assert_eq!(output.result_text, expected);
    let _ = fs::remove_file(&script);
    let _ = fs::remove_dir_all(&workdir);
}

#[tokio::test]
async fn run_completion_with_system_prompt_succeeds() {
    // WHY: Verifies the --system-prompt branch executes without error.
    // The actual arg passing is structural (cmd.arg) and not visible from
    // outside the subprocess, but the round-trip proves the branch is taken.
    let script = write_script(
        "completion_sys",
        r#"cat > /dev/null
printf '{"type":"result","subtype":"success","result":"sys ok","is_error":false}\n'"#,
    );

    let output = run_completion(
        &script,
        None,
        "claude-test-model",
        Some("You are a helpful assistant."),
        "prompt",
        0,
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    assert_eq!(output.result_text, "sys ok");
    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_completion_timeout_returns_error() {
    // WHY: Subprocess that sleeps past the deadline must be killed and
    // must surface a timeout error message that includes the duration.
    let script = write_script("completion_sleep", "sleep 30");

    let err = run_completion(
        &script,
        None,
        "claude-test-model",
        None,
        "prompt",
        0,
        Duration::from_millis(100),
    )
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("timed out"),
        "error must mention timeout, got: {msg}"
    );
    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_completion_nonzero_exit_with_stderr_captured() {
    // WHY: When CC emits a result event with empty result text and then exits
    // nonzero, run_completion falls through to the stderr-capture branch
    // (`!status.success() && result_text.is_empty()`). The captured stderr
    // must appear in the error so the operator can see the failure reason
    // (e.g. "not logged in", "invalid model").
    //
    // The script emits a result event with an empty result string so that
    // read_stream returns Ok(CcOutput { result_text: "", ... }), which
    // triggers the stderr-capture branch when the exit code is nonzero.
    let script = write_script(
        "completion_fail",
        r#"cat > /dev/null
printf '{"type":"result","subtype":"error","result":"","is_error":true}\n'
printf 'OAuth token rejected\n' >&2
exit 1"#,
    );

    let err = run_completion(
        &script,
        None,
        "claude-test-model",
        None,
        "prompt",
        0,
        Duration::from_secs(10),
    )
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("OAuth token rejected"),
        "stderr must appear in error message, got: {msg}"
    );
    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_streaming_spawn_failure_reports_binary_path() {
    // WHY: Mirrors run_completion spawn failure — streaming entry point must
    // also surface the bad binary path in a ProviderInit error.
    let binary = PathBuf::from("/nonexistent/path/to/claude-stream");
    let mut on_delta = |_: &str| {};
    let err = run_streaming(
        &binary,
        None,
        "claude-test-model",
        None,
        "hello",
        0,
        Duration::from_secs(5),
        &mut on_delta,
    )
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("/nonexistent/path/to/claude-stream"),
        "error must include binary path, got: {msg}"
    );
    assert!(
        msg.contains("provider init failed"),
        "error must be ProviderInit variant, got: {msg}"
    );
}

#[tokio::test]
async fn run_streaming_tolerates_nonzero_max_tokens() {
    // WHY: As with run_completion, a non-zero max_tokens is unenforceable by the
    // claude CLI and must be ignored rather than rejected, so streamed turns run
    // on the default CC provider. See #4158.
    let script = write_script(
        "streaming_max_tokens_ok",
        r#"cat > /dev/null
printf '{"type":"assistant","message":{"type":"text","text":"hi"}}\n'
printf '{"type":"result","subtype":"success","result":"hi","is_error":false}\n'"#,
    );

    let mut collected: Vec<String> = Vec::new();
    let mut on_delta = |s: &str| collected.push(s.to_owned());

    let output = run_streaming(
        &script,
        None,
        "claude-test-model",
        None,
        "hello",
        2048,
        Duration::from_secs(10),
        &mut on_delta,
    )
    .await
    .unwrap();

    assert_eq!(output.result_text, "hi");
    assert_eq!(collected, vec!["hi"]);

    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_streaming_invokes_callback_for_each_delta() {
    // WHY: run_streaming must call on_delta once per assistant text event,
    // in order, with the exact text. This is the primary contract of the
    // streaming API — callers relay deltas to UI/SSE consumers in real time.
    let script = write_script(
        "streaming_deltas",
        r#"cat > /dev/null
printf '{"type":"assistant","message":{"type":"text","text":"chunk1"}}\n'
printf '{"type":"assistant","message":{"type":"text","text":"chunk2"}}\n'
printf '{"type":"assistant","message":{"type":"text","text":"chunk3"}}\n'
printf '{"type":"result","subtype":"success","result":"chunk1chunk2chunk3","is_error":false}\n'"#,
    );

    let mut collected: Vec<String> = Vec::new();
    let mut on_delta = |s: &str| collected.push(s.to_owned());

    let output = run_streaming(
        &script,
        None,
        "claude-test-model",
        None,
        "prompt",
        0,
        Duration::from_secs(10),
        &mut on_delta,
    )
    .await
    .unwrap();

    assert_eq!(output.result_text, "chunk1chunk2chunk3");
    assert_eq!(
        collected,
        vec!["chunk1", "chunk2", "chunk3"],
        "on_delta must be called in order with each text delta"
    );

    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_streaming_timeout_returns_error() {
    // WHY: Same timeout contract as run_completion — streaming subprocess
    // that stalls must be killed and must return a timeout error.
    let script = write_script("streaming_sleep", "sleep 30");

    let mut on_delta = |_: &str| {};
    let err = run_streaming(
        &script,
        None,
        "claude-test-model",
        None,
        "prompt",
        0,
        Duration::from_millis(100),
        &mut on_delta,
    )
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("timed out"),
        "error must mention timeout, got: {msg}"
    );
    let _ = fs::remove_file(&script);
}

#[tokio::test]
async fn run_completion_rejects_oversized_system_prompt() {
    // WHY: A system prompt exceeding MAX_SYSTEM_PROMPT_BYTES must be
    // rejected before spawning the subprocess.
    let big_prompt = "x".repeat(MAX_SYSTEM_PROMPT_BYTES + 1);
    let binary = PathBuf::from("/bin/echo"); // won't be reached
    let err = run_completion(
        &binary,
        None,
        "test-model",
        Some(&big_prompt),
        "hello",
        0,
        Duration::from_secs(5),
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("system prompt exceeds maximum size"),
        "error should mention system prompt size, got: {msg}"
    );
}

#[tokio::test]
async fn run_streaming_rejects_oversized_system_prompt() {
    // WHY: The streaming variant must enforce the same system prompt limit.
    let big_prompt = "x".repeat(MAX_SYSTEM_PROMPT_BYTES + 1);
    let binary = PathBuf::from("/bin/echo");
    let mut on_delta = |_: &str| {};
    let err = run_streaming(
        &binary,
        None,
        "test-model",
        Some(&big_prompt),
        "hello",
        0,
        Duration::from_secs(5),
        &mut on_delta,
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("system prompt exceeds maximum size"),
        "error should mention system prompt size, got: {msg}"
    );
}

/// Return `true` if the process with `pid` still exists (Linux-only).
#[cfg(target_os = "linux")]
fn is_process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn run_completion_subprocess_killed_on_future_drop() {
    // WHY(#4884): kill_on_drop guarantees the subprocess terminates when the
    // caller's future is dropped (actor cancellation, timeout, etc.).
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let pid_path = std::env::temp_dir().join(format!(
        "hermeneus_cc_killondrop_{}_{nonce}.txt",
        std::process::id()
    ));
    let pid_path_str = pid_path.display().to_string();
    let script = write_script(
        "kill_on_drop_completion",
        &format!("echo $$ > {pid_path_str}\nsleep 30"),
    );

    let pid_path_clone = pid_path.clone();
    let binary = script.clone();
    let handle = tokio::spawn(async move {
        run_completion(
            &binary,
            None,
            "test-model",
            None,
            "prompt",
            0,
            Duration::from_secs(30),
        )
        .await
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
        "CC completion subprocess (pid={pid}) should be dead after future drop"
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_file(&pid_path);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn run_streaming_subprocess_killed_on_future_drop() {
    // WHY(#4884): streaming path also sets kill_on_drop — verify the contract.
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let pid_path = std::env::temp_dir().join(format!(
        "hermeneus_cc_stream_killondrop_{}_{nonce}.txt",
        std::process::id()
    ));
    let pid_path_str = pid_path.display().to_string();
    let script = write_script(
        "kill_on_drop_streaming",
        &format!("echo $$ > {pid_path_str}\nsleep 30"),
    );

    let pid_path_clone = pid_path.clone();
    let binary = script.clone();
    let handle = tokio::spawn(async move {
        let mut on_delta = |_: &str| {};
        run_streaming(
            &binary,
            None,
            "test-model",
            None,
            "prompt",
            0,
            Duration::from_secs(30),
            &mut on_delta,
        )
        .await
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
        "CC streaming subprocess (pid={pid}) should be dead after future drop"
    );

    let _ = fs::remove_file(&script);
    let _ = fs::remove_file(&pid_path);
}
