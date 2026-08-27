//! Pack tool registration and shell execution.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr as _;
use std::time::Duration;

use indexmap::IndexMap;
use koina::defaults::MAX_OUTPUT_BYTES;
use koina::id::ToolName;
use organon::registry::{ToolExecutor, ToolRegistry};
use organon::subprocess::{SubprocessError, SubprocessRequest, SubprocessRunner};
use organon::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolDiagnostics, ToolGroupId, ToolInput, ToolResult, ToolTag,
};
use tracing::info;

use crate::error;
use crate::health::PackInstanceId;
use crate::loader::LoadedPack;
use crate::manifest::{PackInputSchema, PackToolDef};

/// A pack tool that failed validation or registration, with the pack and
/// tool it belongs to so the failure can be folded into
/// [`crate::health::PackReport`] (#5208).
#[derive(Debug)]
pub struct PackToolFailure {
    /// Stable identity of the configured pack occurrence.
    pub pack_instance_id: PackInstanceId,
    /// Name of the pack declaring the tool.
    pub pack_name: String,
    /// Name of the tool that failed.
    pub tool_name: String,
    /// The underlying validation or registration error.
    pub error: error::Error,
}

/// Executes a pack-declared shell script with JSON input on stdin.
struct ShellToolExecutor {
    command_path: PathBuf,
    pack_root: PathBuf,
    runner: SubprocessRunner,
    timeout_ms: u64,
    /// Identity of `command_path` captured at registration time (#5213).
    expected_identity: FileIdentity,
    /// Whether the tool declared `egress = "none"` (#5214).
    deny_egress: bool,
}

/// Map a subprocess failure to an error result carrying machine-readable
/// diagnostics.
///
/// WHY(#5212): a sandbox/setup refusal is a different recovery path from a
/// command that ran and exited non-zero, so setup failures surface as
/// `sandbox_violations` rather than only a human-readable message.
fn subprocess_failure(error: &SubprocessError) -> ToolResult {
    let (message, sandbox_violations) = match error {
        SubprocessError::SandboxSetup(_) => (
            "tool sandbox setup failed",
            vec!["sandbox_setup_failed".to_owned()],
        ),
        SubprocessError::Spawn(_) => ("tool process could not start", Vec::new()),
        SubprocessError::Stdin(_) => ("tool input delivery failed", Vec::new()),
        SubprocessError::Wait(_) => ("tool process wait failed", Vec::new()),
        SubprocessError::Timeout(_) => ("tool command timed out", Vec::new()),
    };
    // Full OS/sandbox errors belong in the operator log. They can contain paths or other
    // process detail, so only stable categories cross into model-visible ToolResult fields.
    tracing::warn!(error = %error, "pack tool subprocess failed");
    let diagnostics = ToolDiagnostics {
        exit_code: None,
        stderr: None,
        sandbox_violations,
        duration_ms: 0,
    };
    ToolResult::error(message).with_diagnostics(diagnostics)
}

impl ToolExecutor for ShellToolExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            // SECURITY(#5213): re-check the command file's identity against
            // what was captured at registration time. A file swap after
            // registration (the pack directory is on disk, not immutable)
            // would otherwise execute silently under the tool's original,
            // reviewed name — bind execution to the specific file that was
            // validated, not just the path.
            if let Err(reason) = self.expected_identity.verify(&self.command_path) {
                return Ok(ToolResult::error(format!(
                    "tool command {} changed since registration: {reason}",
                    self.command_path.display()
                )));
            }

            let json_input = serde_json::to_string(&input.arguments).unwrap_or_else(|e| {
                tracing::debug!("failed to serialize tool arguments: {e}");
                String::new()
            });
            let timeout = Duration::from_millis(self.timeout_ms);
            let output_result =
                run_pack_command_with_retry(self, ctx, json_input.into_bytes(), timeout).await;
            let output_result = match output_result {
                Ok(output) => output,
                Err(e) => return Ok(subprocess_failure(&e)),
            };

            let code = output_result.exit_code;
            let is_error = code != 0;

            if !output_result.stderr.trim().is_empty() {
                tracing::warn!(
                    tool = %input.name,
                    exit_code = code,
                    stderr_bytes = output_result.stderr.len(),
                    "pack tool wrote stderr"
                );
            }

            let output = if !output_result.stdout.is_empty() {
                output_result.stdout
            } else if is_error {
                format!("command exited with status {code}")
            } else {
                String::new()
            };

            let diagnostics = ToolDiagnostics {
                exit_code: Some(code),
                // SECURITY(#5212): ToolDiagnostics renders into the model-visible turn.
                // Stderr is arbitrary subprocess output and no finite pattern redactor can
                // prove it secret-free, so keep it out of this surface. The warning above
                // gives operators the exit code and byte count without copying the content.
                stderr: None,
                sandbox_violations: Vec::new(),
                duration_ms: u64::try_from(output_result.duration.as_millis()).unwrap_or(u64::MAX),
            };

            if is_error {
                Ok(ToolResult::error(output).with_diagnostics(diagnostics))
            } else {
                Ok(ToolResult::text(output).with_diagnostics(diagnostics))
            }
        })
    }
}

