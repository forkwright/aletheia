//! Shared subprocess execution policy for Organon-owned tools.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use koina::defaults::MAX_OUTPUT_BYTES;

use crate::process_guard::ProcessGuard;
use crate::sandbox::{SandboxConfig, SandboxPolicy};
use crate::types::ToolContext;

/// Environment variables allowed to cross from Aletheia into child processes.
///
/// The parent process holds API keys (`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`,
/// `ALETHEIA_*`, etc.) in its environment. Child processes must start from an
/// empty environment and receive only variables needed for basic process
/// operation.
const SAFE_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "TERM",
    "LANG",
    "LC_ALL",
    "USER",
    "SHELL",
    "TMPDIR",
    "XDG_RUNTIME_DIR",
    "TZ",
];

const DEFAULT_SUBPROCESS_TIMEOUT: Duration = Duration::from_mins(1);

#[cfg(test)]
pub(crate) static SUBPROCESS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One subprocess invocation.
#[derive(Debug, Clone)]
pub struct SubprocessRequest {
    program: OsString,
    args: Vec<OsString>,
    current_dir: PathBuf,
    stdin: Option<Vec<u8>>,
    timeout: Duration,
    max_output_bytes: usize,
    extra_read_paths: Vec<PathBuf>,
    extra_write_paths: Vec<PathBuf>,
    extra_exec_paths: Vec<PathBuf>,
    extra_env_vars: Vec<&'static str>,
    sandbox_policy: Option<SandboxPolicy>,
}

impl SubprocessRequest {
    /// Create a request for `program` running from `current_dir`.
    pub fn new(program: impl Into<OsString>, current_dir: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: current_dir.into(),
            stdin: None,
            timeout: DEFAULT_SUBPROCESS_TIMEOUT,
            max_output_bytes: MAX_OUTPUT_BYTES,
            extra_read_paths: Vec::new(),
            extra_write_paths: Vec::new(),
            extra_exec_paths: Vec::new(),
            extra_env_vars: Vec::new(),
            sandbox_policy: None,
        }
    }

    /// Add one command argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add many command arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Write bytes to the child process stdin and then close it.
    #[must_use]
    pub fn stdin_bytes(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }

    /// Set the wall-clock timeout.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the maximum stored bytes for each output stream.
    #[must_use]
    pub fn max_output_bytes(mut self, max_output_bytes: usize) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }

    /// Add a read-only path to the sandbox policy for this invocation.
    #[must_use]
    pub fn allow_read_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.extra_read_paths.push(path.into());
        self
    }

    /// Add a read-write path to the sandbox policy for this invocation.
    #[must_use]
    pub fn allow_write_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.extra_write_paths.push(path.into());
        self
    }

    /// Add an executable path to the sandbox policy for this invocation.
    #[must_use]
    pub fn allow_exec_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.extra_exec_paths.push(path.into());
        self
    }

    /// Preserve one additional environment variable from the parent process.
    ///
    /// Child environments are cleared by default. Use this only for narrow,
    /// non-secret variables a subprocess genuinely needs.
    #[must_use]
    pub fn allow_env_var(mut self, var: &'static str) -> Self {
        self.extra_env_vars.push(var);
        self
    }

    /// Preserve additional environment variables from the parent process.
    ///
    /// Child environments are cleared by default. Use this only for narrow,
    /// non-secret variables a subprocess genuinely needs.
    #[must_use]
    pub fn allow_env_vars<I>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = &'static str>,
    {
        self.extra_env_vars.extend(vars);
        self
    }

    /// Use an explicit sandbox policy for this invocation.
    ///
    /// The shared runner still applies environment clearing, resource limits,
    /// process-group cleanup, output caps, and timeout handling. This override
    /// is for tools whose filesystem policy cannot be expressed by the
    /// workspace-root default.
    #[must_use]
    pub fn sandbox_policy(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox_policy = Some(policy);
        self
    }

    #[cfg(all(test, feature = "computer-use"))]
    pub(crate) fn allowed_env_vars_for_test(&self) -> &[&'static str] {
        &self.extra_env_vars
    }

    #[cfg(all(test, feature = "computer-use"))]
    pub(crate) fn explicit_sandbox_policy_for_test(&self) -> Option<&SandboxPolicy> {
        self.sandbox_policy.as_ref()
    }

    #[cfg(all(test, feature = "computer-use"))]
    pub(crate) fn program_for_test(&self) -> &OsString {
        &self.program
    }

    #[cfg(all(test, feature = "computer-use"))]
    pub(crate) fn args_for_test(&self) -> &[OsString] {
        &self.args
    }

    #[cfg(all(test, feature = "computer-use"))]
    pub(crate) fn timeout_for_test(&self) -> Duration {
        self.timeout
    }

    #[cfg(all(test, feature = "computer-use"))]
    pub(crate) fn max_output_bytes_for_test(&self) -> usize {
        self.max_output_bytes
    }
}

