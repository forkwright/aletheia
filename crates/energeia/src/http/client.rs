//! `HttpEngine`: subprocess-based implementation of [`DispatchEngine`].
//!
//! Named `HttpEngine` because the trait targets the Anthropic Agent SDK HTTP/SSE
//! API. The current implementation uses the Claude CLI subprocess as a transport
//! (matching phronesis's approach) because the Agent SDK HTTP endpoints are not
//! yet publicly documented. The [`DispatchEngine`] trait boundary insulates
//! callers from this implementation detail.

use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;

use tokio::process::Command;

use crate::engine::{AgentOptions, DispatchEngine, SessionHandle, SessionSpec};
use crate::error::{self, Result};
use crate::http::session::ProcessSessionHandle;
use crate::http::stream::EventStream;

// ---------------------------------------------------------------------------
// HttpEngine
// ---------------------------------------------------------------------------

/// Subprocess-based dispatch engine targeting the Claude CLI.
///
/// Spawns `claude --output-format stream-json` subprocesses and streams NDJSON
/// events. Will be replaced by a direct HTTP/SSE client when the Anthropic
/// Agent SDK HTTP endpoints are publicly available.
pub struct HttpEngine {
    /// Default model identifier (e.g., "claude-sonnet-4-20250514").
    default_model: String,
    /// Path to the claude CLI binary.
    binary: String,
}

impl HttpEngine {
    /// Create a new engine with the given default model.
    #[must_use]
    pub fn new(default_model: impl Into<String>) -> Self {
        Self {
            default_model: default_model.into(),
            binary: "claude".to_owned(),
        }
    }

    /// Override the CLI binary path (for testing with mock scripts).
    #[cfg(test)]
    #[must_use]
    pub(crate) fn binary(mut self, path: impl Into<String>) -> Self {
        self.binary = path.into();
        self
    }

    /// Build the CLI argument vector for a session.
    fn build_args(&self, options: &AgentOptions) -> Vec<String> {
        let mut args = Vec::new();

        args.extend(["--output-format".to_owned(), "stream-json".to_owned()]);
        args.push("--verbose".to_owned());

        let model = options.model.as_deref().unwrap_or(&self.default_model);
        args.extend(["--model".to_owned(), model.to_owned()]);

        if let Some(ref mode) = options.permission_mode {
            args.extend(["--permission-mode".to_owned(), mode.clone()]);
        }

        if let Some(ref prompt) = options.system_prompt {
            args.extend(["--system-prompt".to_owned(), prompt.clone()]);
        }

        if let Some(turns) = options.max_turns {
            args.extend(["--max-turns".to_owned(), turns.to_string()]);
        }

        args
    }

    /// Spawn a subprocess and return a session handle.
    fn launch(mut cmd: Command) -> Result<ProcessSessionHandle> {
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // INVARIANT: kill_on_drop ensures cleanup if the handle is dropped
        // without explicit wait/abort.
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            error::EngineSnafu {
                detail: format!("failed to spawn claude subprocess: {e}"),
            }
            .build()
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            error::EngineSnafu {
                detail: "stdout not captured from subprocess",
            }
            .build()
        })?;

        let stream = EventStream::new(stdout);
        Ok(ProcessSessionHandle::new(child, stream, String::new()))
    }
}

impl DispatchEngine for HttpEngine {
    fn spawn_session<'a>(
        &'a self,
        spec: &'a SessionSpec,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        Box::pin(async move {
            let mut cmd = Command::new(&self.binary);
            cmd.args(["-p", &spec.prompt]);
            cmd.args(self.build_args(options));