async fn run_pack_command_with_retry(
    executor: &ShellToolExecutor,
    ctx: &ToolContext,
    stdin: Vec<u8>,
    timeout: Duration,
) -> Result<organon::subprocess::SubprocessOutput, SubprocessError> {
    let runner = executor.runner.clone();
    run_pack_command_with_retry_using(
        executor,
        ctx,
        stdin,
        timeout,
        move |request, attempt_ctx| {
            let runner = runner.clone();
            let command_path = executor.command_path.clone();
            let expected_identity = executor.expected_identity;
            async move {
                tokio::task::spawn_blocking(move || {
                    // Keep the production recheck in the blocking task so it
                    // runs after scheduler handoff, directly before Organon
                    // prepares and spawns this attempt.
                    expected_identity
                        .verify(&command_path)
                        .map_err(|reason| command_identity_error(&command_path, &reason))?;
                    runner.run(request, &attempt_ctx)
                })
                .await
                .map_err(|e| SubprocessError::Wait(std::io::Error::other(e.to_string())))?
            }
        },
    )
    .await
}

/// Run a pack command with an injected single-attempt operation.
///
/// The injection keeps retry policy deterministic in tests. Production passes
/// the real [`SubprocessRunner`] operation above.
async fn run_pack_command_with_retry_using<RunAttempt, AttemptFuture>(
    executor: &ShellToolExecutor,
    ctx: &ToolContext,
    stdin: Vec<u8>,
    timeout: Duration,
    mut run_attempt: RunAttempt,
) -> Result<organon::subprocess::SubprocessOutput, SubprocessError>
where
    RunAttempt: FnMut(SubprocessRequest, ToolContext) -> AttemptFuture,
    AttemptFuture: Future<Output = Result<organon::subprocess::SubprocessOutput, SubprocessError>>,
{
    let mut last_err = None;
    for attempt in 0..4 {
        let request = build_request(executor, stdin.clone(), timeout);

        // SECURITY: ETXTBSY is retryable, but the wait between attempts is
        // also a file-swap window. Revalidate immediately before *every*
        // spawn attempt, not only once before entering this loop.
        //
        // Residual boundary: Organon still executes `command_path` by path.
        // A mutation after this stat and before the kernel resolves exec is a
        // narrower remaining race; eliminating it requires descriptor-based
        // execution (fexecve/execveat), not another path check.
        executor
            .expected_identity
            .verify(&executor.command_path)
            .map_err(|reason| command_identity_error(&executor.command_path, &reason))?;

        let result = run_attempt(request, ctx.clone()).await;

        match result {
            Ok(output) => return Ok(output),
            Err(e) if is_text_file_busy(&e) && attempt < 3 => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(1 << (2 * attempt))).await;
            }
            Err(e) => return Err(e),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        SubprocessError::Spawn(std::io::Error::other("spawn failed after retry attempts"))
    }))
}

fn command_identity_error(command_path: &Path, reason: &str) -> SubprocessError {
    SubprocessError::Spawn(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!(
            "tool command {} changed since registration: {reason}",
            command_path.display()
        ),
    ))
}