/// Output captured from a subprocess.
#[derive(Debug, Clone)]
pub struct SubprocessOutput {
    /// Process exit code, or `-1` when the platform did not provide one.
    pub exit_code: i32,
    /// Captured stdout, bounded by the request limit.
    pub stdout: String,
    /// Captured stderr, bounded by the request limit.
    pub stderr: String,
    /// Wall-clock duration of the subprocess.
    pub duration: Duration,
}

/// Failure before normal process completion.
#[derive(Debug)]
pub enum SubprocessError {
    /// Sandbox setup failed before the process was spawned.
    SandboxSetup(std::io::Error),
    /// Process spawn failed.
    Spawn(std::io::Error),
    /// Writing stdin failed.
    Stdin(std::io::Error),
    /// Waiting for the process failed.
    Wait(std::io::Error),
    /// The process exceeded its wall-clock timeout.
    Timeout(Duration),
}

impl std::fmt::Display for SubprocessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SandboxSetup(e) => write!(f, "sandbox setup failed: {e}"),
            Self::Spawn(e) => write!(f, "spawn failed: {e}"),
            Self::Stdin(e) => write!(f, "failed to write tool input: {e}"),
            Self::Wait(e) => write!(f, "wait failed: {e}"),
            Self::Timeout(timeout) => {
                write!(f, "command timed out after {}ms", timeout.as_millis())
            }
        }
    }
}

impl std::error::Error for SubprocessError {}

/// Applies the shared child-process policy used by Organon built-ins.
#[derive(Debug, Clone)]
pub struct SubprocessRunner {
    sandbox: SandboxConfig,
}

impl SubprocessRunner {
    /// Create a runner with the given sandbox configuration.
    #[must_use]
    pub fn new(sandbox: SandboxConfig) -> Self {
        Self { sandbox }
    }

