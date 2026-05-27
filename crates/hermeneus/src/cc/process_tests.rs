#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use super::*;

/// Linux `ETXTBSY` errno — `execve(2)` returns this when the target file
/// has an open writable descriptor anywhere in the kernel.
const ETXTBSY: i32 = 26;

/// Write a shell script to a unique temp path, make it executable, and
/// verify the path is safe to exec before returning.
///
/// Returns the final script path. The caller is responsible for cleanup
/// (or letting the OS reclaim the temp dir on process exit).
///
/// Defeats the Linux ETXTBSY (errno 26, "Text file busy") race described
/// in forkwright/aletheia#3723 with two layers of defense:
///
/// 1. Stage the script at a `.tmp` sibling and rename into place. The
///    writer's file descriptor is tied to the tmp dentry and fully
///    released before rename exposes the final inode, so the common
///    case sees the final path land with a zero writer-count.
/// 2. Probe the final path by sacrificially spawning it and killing the
///    child immediately. An exec syscall is the only observer that
///    fires when the inode still has an open writer, so a successful
///    spawn is the definitive signal that the caller's real spawn
///    cannot race. Transient ETXTBSY is swallowed and retried on a
///    short sleep; the loop caps so a genuinely busy file surfaces an
///    error rather than hanging.
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

/// Build a multi-line stream-json buffer from individual event JSON strings.
fn stream_buf(events: &[&str]) -> Vec<u8> {
    let mut out = String::new();
    for line in events {
        out.push_str(line);
        out.push('\n');
    }
    out.into_bytes()
}

#[tokio::test]
async fn read_stream_assistant_then_result() {
    let buf = stream_buf(&[
        r#"{"type":"system","subtype":"init","session_id":"abc"}"#,
        r#"{"type":"assistant","message":{"type":"text","text":"Hello "}}"#,
        r#"{"type":"assistant","message":{"type":"text","text":"world"}}"#,
        r#"{"type":"result","subtype":"success","result":"Hello world","is_error":false,"session_id":"sess_42","cost_usd":0.002,"duration_ms":1500,"usage":{"input_tokens":12,"output_tokens":3}}"#,
    ]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "Hello world");
    assert!(!output.is_error);
    assert_eq!(output.stream_deltas, vec!["Hello ", "world"]);
    assert_eq!(output.session_id.as_deref(), Some("sess_42"));
    assert_eq!(output.cost_usd, Some(0.002));
    assert_eq!(output.duration_ms, Some(1500));
    let usage = output.usage.unwrap();
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 3);
}