/// Build the subprocess request for one invocation, applying the tool's
/// egress narrowing on top of the pack-root read/exec grants (#5214).
fn build_request(
    executor: &ShellToolExecutor,
    stdin: Vec<u8>,
    timeout: Duration,
) -> SubprocessRequest {
    let mut request =
        SubprocessRequest::new(executor.command_path.clone(), executor.pack_root.clone())
            .stdin_bytes(stdin)
            .timeout(timeout)
            .max_output_bytes(MAX_OUTPUT_BYTES)
            .allow_read_path(executor.pack_root.clone())
            .allow_exec_path(executor.command_path.clone());
    if executor.deny_egress {
        request = request.deny_egress();
    }
    request
}

fn is_text_file_busy(error: &SubprocessError) -> bool {
    matches!(error, SubprocessError::Spawn(e) if e.raw_os_error() == Some(26))
}

/// Register all tools from loaded packs into the tool registry.
///
/// Validates each tool's command path and schema, then registers it.
/// Invalid tools are skipped with warnings; per-tool failures are returned
/// with their pack association for health reporting.
pub fn register_pack_tools(
    packs: &[LoadedPack],
    registry: &mut ToolRegistry,
) -> Vec<PackToolFailure> {
    register_pack_tools_with_sandbox(packs, registry, organon::sandbox::SandboxConfig::default())
}

/// Register all tools from loaded packs with the supplied subprocess sandbox.
///
/// Runtime callers pass the same sandbox config used by built-in tools so pack
/// shell tools inherit the deployment's process, filesystem, and egress policy.
/// Declared tool timeouts are used as-is (no deployment-policy ceiling); prefer
/// [`register_pack_tools_with_sandbox_and_limits`] in production.
pub fn register_pack_tools_with_sandbox(
    packs: &[LoadedPack],
    registry: &mut ToolRegistry,
    sandbox: organon::sandbox::SandboxConfig,
) -> Vec<PackToolFailure> {
    register_pack_tools_impl(packs, registry, sandbox, None)
}

/// Register all tools from loaded packs, clamping each tool's declared timeout to the
/// deployment's subprocess timeout policy.
///
/// `subprocess_timeout_secs` is the deployment-wide ceiling (`ToolLimitsConfig`); a
/// pack tool declaring a timeout above this ceiling, or below a usable floor, is
/// clamped rather than honored verbatim, with a warning logged per clamped tool.
pub fn register_pack_tools_with_sandbox_and_limits(
    packs: &[LoadedPack],
    registry: &mut ToolRegistry,
    sandbox: organon::sandbox::SandboxConfig,
    subprocess_timeout_secs: u64,
) -> Vec<PackToolFailure> {
    let max_timeout_ms = subprocess_timeout_secs.saturating_mul(1_000);
    register_pack_tools_impl(packs, registry, sandbox, Some(max_timeout_ms))
}

fn register_pack_tools_impl(
    packs: &[LoadedPack],
    registry: &mut ToolRegistry,
    sandbox: organon::sandbox::SandboxConfig,
    max_timeout_ms: Option<u64>,
) -> Vec<PackToolFailure> {
    let mut errors = Vec::new();
    let baseline_failure =
        enforcing_baseline_failure(&sandbox, organon::sandbox::diagnostic_guarantees(&sandbox));
    let mut deny_egress_sandbox = sandbox.clone();
    deny_egress_sandbox.egress = organon::sandbox::EgressPolicy::Deny;
    deny_egress_sandbox.egress_allowlist.clear();
    let egress_none_enforceable = egress_none_is_enforceable(
        &deny_egress_sandbox,
        organon::sandbox::diagnostic_guarantees(&deny_egress_sandbox),
    );
    let runner = SubprocessRunner::new(sandbox);

    for pack in packs {
        let manifest = pack.manifest();
        // WHY: snapshot error count before this pack to compute per-pack failures
        // without contaminating counts from prior packs
        let errors_before = errors.len();

        for tool_def in &manifest.tools {
            let prepared = if let Some(reason) = baseline_failure.as_deref() {
                Err(tool_registration_error(
                    tool_def,
                    pack.name(),
                    reason.to_owned(),
                ))
            } else if tool_def.egress.as_deref() == Some("none") && !egress_none_enforceable {
                Err(tool_registration_error(
                    tool_def,
                    pack.name(),
                    "egress = \"none\" requires an enabled enforcing sandbox with an active \
                     egress-denial guarantee"
                        .to_owned(),
                ))
            } else {
                prepare_tool(
                    tool_def,
                    pack.root(),
                    pack.name(),
                    runner.clone(),
                    max_timeout_ms,
                )
            };
            let failure = match prepared {
                Ok((def, executor)) => match registry.register(def, executor) {
                    Ok(()) => {
                        info!(
                            tool = %tool_def.name,
                            pack = %pack.name(),
                            "pack tool registered"
                        );
                        continue;
                    }
                    Err(e) => error::Error::ToolRegistration {
                        tool_name: tool_def.name.clone(),
                        pack_name: pack.name().to_owned(),
                        reason: e.to_string(),
                        location: snafu::location!(),
                    },
                },
                Err(e) => e,
            };
            errors.push(PackToolFailure {
                pack_instance_id: pack.instance_id(),
                pack_name: pack.name().to_owned(),
                tool_name: tool_def.name.clone(),
                error: failure,
            });
        }

        if !manifest.tools.is_empty() {
            let pack_errors = errors.len() - errors_before;
            let registered = manifest.tools.len() - pack_errors;
            if registered > 0 {
                info!(
                    pack = %pack.name(),
                    tools = registered,
                    "pack tools registered"
                );
            }
        }
    }

    errors
}