    /// Run a subprocess with cleared environment, resource limits, sandbox, and timeout.
    ///
    /// # Errors
    ///
    /// Returns [`SubprocessError`] when sandbox setup, spawn, stdin, wait, or timeout fails.
    pub fn run(
        &self,
        request: SubprocessRequest,
        ctx: &ToolContext,
    ) -> Result<SubprocessOutput, SubprocessError> {
        let start = Instant::now();
        let mut cmd = Command::new(&request.program);
        cmd.args(&request.args)
            .current_dir(&request.current_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        cmd.env_clear();
        for &var in SAFE_ENV_VARS {
            copy_env_var(&mut cmd, var);
        }
        for &var in &request.extra_env_vars {
            copy_env_var(&mut cmd, var);
        }

        apply_resource_limits(&mut cmd, self.sandbox.nproc_limit, self.sandbox.enforcement);
        isolate_process_group(&mut cmd);

        if let Some(policy) = self.policy_for_request(ctx, &request) {
            crate::sandbox::apply_sandbox(&mut cmd, policy)
                .map_err(SubprocessError::SandboxSetup)?;
        }

        let child = cmd.spawn().map_err(SubprocessError::Spawn)?;
        let mut guard = ProcessGuard::new(child);

        let stdout_reader = guard
            .get_mut()
            .stdout
            .take()
            .map(|pipe| spawn_bounded_reader(pipe, request.max_output_bytes));
        let stderr_reader = guard
            .get_mut()
            .stderr
            .take()
            .map(|pipe| spawn_bounded_reader(pipe, request.max_output_bytes));

        if let Some(stdin_bytes) = request.stdin
            && let Some(mut stdin) = guard.get_mut().stdin.take()
            && let Err(e) = stdin.write_all(&stdin_bytes)
        {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                kill_and_wait(&mut guard);
                let _ = collect_reader(stdout_reader);
                let _ = collect_reader(stderr_reader);
                return Err(SubprocessError::Stdin(e));
            }
            tracing::debug!("subprocess closed stdin before all input was written");
        }

        let deadline = Instant::now() + request.timeout;
        let status = loop {
            match guard.get_mut().try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        kill_and_wait(&mut guard);
                        let _ = collect_reader(stdout_reader);
                        let _ = collect_reader(stderr_reader);
                        return Err(SubprocessError::Timeout(request.timeout));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    kill_and_wait(&mut guard);
                    let _ = collect_reader(stdout_reader);
                    let _ = collect_reader(stderr_reader);
                    return Err(SubprocessError::Wait(e));
                }
            }
        };

        let stdout = collect_reader(stdout_reader);
        let stderr = collect_reader(stderr_reader);
        let exit_code = status.code().unwrap_or(-1);
        #[expect(
            clippy::zombie_processes,
            reason = "process already reaped via try_wait in poll loop above"
        )]
        let _ = guard.detach();

        Ok(SubprocessOutput {
            exit_code,
            stdout,
            stderr,
            duration: start.elapsed(),
        })
    }

    fn build_policy(
        &self,
        ctx: &ToolContext,
        request: &SubprocessRequest,
    ) -> crate::sandbox::SandboxPolicy {
        let mut sandbox = self.sandbox.clone();
        sandbox
            .extra_read_paths
            .extend(request.extra_read_paths.iter().cloned());
        sandbox
            .extra_write_paths
            .extend(request.extra_write_paths.iter().cloned());
        sandbox
            .extra_exec_paths
            .extend(request.extra_exec_paths.iter().cloned());
        sandbox.build_policy(&ctx.workspace, &ctx.allowed_roots)
    }

    fn policy_for_request(
        &self,
        ctx: &ToolContext,
        request: &SubprocessRequest,
    ) -> Option<crate::sandbox::SandboxPolicy> {
        if let Some(policy) = &request.sandbox_policy {
            return Some(policy.clone());
        }
        self.sandbox
            .enabled
            .then(|| self.build_policy(ctx, request))
    }
}

fn copy_env_var(cmd: &mut Command, var: &'static str) {
    if var.is_empty() || var.as_bytes().contains(&b'=') || var.as_bytes().contains(&0) {
        tracing::warn!(
            variable = var,
            "ignoring invalid subprocess env allowlist entry"
        );
        return;
    }
    if let Some(val) = std::env::var_os(var) {
        cmd.env(var, val);
    }
}