            // WHY: cwd is set on the Command, not as a CLI flag.
            let cwd = spec.cwd.as_ref().or(options.cwd.as_ref());
            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }

            if let Some(ref sys) = spec.system_prompt
                && options.system_prompt.is_none()
            {
                cmd.args(["--system-prompt", sys]);
            }

            tracing::debug!(
                prompt_len = spec.prompt.len(),
                model = options.model.as_deref().unwrap_or(&self.default_model),
                "spawning session"
            );

            let handle = Self::launch(cmd)?;
            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }

    fn resume_session<'a>(
        &'a self,
        session_id: &'a str,
        prompt: &'a str,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        Box::pin(async move {
            let mut cmd = Command::new(&self.binary);
            cmd.args(["--resume", session_id, "-p", prompt]);
            cmd.args(self.build_args(options));

            if let Some(ref dir) = options.cwd {
                cmd.current_dir(dir);
            }

            tracing::debug!(session_id, prompt_len = prompt.len(), "resuming session");

            let handle = Self::launch(cmd)?;
            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions and helpers"
)]
mod tests {
    use std::fmt::Write;

    use super::*;
    use crate::engine::SessionEvent;

    #[test]
    fn build_args_default_model() {
        let engine = HttpEngine::new("claude-sonnet-4-20250514");
        let options = AgentOptions::new();
        let args = engine.build_args(&options);

        assert!(args.contains(&"--output-format".to_owned()));
        assert!(args.contains(&"stream-json".to_owned()));
        assert!(args.contains(&"--model".to_owned()));
        assert!(args.contains(&"claude-sonnet-4-20250514".to_owned()));
    }

    #[test]
    fn build_args_option_overrides() {
        let engine = HttpEngine::new("default-model");
        let options = AgentOptions::new()
            .model("claude-opus-4-6")
            .max_turns(50)
            .permission_mode("bypassPermissions");
        let args = engine.build_args(&options);

        assert!(args.contains(&"claude-opus-4-6".to_owned()));
        assert!(args.contains(&"--max-turns".to_owned()));
        assert!(args.contains(&"50".to_owned()));
        assert!(args.contains(&"--permission-mode".to_owned()));
        assert!(args.contains(&"bypassPermissions".to_owned()));
    }

    #[test]
    fn build_args_omits_unset_options() {
        let engine = HttpEngine::new("m");
        let options = AgentOptions::new();
        let args = engine.build_args(&options);

        assert!(!args.contains(&"--system-prompt".to_owned()));
        assert!(!args.contains(&"--max-turns".to_owned()));
        assert!(!args.contains(&"--permission-mode".to_owned()));
    }

    /// Helper: create a mock script that emits NDJSON lines and exits.
    fn create_mock_script(
        dir: &std::path::Path,
        stdout_lines: &[&str],
        exit_code: i32,
    ) -> std::path::PathBuf {
        let script_path = dir.join("claude");
        let mut content = String::from("#!/bin/sh\n");
        for line in stdout_lines {
            let _ = writeln!(content, "echo '{line}'");
        }
        let _ = writeln!(content, "exit {exit_code}");
        std::fs::write(&script_path, &content).expect("mock script should be writable");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                .expect("mock script should be chmod-able");
        }