/// Refuse all pack tools when an enforcing sandbox cannot provide its
/// baseline filesystem, syscall, and configured egress guarantees.
/// Registration-time refusal is honest; marking a tool active and waiting
/// for first execution is not.
fn enforcing_baseline_failure(
    sandbox: &organon::sandbox::SandboxConfig,
    guarantees: organon::sandbox::SandboxGuarantees,
) -> Option<String> {
    if !sandbox.enabled || sandbox.enforcement != organon::sandbox::SandboxEnforcement::Enforcing {
        return None;
    }

    let mut unavailable = Vec::new();
    if guarantees.landlock != organon::sandbox::GuaranteeStatus::Active {
        unavailable.push(format!("filesystem={}", guarantees.landlock));
    }
    if guarantees.seccomp != organon::sandbox::GuaranteeStatus::Active {
        unavailable.push(format!("syscall={}", guarantees.seccomp));
    }
    if !matches!(
        guarantees.egress,
        organon::sandbox::GuaranteeStatus::Active | organon::sandbox::GuaranteeStatus::Unrestricted
    ) {
        unavailable.push(format!("egress={}", guarantees.egress));
    }
    if unavailable.is_empty() {
        None
    } else {
        Some(format!(
            "enforcing sandbox baseline is unavailable ({})",
            unavailable.join(", ")
        ))
    }
}

/// Whether an explicit per-tool egress denial can be promised at
/// registration time.
fn egress_none_is_enforceable(
    sandbox: &organon::sandbox::SandboxConfig,
    guarantees: organon::sandbox::SandboxGuarantees,
) -> bool {
    sandbox.enabled
        && sandbox.enforcement == organon::sandbox::SandboxEnforcement::Enforcing
        && guarantees.egress == organon::sandbox::GuaranteeStatus::Active
}