#[tokio::test]
async fn read_stream_result_only() {
    // WHY: CC can emit a result event with no preceding assistant deltas
    // (e.g. when the response is short and CC batches it into the result).
    let buf =
        stream_buf(&[r#"{"type":"result","subtype":"success","result":"ok","is_error":false}"#]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "ok");
    assert!(output.stream_deltas.is_empty());
}

#[tokio::test]
async fn read_stream_no_result_synthesizes_from_deltas() {
    // WHY: If CC exits without emitting a result event we fall back to
    // joining the collected text deltas. This guards graceful degradation.
    let buf = stream_buf(&[
        r#"{"type":"assistant","message":{"type":"text","text":"part1 "}}"#,
        r#"{"type":"assistant","message":{"type":"text","text":"part2"}}"#,
    ]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "part1 part2");
    assert_eq!(output.stream_deltas.len(), 2);
}

#[tokio::test]
async fn read_stream_empty_input_errors() {
    // WHY: No result event AND no deltas is unrecoverable — caller needs to
    // know the subprocess produced nothing usable.
    let buf: Vec<u8> = Vec::new();
    let err = read_stream(buf.as_slice()).await.unwrap_err();
    assert!(
        err.to_string().contains("no result event"),
        "expected 'no result event' in error, got: {err}"
    );
}

#[tokio::test]
async fn read_stream_blank_lines_skipped() {
    // WHY: parse_event treats blank lines as None — read_stream must not
    // crash on them and must not synthesize empty deltas.
    let buf = b"\n\n   \n{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"is_error\":false}\n";
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "ok");
}

#[tokio::test]
async fn read_stream_invalid_json_skipped() {
    // WHY: parse_event returns None on invalid JSON (logged as warning) —
    // read_stream should continue past it to subsequent valid events.
    let buf = stream_buf(&[
        "not json at all",
        r#"{"type":"result","subtype":"success","result":"recovered","is_error":false}"#,
    ]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "recovered");
}

#[tokio::test]
async fn read_stream_with_callback_invokes_for_each_delta() {
    let buf = stream_buf(&[
        r#"{"type":"assistant","message":{"type":"text","text":"a"}}"#,
        r#"{"type":"assistant","message":{"type":"text","text":"b"}}"#,
        r#"{"type":"assistant","message":{"type":"text","text":"c"}}"#,
        r#"{"type":"result","subtype":"success","result":"abc","is_error":false}"#,
    ]);
    let mut collected: Vec<String> = Vec::new();
    {
        let mut on_delta = |s: &str| collected.push(s.to_string());
        let output = read_stream_with_callback(buf.as_slice(), &mut on_delta)
            .await
            .unwrap();
        assert_eq!(output.result_text, "abc");
        assert_eq!(output.stream_deltas, vec!["a", "b", "c"]);
    }
    assert_eq!(collected, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn read_stream_with_callback_skips_empty_text() {
    // WHY: An assistant event with empty text should not invoke the
    // callback (matches read_stream behavior to avoid empty UI updates).
    let buf = stream_buf(&[
        r#"{"type":"assistant","message":{"type":"text","text":""}}"#,
        r#"{"type":"assistant","message":{"type":"text","text":"real"}}"#,
        r#"{"type":"result","subtype":"success","result":"real","is_error":false}"#,
    ]);
    let mut count = 0_u32;
    let mut on_delta = |_: &str| count += 1;
    let _ = read_stream_with_callback(buf.as_slice(), &mut on_delta)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "callback should fire once for the non-empty delta"
    );
}

#[tokio::test]
async fn read_stream_propagates_is_error_flag() {
    // WHY: A `result` event with is_error=true is preserved on the output
    // so the provider layer can map it to a hermeneus::Error variant.
    let buf = stream_buf(&[
        r#"{"type":"result","subtype":"error","result":"rate limit","is_error":true}"#,
    ]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert!(output.is_error);
    assert_eq!(output.result_text, "rate limit");
}

// WHY(#3717): regression — when CC terminates with `subtype = "error_max_turns"`
// (or any error subtype) it omits the `result` field entirely, populating
// `errors` + `terminal_reason` instead. Before the fix, the event failed
// deserialization ("missing field `result`") and the whole turn was
// dropped as pipeline_error. Now we parse it, propagate is_error=true,
// and synthesize a human-readable result_text from terminal_reason +
// errors for downstream error mapping.
#[tokio::test]
async fn read_stream_error_subtype_without_result_field() {
    let buf = stream_buf(&[
        r#"{"type":"result","subtype":"error_max_turns","duration_ms":4065,"is_error":true,"num_turns":2,"session_id":"sess_err","terminal_reason":"max_turns","errors":["Reached maximum number of turns (1)"]}"#,
    ]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert!(output.is_error);
    assert!(
        output.result_text.contains("max_turns"),
        "synthesized text should name the terminal reason: {}",
        output.result_text
    );
    assert!(
        output
            .result_text
            .contains("Reached maximum number of turns (1)"),
        "synthesized text should include the error message: {}",
        output.result_text
    );
    assert_eq!(output.session_id.as_deref(), Some("sess_err"));
}

// WHY(#3717): regression — CC also emits `{"type":"user"}` echo events
// carrying `tool_result` content blocks. These must parse without
// emitting the `unknown variant \`user\`` warning and must not end the
// read_stream loop. Verify by feeding a user event followed by a normal
// result event and checking the result survives.
#[tokio::test]
async fn read_stream_ignores_user_tool_result_event() {
    let buf = stream_buf(&[
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"File not found","is_error":true,"tool_use_id":"toolu_x"}]},"session_id":"s","uuid":"u"}"#,
        r#"{"type":"result","subtype":"success","result":"recovered","is_error":false}"#,
    ]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert!(!output.is_error);
    assert_eq!(output.result_text, "recovered");
}

// ── parse_oauth_token_from_json ───────────────────────────────────────────
// WHY: Tests target the JSON-parsing helper rather than read_oauth_token
// directly. read_oauth_token wraps parse_oauth_token_from_json with I/O
// and HOME resolution. Testing the parser in isolation avoids env var
// manipulation (unsafe in Rust 2024) while covering all key branches.

#[test]
fn parse_oauth_token_succeeds_with_valid_credentials() {
    // WHY: Happy path — valid JSON with the expected key hierarchy returns
    // the access token string without error.
    let json = r#"{"claudeAiOauth":{"accessToken":"test-token-abc123"}}"#;
    let token = parse_oauth_token_from_json(json).unwrap();
    assert_eq!(token, "test-token-abc123");
}

#[test]
fn parse_oauth_token_fails_when_access_token_key_absent() {
    // WHY: JSON exists but lacks the `accessToken` key — must return an
    // error rather than silently returning empty, so callers don't inject
    // a blank token into the subprocess environment.
    let json = r#"{"claudeAiOauth":{"someOtherKey":"value"}}"#;
    let err = parse_oauth_token_from_json(json).unwrap_err();
    assert!(
        err.to_string().contains("no accessToken"),
        "expected 'no accessToken' in error, got: {err}"
    );
}

#[test]
fn parse_oauth_token_fails_when_top_level_key_absent() {
    // WHY: JSON without the `claudeAiOauth` wrapper must fail cleanly —
    // this covers flat credential formats that don't contain CC OAuth data.
    let json = r#"{"someOtherProvider":{"accessToken":"irrelevant"}}"#;
    let err = parse_oauth_token_from_json(json).unwrap_err();
    assert!(
        err.to_string().contains("no accessToken"),
        "expected 'no accessToken' in error, got: {err}"
    );
}

#[test]
fn parse_oauth_token_fails_on_malformed_json() {
    // WHY: Malformed credentials (e.g. truncated write) must return an
    // error, not panic. The caller silently skips OAuth injection on error.
    let err = parse_oauth_token_from_json("not-json{{{").unwrap_err();
    assert!(
        !err.to_string().is_empty(),
        "error message must not be empty"
    );
}

#[test]
fn parse_oauth_token_fails_on_empty_input() {
    let err = parse_oauth_token_from_json("").unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[test]
fn scrub_cc_auth_env_marks_token_env_for_removal() {
    let mut cmd = tokio::process::Command::new("claude");
    cmd.env("ANTHROPIC_AUTH_TOKEN", "raw-oauth-token")
        .env("ANTHROPIC_API_KEY", "raw-api-key")
        .env("CLAUDE_CODE_OAUTH_TOKEN", "raw-cc-token");

    scrub_cc_auth_env(&mut cmd);

    let mut envs: Vec<_> = cmd
        .as_std_mut()
        .get_envs()
        .filter_map(|(key, value)| {
            key.to_str()
                .filter(|name| name.contains("TOKEN") || name.contains("API_KEY"))
                .map(|name| (name.to_owned(), value.map(std::borrow::ToOwned::to_owned)))
        })
        .collect();
    envs.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        envs,
        vec![
            ("ANTHROPIC_API_KEY".to_owned(), None),
            ("ANTHROPIC_AUTH_TOKEN".to_owned(), None),
            ("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), None),
        ],
        "CC subprocesses must remove inherited raw auth tokens"
    );
}

// ── run_completion ────────────────────────────────────────────────────────

#[tokio::test]
async fn run_completion_spawn_failure_reports_binary_path() {
    // WHY: A missing or non-executable binary must produce a ProviderInit
    // error that names the bad path so the operator can diagnose it.
    let binary = PathBuf::from("/nonexistent/path/to/claude-binary");
    let err = run_completion(
        &binary,
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
    // WHY: The claude CLI exposes no max-output-token flag, so a non-zero
    // max_tokens is unenforceable and must be ignored (with a one-time warning),
    // not rejected. Previously this hard-errored, breaking every turn on the
    // default zero-config CC provider. See #4158.
    let script = write_script(
        "completion_max_tokens_ok",
        r#"cat > /dev/null
printf '{"type":"result","subtype":"success","result":"ok","is_error":false}\n'"#,
    );

    let output = run_completion(
        &script,
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
    // WHY: End-to-end subprocess path with a real script. Verifies that
    // run_completion feeds stdin, reads stdout stream-json, and returns
    // a populated CcOutput with the result text and delta list intact.
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
    assert_eq!(output.stream_deltas, vec!["hello ", "world"]);
    assert_eq!(output.session_id.as_deref(), Some("s1"));
    assert_eq!(output.cost_usd, Some(0.001));
    assert_eq!(output.duration_ms, Some(200));

    let _ = fs::remove_file(&script);
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

// ── run_streaming ─────────────────────────────────────────────────────────

#[tokio::test]
async fn run_streaming_spawn_failure_reports_binary_path() {
    // WHY: Mirrors run_completion spawn failure — streaming entry point must
    // also surface the bad binary path in a ProviderInit error.
    let binary = PathBuf::from("/nonexistent/path/to/claude-stream");
    let mut on_delta = |_: &str| {};
    let err = run_streaming(
        &binary,
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
    assert_eq!(output.stream_deltas, vec!["chunk1", "chunk2", "chunk3"]);
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

// ── output size limits (#3324) ───────────────────────────────────────────

#[tokio::test]
async fn read_stream_rejects_oversized_output_by_bytes() {
    // WHY: A CC subprocess that outputs more than MAX_OUTPUT_BYTES must be
    // rejected with a clear error, not allowed to grow unbounded to OOM.
    let big_text = "x".repeat(MAX_OUTPUT_BYTES + 1);
    let event =
        format!(r#"{{"type":"assistant","message":{{"type":"text","text":"{big_text}"}}}}"#);
    let buf = stream_buf(&[&event]);
    let err = read_stream(buf.as_slice()).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("byte limit"),
        "error should mention byte limit, got: {msg}"
    );
}

#[tokio::test]
async fn read_stream_with_callback_rejects_oversized_output() {
    // WHY: The streaming variant must enforce the same size limits.
    let big_text = "x".repeat(MAX_OUTPUT_BYTES + 1);
    let event =
        format!(r#"{{"type":"assistant","message":{{"type":"text","text":"{big_text}"}}}}"#);
    let buf = stream_buf(&[&event]);
    let mut on_delta = |_: &str| {};
    let err = read_stream_with_callback(buf.as_slice(), &mut on_delta)
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("byte limit"),
        "error should mention byte limit, got: {msg}"
    );
}

#[tokio::test]
async fn read_stream_accepts_output_within_limits() {
    // WHY: Output within the byte limit must succeed normally.
    let text = "x".repeat(1000);
    let event = format!(r#"{{"type":"assistant","message":{{"type":"text","text":"{text}"}}}}"#);
    let result_event =
        format!(r#"{{"type":"result","subtype":"success","result":"{text}","is_error":false}}"#);
    let buf = stream_buf(&[&event, &result_event]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, text);
}

#[tokio::test]
async fn run_completion_rejects_oversized_system_prompt() {
    // WHY: A system prompt exceeding MAX_SYSTEM_PROMPT_BYTES must be
    // rejected before spawning the subprocess.
    let big_prompt = "x".repeat(MAX_SYSTEM_PROMPT_BYTES + 1);
    let binary = PathBuf::from("/bin/echo"); // won't be reached
    let err = run_completion(
        &binary,
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
