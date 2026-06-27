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
use crate::loader::LoadedPack;
use crate::manifest::{PackInputSchema, PackToolDef};

/// Executes a pack-declared shell script with JSON input on stdin.
struct ShellToolExecutor {
    command_path: PathBuf,
    pack_root: PathBuf,
    runner: SubprocessRunner,
    timeout_ms: u64,
}

impl ToolExecutor for ShellToolExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let json_input = serde_json::to_string(&input.arguments).unwrap_or_else(|e| {
                tracing::debug!("failed to serialize tool arguments: {e}");
                String::new()
            });
            let timeout = Duration::from_millis(self.timeout_ms);
            let output_result =
                run_pack_command_with_retry(self, ctx, json_input.into_bytes(), timeout).await;
            let output_result = match output_result {
                Ok(output) => output,
                Err(e) => return Ok(ToolResult::error(e.to_string())),
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
    let mut last_err = None;
    for attempt in 0..4 {
        let runner = executor.runner.clone();
        let ctx = ctx.clone();
        let request =
            SubprocessRequest::new(executor.command_path.clone(), executor.pack_root.clone())
                .stdin_bytes(stdin.clone())
                .timeout(timeout)
                .max_output_bytes(MAX_OUTPUT_BYTES)
                .allow_read_path(executor.pack_root.clone())
                .allow_exec_path(executor.command_path.clone());

        let result = tokio::task::spawn_blocking(move || runner.run(request, &ctx))
            .await
            .map_err(|e| SubprocessError::Wait(std::io::Error::other(e.to_string())))?;

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

fn is_text_file_busy(error: &SubprocessError) -> bool {
    matches!(error, SubprocessError::Spawn(e) if e.raw_os_error() == Some(26))
}

/// Register all tools from loaded packs into the tool registry.
///
/// Validates each tool's command path and schema, then registers it.
/// Invalid tools are skipped with warnings; errors are collected and returned.
pub fn register_pack_tools(packs: &[LoadedPack], registry: &mut ToolRegistry) -> Vec<error::Error> {
    register_pack_tools_with_sandbox(packs, registry, organon::sandbox::SandboxConfig::default())
}

/// Register all tools from loaded packs with the supplied subprocess sandbox.
///
/// Runtime callers pass the same sandbox config used by built-in tools so pack
/// shell tools inherit the deployment's process, filesystem, and egress policy.
pub fn register_pack_tools_with_sandbox(
    packs: &[LoadedPack],
    registry: &mut ToolRegistry,
    sandbox: organon::sandbox::SandboxConfig,
) -> Vec<error::Error> {
    let mut errors = Vec::new();
    let runner = SubprocessRunner::new(sandbox);

    for pack in packs {
        // WHY: snapshot error count before this pack to compute per-pack failures
        // without contaminating counts from prior packs
        let errors_before = errors.len();

        for tool_def in &pack.manifest.tools {
            match prepare_tool(tool_def, &pack.root, &pack.manifest.name, runner.clone()) {
                Ok((def, executor)) => match registry.register(def, executor) {
                    Ok(()) => {
                        info!(
                            tool = %tool_def.name,
                            pack = %pack.manifest.name,
                            "pack tool registered"
                        );
                    }
                    Err(e) => {
                        let err = error::Error::ToolRegistration {
                            tool_name: tool_def.name.clone(),
                            pack_name: pack.manifest.name.clone(),
                            reason: e.to_string(),
                            location: snafu::Location::new(file!(), line!(), column!()),
                        };
                        errors.push(err);
                    }
                },
                Err(e) => errors.push(e),
            }
        }

        if !pack.manifest.tools.is_empty() {
            let pack_errors = errors.len() - errors_before;
            let registered = pack.manifest.tools.len() - pack_errors;
            if registered > 0 {
                info!(
                    pack = %pack.manifest.name,
                    tools = registered,
                    "pack tools registered"
                );
            }
        }
    }

    errors
}

/// Validate and convert a single pack tool definition into organon types.
fn prepare_tool(
    tool_def: &PackToolDef,
    pack_root: &Path,
    pack_name: &str,
    runner: SubprocessRunner,
) -> Result<(ToolDef, Box<dyn ToolExecutor>), error::Error> {
    let command_path = validate_command_path(pack_root, &tool_def.command)?;
    let groups = parse_groups(tool_def, pack_name)?;
    let tags = parse_tags(tool_def, pack_name)?;
    let reversibility = parse_reversibility(tool_def, pack_name)?;

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
        location: snafu::Location::new(file!(), line!(), column!()),
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

    let executor = Box::new(ShellToolExecutor {
        command_path,
        pack_root: pack_root.to_path_buf(),
        runner,
        timeout_ms: tool_def.timeout,
    });

    Ok((def, executor))
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
        location: snafu::Location::new(file!(), line!(), column!()),
    }
}

/// Validate that a command path exists and stays within the pack root.
fn validate_command_path(pack_root: &Path, command: &str) -> Result<PathBuf, error::Error> {
    let resolved = pack_root.join(command);

    let canonical =
        resolved
            .canonicalize()
            .map_err(|_io_err| error::Error::ToolCommandNotFound {
                path: resolved.clone(),
                location: snafu::Location::new(file!(), line!(), column!()),
            })?;

    let canonical_root =
        pack_root
            .canonicalize()
            .map_err(|_io_err| error::Error::ToolCommandNotFound {
                path: pack_root.to_path_buf(),
                location: snafu::Location::new(file!(), line!(), column!()),
            })?;

    if !canonical.starts_with(&canonical_root) {
        return Err(error::Error::ToolCommandEscape {
            path: resolved,
            location: snafu::Location::new(file!(), line!(), column!()),
        });
    }

    Ok(canonical)
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
            location: snafu::Location::new(file!(), line!(), column!()),
        }),
    }
}

#[cfg(test)]
mod tests;