/// Validate and convert a single pack tool definition into organon types.
///
/// `max_timeout_ms`, when `Some`, is the deployment's subprocess timeout ceiling
/// in milliseconds; the tool's declared timeout is clamped to `[1_000, max_timeout_ms]`
/// (with the ceiling itself floored to `1_000` to keep the clamp bounds valid) and a
/// warning is logged when clamping changes the effective value. `None` preserves the
/// declared timeout unclamped (test-only call paths).
fn prepare_tool(
    tool_def: &PackToolDef,
    pack_root: &Path,
    pack_name: &str,
    runner: SubprocessRunner,
    max_timeout_ms: Option<u64>,
) -> Result<(ToolDef, Box<dyn ToolExecutor>), error::Error> {
    if tool_def.timeout == 0 {
        return Err(error::Error::InvalidToolTimeout {
            pack: pack_name.to_owned(),
            tool: tool_def.name.clone(),
            timeout: tool_def.timeout,
            location: snafu::location!(),
        });
    }

    // WHY: the platform check runs first among the contract checks so an
    // unsupported host reports the real reason (platform mismatch) instead
    // of a secondary validation error for a tool that would never run here.
    validate_platform_support(tool_def, pack_name)?;

    let (command_path, expected_identity) = validate_command_path(pack_root, &tool_def.command)?;
    let groups = parse_groups(tool_def, pack_name)?;
    let tags = parse_tags(tool_def, pack_name)?;
    let reversibility = parse_reversibility(tool_def, pack_name)?;
    reject_unbound_authority(tool_def, pack_name)?;
    let deny_egress = parse_egress(tool_def, pack_name)?;

    let input_schema = match &tool_def.input_schema {
        Some(schema) => convert_input_schema(schema, &tool_def.name)?,
        None => InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
    };

    let tool_name = ToolName::new(&tool_def.name).map_err(|e| error::Error::ToolRegistration {
        tool_name: tool_def.name.clone(),
        pack_name: pack_name.to_owned(),
        reason: e.to_string(),
        location: snafu::location!(),
    })?;

    let def = ToolDef {
        name: tool_name,
        description: tool_def.description.clone(),
        extended_description: None,
        input_schema,
        category: ToolCategory::Domain,
        reversibility,
        auto_activate: false,
        groups,
        tags,
    };

    let effective_timeout_ms = match max_timeout_ms {
        Some(max) => {
            // WHY: guard against a ceiling below the floor (e.g. a misconfigured
            // near-zero subprocess_timeout_secs) so clamp's min<=max invariant holds
            let max = max.max(1_000);
            let clamped = tool_def.timeout.clamp(1_000, max);
            if clamped != tool_def.timeout {
                tracing::warn!(
                    pack = %pack_name,
                    tool = %tool_def.name,
                    requested_ms = tool_def.timeout,
                    effective_ms = clamped,
                    "pack tool timeout clamped to deployment policy"
                );
            }
            clamped
        }
        None => tool_def.timeout,
    };

    let executor = Box::new(ShellToolExecutor {
        command_path,
        pack_root: pack_root.to_path_buf(),
        runner,
        timeout_ms: effective_timeout_ms,
        expected_identity,
        deny_egress,
    });

    Ok((def, executor))
}

/// Reject manifest-controlled daemon/process authority (#5214).
///
/// The fields remain reserved for a future operator-owned per-tool grant,
/// but a pack manifest alone is not an authorization mechanism.
fn reject_unbound_authority(tool_def: &PackToolDef, pack_name: &str) -> Result<(), error::Error> {
    if !tool_def.env.is_empty() {
        return Err(tool_registration_error(
            tool_def,
            pack_name,
            "pack env grants require an operator-owned per-tool policy".to_owned(),
        ));
    }
    if !tool_def.write_paths.is_empty() {
        return Err(tool_registration_error(
            tool_def,
            pack_name,
            "pack write grants require intersection with operator policy".to_owned(),
        ));
    }
    Ok(())
}

/// Parse the tool's declared egress intent (#5214). Only `"none"` tightens
/// the sandbox policy; `"inherit"`/absent leaves the deployment policy
/// unchanged. A pack can never widen egress beyond the deployment policy.
fn parse_egress(tool_def: &PackToolDef, pack_name: &str) -> Result<bool, error::Error> {
    match tool_def.egress.as_deref() {
        None | Some("inherit") => Ok(false),
        Some("none") => Ok(true),
        Some(other) => Err(tool_registration_error(
            tool_def,
            pack_name,
            format!("unknown egress intent: {other} (expected \"none\" or \"inherit\")"),
        )),
    }
}

/// Refuse registration when the tool's declared `platforms` do not cover
/// this host (#5215).
///
/// WHY: absent `platforms` defaults to `["unix"]` — a pack tool is a
/// shebang-executed script, which needs a Unix exec environment. Skipping
/// (with a registration failure recorded in pack health) beats registering
/// a tool that can only fail at exec time on an unsupported host.
fn validate_platform_support(tool_def: &PackToolDef, pack_name: &str) -> Result<(), error::Error> {
    for platform in &tool_def.platforms {
        if !matches!(platform.as_str(), "linux" | "macos" | "unix") {
            return Err(tool_registration_error(
                tool_def,
                pack_name,
                format!(
                    "unknown platform '{platform}' (expected \"linux\", \"macos\", or \"unix\")"
                ),
            ));
        }
    }

    let declared: Vec<&str> = if tool_def.platforms.is_empty() {
        vec!["unix"]
    } else {
        tool_def.platforms.iter().map(String::as_str).collect()
    };
    let supported = declared.iter().any(|p| match *p {
        "linux" => cfg!(target_os = "linux"),
        "macos" => cfg!(target_os = "macos"),
        "unix" => cfg!(unix),
        _ => false,
    });
    if supported {
        return Ok(());
    }
    Err(tool_registration_error(
        tool_def,
        pack_name,
        format!(
            "tool supports platforms [{}], but this host is '{}' — tool skipped",
            declared.join(", "),
            std::env::consts::OS
        ),
    ))
}