#[cfg(target_os = "linux")]
fn apply_resource_limits(
    cmd: &mut Command,
    nproc_limit: u32,
    enforcement: crate::sandbox::SandboxEnforcement,
) {
    use std::os::unix::process::CommandExt as _;

    let nproc_cap = u64::from(nproc_limit);
    let enforcing = enforcement == crate::sandbox::SandboxEnforcement::Enforcing;

    #[expect(
        unsafe_code,
        reason = "pre_exec requires unsafe; setrlimit is async-signal-safe"
    )]
    unsafe {
        cmd.pre_exec(move || {
            use rustix::process::{Resource, Rlimit, setrlimit};

            let nproc_limit = Rlimit {
                current: Some(nproc_cap),
                maximum: Some(nproc_cap),
            };
            // WHY: Under enforcing policy, resource limits are safety controls
            // and must succeed. Under permissive policy, failures are logged
            // but do not block execution.
            if let Err(e) = setrlimit(Resource::Nproc, nproc_limit)
                && enforcing
            {
                return Err(std::io::Error::other(format!(
                    "setrlimit(RLIMIT_NPROC) failed: {e}"
                )));
            }

            let cpu_limit = Rlimit {
                current: Some(60),
                maximum: Some(60),
            };
            if let Err(e) = setrlimit(Resource::Cpu, cpu_limit)
                && enforcing
            {
                return Err(std::io::Error::other(format!(
                    "setrlimit(RLIMIT_CPU) failed: {e}"
                )));
            }

            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_resource_limits(
    _cmd: &mut Command,
    _nproc_limit: u32,
    _enforcement: crate::sandbox::SandboxEnforcement,
) {
}

#[cfg(unix)]
fn isolate_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    cmd.process_group(0);
}

#[cfg(not(unix))]
fn isolate_process_group(_cmd: &mut Command) {}

fn kill_and_wait(guard: &mut ProcessGuard) {
    kill_process_group(guard.get_mut());
    let _ = guard.get_mut().kill();
    let _ = guard.get_mut().wait();
}

#[cfg(target_os = "linux")]
fn kill_process_group(child: &std::process::Child) {
    let pid = rustix::process::Pid::from_child(child);
    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
}

#[cfg(not(target_os = "linux"))]
fn kill_process_group(_child: &std::process::Child) {}

fn spawn_bounded_reader<R>(mut reader: R, max_bytes: usize) -> JoinHandle<String>
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut stored = Vec::new();
        let mut truncated = false;
        let mut buf = [0_u8; 8192];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let remaining = max_bytes.saturating_sub(stored.len());
                    if remaining > 0 {
                        let keep = remaining.min(n);
                        if let Some(chunk) = buf.get(..keep) {
                            stored.extend_from_slice(chunk);
                        }
                    }
                    if n > remaining {
                        truncated = true;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }

        let mut text = String::from_utf8_lossy(&stored).into_owned();
        if truncated {
            trim_to_char_boundary(&mut text, max_bytes);
            text.push_str("\n[output truncated]");
        }
        text
    })
}

fn collect_reader(reader: Option<JoinHandle<String>>) -> String {
    let Some(reader) = reader else {
        return String::new();
    };

    if let Ok(output) = reader.join() {
        output
    } else {
        tracing::warn!("subprocess output reader panicked");
        String::new()
    }
}