        script_path
    }

    fn mock_engine(dir: &std::path::Path) -> HttpEngine {
        HttpEngine::new("test-model").binary(dir.join("claude").display().to_string())
    }

    fn sample_session_spec(prompt: &str) -> SessionSpec {
        SessionSpec {
            prompt: prompt.to_owned(),
            system_prompt: None,
            cwd: None,
        }
    }

    // WHY: single-threaded runtime prevents non-deterministic event ordering
    // when workspace tests run in parallel and CPU pressure delays subprocess I/O.
    #[tokio::test(flavor = "current_thread")]
    async fn spawn_session_successful() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lines = &[
            r#"{"type":"system","subtype":"init","session_id":"sess-001"}"#,
            r#"{"type":"assistant","content":[{"type":"text","text":"Hello"}]}"#,
            r#"{"type":"result","session_id":"sess-001","subtype":"success","is_error":false,"total_cost_usd":0.10,"num_turns":3,"duration_ms":5000,"result":"Done"}"#,
        ];
        create_mock_script(tmp.path(), lines, 0);

        let engine = mock_engine(tmp.path());
        let handle = engine
            .spawn_session(&sample_session_spec("test prompt"), &AgentOptions::new())
            .await
            .unwrap();
        let result = handle.wait().await.unwrap();

        assert_eq!(result.session_id, "sess-001");
        assert!(result.success);
        assert_eq!(result.num_turns, 3);
        assert!((result.cost_usd - 0.10).abs() < f64::EPSILON);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_session_error_result() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lines = &[
            r#"{"type":"system","subtype":"init","session_id":"sess-err"}"#,
            r#"{"type":"result","session_id":"sess-err","subtype":"error_during_execution","is_error":true,"num_turns":1,"duration_ms":500}"#,
        ];
        create_mock_script(tmp.path(), lines, 1);

        let engine = mock_engine(tmp.path());
        let handle = engine
            .spawn_session(&sample_session_spec("bad prompt"), &AgentOptions::new())
            .await
            .unwrap();
        let result = handle.wait().await.unwrap();

        assert!(!result.success);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resume_session_works() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lines = &[
            r#"{"type":"system","subtype":"init","session_id":"sess-resume"}"#,
            r#"{"type":"result","session_id":"sess-resume","subtype":"success","is_error":false,"num_turns":5,"duration_ms":3000}"#,
        ];
        create_mock_script(tmp.path(), lines, 0);

        let engine = mock_engine(tmp.path());
        let handle = engine
            .resume_session("sess-resume", "continue working", &AgentOptions::new())
            .await
            .unwrap();
        let result = handle.wait().await.unwrap();

        assert_eq!(result.session_id, "sess-resume");
        assert!(result.success);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_session_event_streaming() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lines = &[
            r#"{"type":"system","subtype":"init","session_id":"sess-stream"}"#,
            r#"{"type":"assistant","content":[{"type":"text","text":"step 1"}]}"#,
            r#"{"type":"assistant","content":[{"type":"text","text":"step 2"}]}"#,
            r#"{"type":"result","session_id":"sess-stream","subtype":"success","is_error":false,"num_turns":2,"duration_ms":1000}"#,
        ];
        create_mock_script(tmp.path(), lines, 0);

        let engine = mock_engine(tmp.path());
        let mut handle = engine
            .spawn_session(&sample_session_spec("test"), &AgentOptions::new())
            .await
            .unwrap();

        let mut events = Vec::new();
        while let Some(event) = handle.next_event().await {
            events.push(event);
        }

        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SessionEvent::TextDelta { text } if text == "step 1"));
        assert!(matches!(&events[1], SessionEvent::TextDelta { text } if text == "step 2"));
    }

    #[tokio::test]
    async fn spawn_session_rate_limit_abort() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lines = &[
            r#"{"type":"system","subtype":"init","session_id":"sess-rl"}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"throttled","utilization":0.99}}"#,
        ];
        create_mock_script(tmp.path(), lines, 0);

        let engine = mock_engine(tmp.path());
        let handle = engine
            .spawn_session(&sample_session_spec("test"), &AgentOptions::new())
            .await
            .unwrap();
        let err = handle.wait().await.unwrap_err();

        assert!(err.to_string().contains("budget exceeded"));
        assert!(err.to_string().contains("rate limit"));
    }

    #[tokio::test]
    async fn spawn_session_no_result_message() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lines = &[r#"{"type":"system","subtype":"init","session_id":"sess-no-result"}"#];
        create_mock_script(tmp.path(), lines, 1);

        let engine = mock_engine(tmp.path());
        let handle = engine
            .spawn_session(&sample_session_spec("test"), &AgentOptions::new())
            .await
            .unwrap();
        let err = handle.wait().await.unwrap_err();

        assert!(err.to_string().contains("without emitting a result"));
    }

    #[tokio::test]
    async fn abort_kills_subprocess() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let script_path = tmp.path().join("claude");
        std::fs::write(&script_path, "#!/bin/sh\nsleep 30\n")
            .expect("long-running mock should be writable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                .expect("mock script should be chmod-able");
        }

        let engine = HttpEngine::new("test-model").binary(script_path.display().to_string());
        let mut handle = engine
            .spawn_session(&sample_session_spec("test"), &AgentOptions::new())
            .await
            .unwrap();
        let result = handle.abort().await;
        assert!(result.is_ok());
    }

    #[test]
    fn http_engine_is_send_sync() {
        static_assertions::assert_impl_all!(HttpEngine: Send, Sync);
    }
}