fn parse_groups(tool_def: &PackToolDef, pack_name: &str) -> Result<Vec<ToolGroupId>, error::Error> {
    if tool_def.groups.is_empty() {
        return Ok(vec![ToolGroupId::Command]);
    }

    tool_def
        .groups
        .iter()
        .map(|group| {
            ToolGroupId::from_str(group).map_err(|e| {
                tool_registration_error(tool_def, pack_name, format!("invalid group: {e}"))
            })
        })
        .collect()
}

fn parse_tags(tool_def: &PackToolDef, pack_name: &str) -> Result<Vec<ToolTag>, error::Error> {
    if tool_def.tags.is_empty() {
        return Ok(vec![ToolTag::Execute]);
    }

    tool_def
        .tags
        .iter()
        .map(|tag| match tag.as_str() {
            "recon" => Ok(ToolTag::Recon),
            "edit" => Ok(ToolTag::Edit),
            "verify" => Ok(ToolTag::Verify),
            "fetch" => Ok(ToolTag::Fetch),
            "spawn" => Ok(ToolTag::Spawn),
            "plan" => Ok(ToolTag::Plan),
            "execute" => Ok(ToolTag::Execute),
            "format" => Ok(ToolTag::Format),
            other => Err(tool_registration_error(
                tool_def,
                pack_name,
                format!("unknown tool tag: {other}"),
            )),
        })
        .collect()
}

fn parse_reversibility(
    tool_def: &PackToolDef,
    pack_name: &str,
) -> Result<Reversibility, error::Error> {
    match tool_def.reversibility.as_deref() {
        None | Some("irreversible") => Ok(Reversibility::Irreversible),
        Some("fully_reversible") => Ok(Reversibility::FullyReversible),
        Some("reversible") => Ok(Reversibility::Reversible),
        Some("partially_reversible") => Ok(Reversibility::PartiallyReversible),
        Some(other) => Err(tool_registration_error(
            tool_def,
            pack_name,
            format!("unknown reversibility: {other}"),
        )),
    }
}

fn tool_registration_error(
    tool_def: &PackToolDef,
    pack_name: &str,
    reason: String,
) -> error::Error {
    error::Error::ToolRegistration {
        tool_name: tool_def.name.clone(),
        pack_name: pack_name.to_owned(),
        reason,
        location: snafu::location!(),
    }
}

/// Validate that a command path is a relative in-pack executable file, and
/// capture its identity for later swap detection at execution time.
///
/// SECURITY(#5213): validation used to be canonicalize-then-contain only,
/// which left three gaps: an absolute/`..` command string was rejected only
/// after a filesystem round-trip (a syntactic pre-check is cheaper and
/// fails before touching disk); a directory or non-executable file at the
/// resolved path passed registration and only failed at first invocation
/// (broken tools were exposed to callers until then); and nothing bound
/// the registered tool to the specific file it validated, so a file swap
/// on disk between registration and execution ran silently under the
/// original, reviewed name.
fn validate_command_path(
    pack_root: &Path,
    command: &str,
) -> Result<(PathBuf, FileIdentity), error::Error> {
    // WHY: reject absolute paths and `..`/root components in the *declared*
    // string before any filesystem access. `Path::join` with an absolute
    // second argument discards the first entirely (`pack_root.join("/etc/passwd")
    // == "/etc/passwd"`), so this also closes that join-level surprise, not
    // just an early-exit optimization over the canonicalize-based check below.
    let declared = Path::new(command);
    if !crate::manifest::is_relative_in_pack_path(declared) {
        return Err(error::Error::ToolCommandEscape {
            path: declared.to_path_buf(),
            location: snafu::location!(),
        });
    }

    let resolved = pack_root.join(command);

    let canonical =
        resolved
            .canonicalize()
            .map_err(|_io_err| error::Error::ToolCommandNotFound {
                path: resolved.clone(),
                location: snafu::location!(),
            })?;

    let canonical_root =
        pack_root
            .canonicalize()
            .map_err(|_io_err| error::Error::ToolCommandNotFound {
                path: pack_root.to_path_buf(),
                location: snafu::location!(),
            })?;

    if !canonical.starts_with(&canonical_root) {
        return Err(error::Error::ToolCommandEscape {
            path: resolved,
            location: snafu::location!(),
        });
    }

    let identity =
        FileIdentity::of(&canonical).map_err(|reason| error::Error::ToolCommandNotExecutable {
            path: canonical.clone(),
            reason,
            location: snafu::location!(),
        })?;

    Ok((canonical, identity))
}