fn trim_to_char_boundary(output: &mut String, max_bytes: usize) {
    if output.len() <= max_bytes {
        return;
    }

    let mut end = max_bytes;
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    output.truncate(end);
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId};

    use super::*;

    fn test_ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: dir.to_path_buf(),
            allowed_roots: vec![dir.to_path_buf()],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn test_runner() -> SubprocessRunner {
        SubprocessRunner::new(SandboxConfig {
            enabled: false,
            nproc_limit: 4096,
            ..SandboxConfig::default()
        })
    }

    #[test]
    fn runner_strips_sensitive_environment() {
        let _guard = SUBPROCESS_ENV_LOCK.lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tmpdir");
        #[expect(
            unsafe_code,
            reason = "set_var requires unsafe in Rust 2024; test controls env"
        )]
        unsafe {
            std::env::set_var("ALETHEIA_TOKEN", "runner-secret");
        }

        let output = test_runner()
            .run(
                SubprocessRequest::new("sh", dir.path())
                    .args(["-c", "printf '%s' \"${ALETHEIA_TOKEN-unset}\""]),
                &test_ctx(dir.path()),
            )
            .expect("run");

        #[expect(
            unsafe_code,
            reason = "remove_var requires unsafe in Rust 2024; test cleanup"
        )]
        unsafe {
            std::env::remove_var("ALETHEIA_TOKEN");
        }

        assert_eq!(output.stdout, "unset", "sensitive env must be stripped");
    }

    #[test]
    fn runner_preserves_request_allowed_environment_only() {
        let _guard = SUBPROCESS_ENV_LOCK.lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tmpdir");
        #[expect(
            unsafe_code,
            reason = "set_var requires unsafe in Rust 2024; test controls env"
        )]
        unsafe {
            std::env::set_var("DISPLAY", ":77");
            std::env::set_var("WAYLAND_DISPLAY", "wayland-test");
            std::env::set_var("XAUTHORITY", "/tmp/test-xauthority");
            std::env::set_var("ALETHEIA_TOKEN", "runner-secret");
        }

        let output = test_runner()
            .run(
                SubprocessRequest::new("sh", dir.path())
                    .args([
                        "-c",
                        "printf '%s|%s|%s|%s' \
                         \"${DISPLAY-unset}\" \
                         \"${WAYLAND_DISPLAY-unset}\" \
                         \"${XAUTHORITY-unset}\" \
                         \"${ALETHEIA_TOKEN-unset}\"",
                    ])
                    .allow_env_vars(["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"]),
                &test_ctx(dir.path()),
            )
            .expect("run");

        #[expect(
            unsafe_code,
            reason = "remove_var requires unsafe in Rust 2024; test cleanup"
        )]
        unsafe {
            std::env::remove_var("DISPLAY");
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::remove_var("XAUTHORITY");
            std::env::remove_var("ALETHEIA_TOKEN");
        }

        assert_eq!(
            output.stdout, ":77|wayland-test|/tmp/test-xauthority|unset",
            "request allowlist should preserve display vars without leaking secrets"
        );
    }

    #[test]
    fn runner_times_out_and_kills_process() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let err = test_runner()
            .run(
                SubprocessRequest::new("sh", dir.path())
                    .args(["-c", "while :; do :; done"])
                    .timeout(Duration::from_millis(50)),
                &test_ctx(dir.path()),
            )
            .expect_err("timeout");

        assert!(
            matches!(err, SubprocessError::Timeout(_)),
            "expected timeout error"
        );
    }

    #[test]
    fn runner_allows_child_to_ignore_stdin() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let output = test_runner()
            .run(
                SubprocessRequest::new("sh", dir.path())
                    .args(["-c", "printf done"])
                    .stdin_bytes(vec![b'x'; 128 * 1024]),
                &test_ctx(dir.path()),
            )
            .expect("run");

        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, "done");
    }

    #[test]
    fn runner_bounds_stdout_while_draining_pipe() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let output = test_runner()
            .run(
                SubprocessRequest::new("sh", dir.path())
                    .args([
                        "-c",
                        "i=0; while [ $i -lt 20000 ]; do printf x; i=$((i + 1)); done",
                    ])
                    .max_output_bytes(32),
                &test_ctx(dir.path()),
            )
            .expect("run");

        assert!(
            output.stdout.ends_with("[output truncated]"),
            "large output should be marked as truncated"
        );
        assert_eq!(output.exit_code, 0, "command should complete");
    }

    #[test]
    fn runner_drains_both_streams_past_pipe_capacity() {
        // WHY: Linux pipe buffers are typically 64 KiB. A child that writes
        // more than that to stdout or stderr without the parent draining
        // concurrently will fill the pipe and deadlock before it can exit.
        // This exercises the real SubprocessRunner path used by the `exec`
        // and git tools.
        let dir = tempfile::tempdir().expect("tmpdir");
        let stream_len = 256 * 1024;
        let max_output = 32 * 1024;
        let cmd = format!("yes x | head -c {stream_len} & yes y | head -c {stream_len} >&2; wait");

        let output = test_runner()
            .run(
                SubprocessRequest::new("sh", dir.path())
                    .args(["-c", &cmd])
                    .max_output_bytes(max_output),
                &test_ctx(dir.path()),
            )
            .expect("child should exit instead of deadlocking on a full pipe");

        assert_eq!(output.exit_code, 0, "child should exit cleanly");
        assert!(
            output.stdout.ends_with("[output truncated]"),
            "stdout should be bounded and marked truncated"
        );
        assert!(
            output.stderr.ends_with("[output truncated]"),
            "stderr should be bounded and marked truncated"
        );
    }
}
