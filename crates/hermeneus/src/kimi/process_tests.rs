#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use super::*;

const ETXTBSY: i32 = 26;

fn write_script(name: &str, body: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let final_path = std::env::temp_dir().join(format!(
        "hermeneus_kimi_test_{name}_{}_{nonce}.sh",
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
                if let Err(e) = child.kill() {
                    assert!(
                        e.kind() == std::io::ErrorKind::InvalidInput,
                        "unexpected kill error: {e}"
                    );
                }
                child.wait().unwrap();
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

fn stream_buf(lines: &[&str]) -> Vec<u8> {
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out.into_bytes()
}

#[test]
fn parse_text_part_line_extracts_text() {
    let text = parse::parse_text_part_line(r"TextPart(type='text', text='hello\nworld')").unwrap();
    assert_eq!(text, "hello\nworld");
}

#[test]
fn parse_text_assignment_line_extracts_multiline_text_part_field() {
    let text = parse::parse_text_assignment_line("    text='hello world',").unwrap();
    assert_eq!(text, "hello world");
}

#[test]
fn parse_usage_assignments_fill_usage() {
    let mut usage = KimiUsage::default();
    parse::parse_usage_assignment("input_other=2072,", &mut usage);
    parse::parse_usage_assignment("output=35,", &mut usage);
    parse::parse_usage_assignment("input_cache_read=9216,", &mut usage);
    parse::parse_usage_assignment("input_cache_creation=4", &mut usage);

    assert_eq!(usage.input_other, 2072);
    assert_eq!(usage.output, 35);
    assert_eq!(usage.input_cache_read, 9216);
    assert_eq!(usage.input_cache_creation, 4);
}

#[test]
fn parse_message_id_line_extracts_id() {
    let id = parse::parse_message_id_line("    message_id='chatcmpl_123',").unwrap();
    assert_eq!(id, "chatcmpl_123");
}

#[tokio::test]
async fn read_stream_text_part_and_status_update() {
    let buf = stream_buf(&[
        "prompt echo",
        "TurnBegin(user_input='prompt echo')",
        "StepBegin(n=1)",
        "TextPart(type='text', text='ok')",
        "StatusUpdate(",
        "    token_usage=TokenUsage(",
        "        input_other=2,",
        "        output=1,",
        "        input_cache_read=3,",
        "        input_cache_creation=4",
        "    ),",
        "    message_id='chatcmpl_abc',",
        ")",
        "TurnEnd()",
    ]);

    let output = read_stream(buf.as_slice()).await.unwrap();
    assert_eq!(output.result_text, "ok");
    assert_eq!(output.stream_deltas, vec!["ok"]);
    assert_eq!(output.message_id.as_deref(), Some("chatcmpl_abc"));
    let usage = output.usage.unwrap();
    assert_eq!(usage.input_other, 2);
    assert_eq!(usage.output, 1);
    assert_eq!(usage.input_cache_read, 3);
    assert_eq!(usage.input_cache_creation, 4);
}

#[tokio::test]
async fn read_stream_with_callback_invokes_for_text_parts() {
    let buf = stream_buf(&[
        "TextPart(type='text', text='a')",
        "TextPart(type='text', text='b')",
    ]);
    let mut collected = Vec::new();
    let mut on_delta = |text: &str| collected.push(text.to_owned());
    let output = read_stream_with_callback(buf.as_slice(), &mut on_delta)
        .await
        .unwrap();

    assert_eq!(output.result_text, "ab");
    assert_eq!(collected, vec!["a", "b"]);
}

#[test]
fn build_kimi_command_uses_validated_headless_invocation_and_scrubs_api_key() {
    let cwd = Path::new("/tmp");
    let mut cmd = build_kimi_command(Path::new("/usr/bin/kimi"), cwd, "say ok");
    cmd.env("MOONSHOT_API_KEY", "raw-api-key");
    scrub_kimi_auth_env(&mut cmd);

    let args: Vec<_> = cmd
        .as_std_mut()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        vec![
            "--print",
            "--afk",
            "--yolo",
            "--thinking",
            "-w",
            "/tmp",
            "-p",
            "say ok",
        ]
    );

    let moonshot = cmd
        .as_std_mut()
        .get_envs()
        .find(|(key, _value)| key.to_str() == Some("MOONSHOT_API_KEY"))
        .map(|(_key, value)| value.map(std::borrow::ToOwned::to_owned));
    assert_eq!(moonshot, Some(None));
}

#[tokio::test]
async fn run_completion_uses_stub_kimi_shape() {
    let script = write_script(
        "completion_shape",
        r#"[ "$1" = "--print" ] || exit 2
[ "$2" = "--afk" ] || exit 2
[ "$3" = "--yolo" ] || exit 2
[ "$4" = "--thinking" ] || exit 2
[ "$5" = "-w" ] || exit 2
[ "$7" = "-p" ] || exit 2
[ "$8" = "prompt text" ] || exit 2
printf "TextPart(type='text', text='stub ok')\n"
printf "StatusUpdate(\n"
printf "    token_usage=TokenUsage(\n"
printf "        input_other=1,\n"
printf "        output=2,\n"
printf "        input_cache_read=3,\n"
printf "        input_cache_creation=4\n"
printf "    ),\n"
printf "    message_id='chatcmpl_stub',\n"
printf ")\n""#,
    );

    let output = run_completion(
        &script,
        &std::env::temp_dir(),
        None,
        "prompt text",
        0,
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    assert_eq!(output.result_text, "stub ok");
    assert_eq!(output.message_id.as_deref(), Some("chatcmpl_stub"));
    let usage = output.usage.unwrap();
    assert_eq!(usage.output, 2);

    fs::remove_file(&script).unwrap();
}

#[tokio::test]
async fn run_completion_rejects_max_tokens() {
    let err = run_completion(
        &PathBuf::from("/bin/echo"),
        &std::env::temp_dir(),
        None,
        "prompt",
        100,
        Duration::from_secs(5),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("cannot enforce max_tokens=100"));
}

#[tokio::test]
async fn run_completion_rejects_oversized_system_prompt() {
    let big_prompt = "x".repeat(MAX_SYSTEM_PROMPT_BYTES + 1);
    let err = run_completion(
        &PathBuf::from("/bin/echo"),
        &std::env::temp_dir(),
        Some(&big_prompt),
        "prompt",
        0,
        Duration::from_secs(5),
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("system prompt exceeds maximum size")
    );
}
