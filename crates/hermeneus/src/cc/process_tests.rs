#![expect(clippy::unwrap_used, reason = "test assertions")]

use super::*;

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
    assert_eq!(output.session_id.as_deref(), Some("sess_42"));
    assert_eq!(output.cost_usd, Some(0.002));
    assert_eq!(output.duration_ms, Some(1500));
    let usage = output.usage.unwrap();
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 3);
}

#[tokio::test]
async fn read_stream_result_only() {
    // WHY: CC can emit a result event with no preceding assistant deltas.
    let buf =
        stream_buf(&[r#"{"type":"result","subtype":"success","result":"ok","is_error":false}"#]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "ok");
}

#[tokio::test]
async fn read_stream_no_result_synthesizes_from_deltas() {
    // WHY: If CC exits without a result event, join collected text deltas.
    let buf = stream_buf(&[
        r#"{"type":"assistant","message":{"type":"text","text":"part1 "}}"#,
        r#"{"type":"assistant","message":{"type":"text","text":"part2"}}"#,
    ]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "part1 part2");
}

#[tokio::test]
async fn read_stream_empty_input_errors() {
    // WHY: no result event and no deltas is unrecoverable.
    let buf: Vec<u8> = Vec::new();
    let err = read_stream(buf.as_slice()).await.unwrap_err();
    assert!(
        err.to_string().contains("no result event"),
        "expected 'no result event' in error, got: {err}"
    );
}

#[tokio::test]
async fn read_stream_blank_lines_skipped() {
    // WHY: blank lines parse as `None`, so read_stream must skip them.
    let buf = b"\n\n   \n{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"is_error\":false}\n";
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "ok");
}

#[tokio::test]
async fn read_stream_invalid_json_skipped() {
    // WHY: invalid JSON yields `None`, so read_stream should continue past it.
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
    }
    assert_eq!(collected, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn read_stream_with_callback_skips_empty_text() {
    // WHY: empty assistant text should not invoke the callback.
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
    // WHY: preserve `is_error=true` so the provider layer can map the error.
    let buf = stream_buf(&[
        r#"{"type":"result","subtype":"error","result":"rate limit","is_error":true}"#,
    ]);
    let output = read_stream(buf.as_slice()).await.unwrap();
    assert!(output.is_error);
    assert_eq!(output.result_text, "rate limit");
}

// WHY(#3717): error subtypes omit `result`; parse `errors` and `terminal_reason`
// instead, then synthesize `result_text` for downstream error mapping.
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

// WHY(#3717): CC emits `{"type":"user"}` echo events for `tool_result` blocks;
// accept them so the stream keeps flowing.
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

// WHY: tests target the JSON parser directly so they avoid I/O and env mutation.

#[test]
fn parse_oauth_token_succeeds_with_valid_credentials() {
    // WHY: valid JSON with the expected key hierarchy returns the access token.
    let json = r#"{"claudeAiOauth":{"accessToken":"test-token-abc123"}}"#;
    let token = parse_oauth_token_from_json(json).unwrap();
    assert_eq!(token, "test-token-abc123");
}

#[test]
fn parse_oauth_token_fails_when_access_token_key_absent() {
    // WHY: missing `accessToken` must return an error, not an empty token.
    let json = r#"{"claudeAiOauth":{"someOtherKey":"value"}}"#;
    let err = parse_oauth_token_from_json(json).unwrap_err();
    assert!(
        err.to_string().contains("no accessToken"),
        "expected 'no accessToken' in error, got: {err}"
    );
}

#[test]
fn parse_oauth_token_fails_when_top_level_key_absent() {
    // WHY: missing `claudeAiOauth` must fail cleanly.
    let json = r#"{"someOtherProvider":{"accessToken":"irrelevant"}}"#;
    let err = parse_oauth_token_from_json(json).unwrap_err();
    assert!(
        err.to_string().contains("no accessToken"),
        "expected 'no accessToken' in error, got: {err}"
    );
}

#[test]
fn parse_oauth_token_fails_on_malformed_json() {
    // WHY: malformed credentials must return an error, not panic.
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