/// Filesystem identity of a validated command file, captured at
/// registration time and re-checked before every execution (#5213).
///
/// Binds by device + inode + size + mtime rather than content hash: pack
/// tool scripts already require a filesystem stat on every execution (the
/// re-check itself), and this needs no new dependency for the same
/// TOCTOU-closing effect the issue asks for ("bind digest/inode"). It is
/// not cryptographic — an attacker who can engineer an inode collision
/// with matching size and mtime on a live filesystem could still swap
/// content, but that requires write access to the pack directory, which is
/// already the trust boundary this whole validation path assumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    len: u64,
    mtime_nanos: i128,
}

impl FileIdentity {
    /// Capture the identity of `path`, which must be a regular file with at
    /// least one executable permission bit set (owner, group, or other).
    fn of(path: &Path) -> std::result::Result<Self, String> {
        let metadata = std::fs::metadata(path).map_err(|e| format!("stat failed: {e}"))?;
        if !metadata.is_file() {
            return Err("not a regular file".to_owned());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err("missing executable permission bit".to_owned());
            }
            let mtime_nanos =
                i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec());
            Ok(Self {
                dev: metadata.dev(),
                ino: metadata.ino(),
                len: metadata.len(),
                mtime_nanos,
            })
        }
        #[cfg(not(unix))]
        {
            let mtime_nanos = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos().cast_signed());
            Ok(Self {
                len: metadata.len(),
                mtime_nanos,
            })
        }
    }

    /// Re-stat `path` and confirm it still matches this captured identity.
    fn verify(&self, path: &Path) -> std::result::Result<(), String> {
        let current = Self::of(path)?;
        if current == *self {
            Ok(())
        } else {
            Err("file identity does not match the version validated at registration".to_owned())
        }
    }
}

/// Convert a pack input schema to an organon `InputSchema`.
fn convert_input_schema(
    schema: &PackInputSchema,
    tool_name: &str,
) -> Result<InputSchema, error::Error> {
    let mut properties = IndexMap::with_capacity(schema.properties.len());

    for (name, prop) in &schema.properties {
        let property_type = parse_property_type(&prop.property_type, tool_name)?;
        properties.insert(
            name.clone(),
            PropertyDef {
                property_type,
                description: prop.description.clone(),
                enum_values: prop.enum_values.clone(),
                default: prop.default.clone(),
                ..Default::default()
            },
        );
    }

    Ok(InputSchema {
        properties,
        required: schema.required.clone(),
    })
}

/// Parse a string type name into an organon `PropertyType`.
fn parse_property_type(type_name: &str, tool_name: &str) -> Result<PropertyType, error::Error> {
    match type_name {
        "string" => Ok(PropertyType::String),
        "number" => Ok(PropertyType::Number),
        "integer" => Ok(PropertyType::Integer),
        "boolean" => Ok(PropertyType::Boolean),
        "array" => Ok(PropertyType::Array),
        "object" => Ok(PropertyType::Object),
        _ => Err(error::Error::UnknownPropertyType {
            type_name: type_name.to_owned(),
            tool_name: tool_name.to_owned(),
            location: snafu::location!(),
        }),
    }
}

#[cfg(test)]
mod tests;
