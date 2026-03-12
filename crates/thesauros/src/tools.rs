//! Pack tool registration and shell execution.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::time::Duration;

use aletheia_koina::id::ToolName;
use aletheia_organon::registry::{ToolExecutor, ToolRegistry};
use aletheia_organon::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};
use indexmap::IndexMap;
use tracing::info;

use crate::error;
use crate::loader::LoadedPack;
use crate::manifest::{PackInputSchema, PackToolDef};

/// Maximum output bytes before truncation (50 KB, matching `ExecExecutor`).
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Executes a pack-declared shell script with JSON input on stdin.
struct ShellToolExecutor {
    command_path: PathBuf,
    pack_root: PathBuf,
    timeout_ms: u64,
}

impl ToolExecutor for ShellToolExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = aletheia_organon::error::Result<ToolResult>> + Send + 'a>>
    {
        Box::pin(async {
            let json_input = serde_json::to_string(&input.arguments).unwrap_or_else(|e| {
                tracing::debug!("failed to serialize tool arguments: {e}");
                String::new()
            });
            let timeout = Duration::from_millis(self.timeout_ms);

            // Retry on ETXTBSY (errno 26) — a benign race between writing/chmod
            // on a script and exec'ing it. Common in CI and on busy systems.
            let mut child = {
                let mut last_err = None;
                let mut spawned = None;
                for attempt in 0..4 {
                    match Command::new(&self.command_path)
                        .current_dir(&self.pack_root)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                    {
                        Ok(c) => {
                            spawned = Some(c);
                            break;
                        }
                        Err(e) if e.raw_os_error() == Some(26) && attempt < 3 => {
                            // ETXTBSY — brief backoff (1ms, 5ms, 25ms)
                            tokio::time::sleep(Duration::from_millis(1 << (2 * attempt))).await;
                            last_err = Some(e);
                        }
                        Err(e) => {
                            return Ok(ToolResult::error(format!("spawn failed: {e}")));
                        }
                    }
                }
                if let Some(c) = spawned {
                    c
                } else {
                    let msg = last_err.map_or_else(
                        || "spawn failed: binary not found or inaccessible".to_owned(),
                        |e| format!("spawn failed after retries: {e}"),
                    );
                    return Ok(ToolResult::error(msg));
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                if let Err(e) = stdin.write_all(json_input.as_bytes()) {
                    return Ok(ToolResult::error(format!(
                        "failed to write tool input: {e}"
                    )));
                }
            }

            // Wait in a background thread to avoid blocking the async runtime,
            // then enforce timeout from this async side via oneshot + tokio timeout.
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let result = child.wait_with_output();
                let _ = tx.send(result);
            });

            let output_result = match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(Ok(o))) => o,
                Ok(Ok(Err(e))) => return Ok(ToolResult::error(format!("wait failed: {e}"))),
                Ok(Err(_)) => {
                    return Ok(ToolResult::error(
                        "wait channel closed unexpectedly".to_owned(),
                    ));
                }
                Err(_) => {
                    return Ok(ToolResult::error(format!(
                        "command timed out after {}ms",
                        self.timeout_ms
                    )));
                }
            };

            let code = output_result.status.code().unwrap_or(-1);
            let is_error = code != 0;

            let stdout = String::from_utf8_lossy(&output_result.stdout);
            let stderr = String::from_utf8_lossy(&output_result.stderr);

            let mut output = if stderr.is_empty() {
                stdout.into_owned()
            } else {
                format!("{stdout}\n[stderr] {stderr}")
            };

            if output.len() > MAX_OUTPUT_BYTES {
                output.truncate(MAX_OUTPUT_BYTES);
                output.push_str("\n[output truncated]");
            }

            if is_error {
                Ok(ToolResult::error(output))
            } else {
                Ok(ToolResult::text(output))
            }
        })
    }
}

/// Register all tools from loaded packs into the tool registry.
///
/// Validates each tool's command path and schema, then registers it.
/// Invalid tools are skipped with warnings; errors are collected and returned.
pub fn register_pack_tools(packs: &[LoadedPack], registry: &mut ToolRegistry) -> Vec<error::Error> {
    let mut errors = Vec::new();

    for pack in packs {
        for tool_def in &pack.manifest.tools {
            match prepare_tool(tool_def, &pack.root, &pack.manifest.name) {
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
            let registered = pack.manifest.tools.len() - errors.len();
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
) -> Result<(ToolDef, Box<dyn ToolExecutor>), error::Error> {
    let command_path = validate_command_path(pack_root, &tool_def.command)?;

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
        auto_activate: false,
    };

    let executor = Box::new(ShellToolExecutor {
        command_path,
        pack_root: pack_root.to_path_buf(),
        timeout_ms: tool_def.timeout,
    });

    Ok((def, executor))
}

/// Validate that a command path exists and stays within the pack root.
fn validate_command_path(pack_root: &Path, command: &str) -> Result<PathBuf, error::Error> {
    let resolved = pack_root.join(command);

    // Canonicalize to resolve symlinks and ../ components
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::manifest::{PackInputSchema, PackManifest, PackPropertyDef, PackToolDef};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn setup_pack_dir(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            // Use explicit File to ensure fd is closed before any chmod/exec
            let file = std::fs::File::create(&path).unwrap();
            std::io::Write::write_all(&mut &file, content.as_bytes()).unwrap();
            file.sync_all().unwrap();
            drop(file);
        }
        dir
    }

    fn make_executable(dir: &TempDir, path: &str) {
        let full = dir.path().join(path);
        let mut perms = fs::metadata(&full).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&full, perms).unwrap();
    }

    fn minimal_loaded_pack(dir: &TempDir, tools: Vec<PackToolDef>) -> LoadedPack {
        LoadedPack {
            manifest: PackManifest {
                name: "test-pack".to_owned(),
                version: "1.0".to_owned(),
                description: None,
                context: vec![],
                tools,
                overlays: std::collections::HashMap::new(),
            },
            sections: vec![],
            root: dir.path().to_path_buf(),
        }
    }

    #[test]
    fn validate_command_path_success() {
        let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh\necho ok")]);
        let result = validate_command_path(dir.path(), "tools/test.sh");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_command_path_missing() {
        let dir = setup_pack_dir(&[]);
        let result = validate_command_path(dir.path(), "tools/missing.sh");
        assert!(matches!(
            result.unwrap_err(),
            error::Error::ToolCommandNotFound { .. }
        ));
    }

    #[test]
    fn validate_command_path_escape_rejected() {
        let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
        let result = validate_command_path(dir.path(), "../../../etc/passwd");
        // Either ToolCommandNotFound (can't canonicalize) or ToolCommandEscape
        let err = result.unwrap_err();
        assert!(
            matches!(err, error::Error::ToolCommandNotFound { .. })
                || matches!(err, error::Error::ToolCommandEscape { .. })
        );
    }

    #[test]
    fn parse_property_type_all_variants() {
        assert_eq!(
            parse_property_type("string", "t").unwrap(),
            PropertyType::String
        );
        assert_eq!(
            parse_property_type("number", "t").unwrap(),
            PropertyType::Number
        );
        assert_eq!(
            parse_property_type("integer", "t").unwrap(),
            PropertyType::Integer
        );
        assert_eq!(
            parse_property_type("boolean", "t").unwrap(),
            PropertyType::Boolean
        );
        assert_eq!(
            parse_property_type("array", "t").unwrap(),
            PropertyType::Array
        );
        assert_eq!(
            parse_property_type("object", "t").unwrap(),
            PropertyType::Object
        );
    }

    #[test]
    fn parse_property_type_unknown_rejected() {
        let err = parse_property_type("float", "my_tool").unwrap_err();
        assert!(matches!(err, error::Error::UnknownPropertyType { .. }));
        assert!(err.to_string().contains("float"));
        assert!(err.to_string().contains("my_tool"));
    }

    #[test]
    fn convert_input_schema_success() {
        let schema = PackInputSchema {
            properties: IndexMap::from([
                (
                    "sql".to_owned(),
                    PackPropertyDef {
                        property_type: "string".to_owned(),
                        description: "SQL query".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "limit".to_owned(),
                    PackPropertyDef {
                        property_type: "integer".to_owned(),
                        description: "Row limit".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(100)),
                    },
                ),
            ]),
            required: vec!["sql".to_owned()],
        };

        let result = convert_input_schema(&schema, "test").unwrap();
        assert_eq!(result.properties.len(), 2);
        assert_eq!(result.properties["sql"].property_type, PropertyType::String);
        assert_eq!(
            result.properties["limit"].property_type,
            PropertyType::Integer
        );
        assert_eq!(
            result.properties["limit"].default,
            Some(serde_json::json!(100))
        );
        assert_eq!(result.required, vec!["sql"]);
    }

    #[test]
    fn register_pack_tools_success() {
        let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\necho ok")]);
        make_executable(&dir, "tools/echo.sh");

        let tool = PackToolDef {
            name: "echo_tool".to_owned(),
            description: "Echo tool".to_owned(),
            command: "tools/echo.sh".to_owned(),
            timeout: 5000,
            input_schema: None,
        };
        let pack = minimal_loaded_pack(&dir, vec![tool]);

        let mut registry = ToolRegistry::new();
        let errors = register_pack_tools(&[pack], &mut registry);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(registry.definitions().len(), 1);
        assert_eq!(registry.definitions()[0].name.as_str(), "echo_tool");
        assert_eq!(registry.definitions()[0].category, ToolCategory::Domain);
    }

    #[test]
    fn register_pack_tools_skips_missing_command() {
        let dir = setup_pack_dir(&[]);
        let tool = PackToolDef {
            name: "missing_tool".to_owned(),
            description: "Missing command".to_owned(),
            command: "tools/nonexistent.sh".to_owned(),
            timeout: 5000,
            input_schema: None,
        };
        let pack = minimal_loaded_pack(&dir, vec![tool]);

        let mut registry = ToolRegistry::new();
        let errors = register_pack_tools(&[pack], &mut registry);
        assert_eq!(errors.len(), 1);
        assert!(registry.definitions().is_empty());
    }

    #[test]
    fn register_pack_tools_skips_bad_schema() {
        let dir = setup_pack_dir(&[("tools/test.sh", "#!/bin/sh")]);
        let tool = PackToolDef {
            name: "bad_schema".to_owned(),
            description: "Bad schema".to_owned(),
            command: "tools/test.sh".to_owned(),
            timeout: 5000,
            input_schema: Some(PackInputSchema {
                properties: IndexMap::from([(
                    "field".to_owned(),
                    PackPropertyDef {
                        property_type: "float".to_owned(),
                        description: "bad type".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                )]),
                required: vec![],
            }),
        };
        let pack = minimal_loaded_pack(&dir, vec![tool]);

        let mut registry = ToolRegistry::new();
        let errors = register_pack_tools(&[pack], &mut registry);
        assert_eq!(errors.len(), 1);
        assert!(registry.definitions().is_empty());
    }

    #[tokio::test]
    async fn shell_executor_runs_script() {
        let dir = setup_pack_dir(&[("tools/echo.sh", "#!/bin/sh\ncat")]);
        make_executable(&dir, "tools/echo.sh");

        let executor = ShellToolExecutor {
            command_path: dir.path().join("tools/echo.sh").canonicalize().unwrap(),
            pack_root: dir.path().to_path_buf(),
            timeout_ms: 5000,
        };

        let input = ToolInput {
            name: ToolName::new("echo_tool").unwrap(),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({"message": "hello"}),
        };
        let ctx = ToolContext {
            nous_id: aletheia_koina::id::NousId::new("test").unwrap(),
            session_id: aletheia_koina::id::SessionId::new(),
            workspace: dir.path().to_path_buf(),
            allowed_roots: vec![],
            services: None,
            active_tools: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
        };

        let result = executor.execute(&input, &ctx).await.unwrap();
        assert!(
            !result.is_error,
            "unexpected error: {}",
            result.content.text_summary()
        );
        assert!(result.content.text_summary().contains("hello"));
    }

    #[tokio::test]
    async fn shell_executor_nonzero_exit_is_error() {
        let dir = setup_pack_dir(&[("tools/fail.sh", "#!/bin/sh\nexit 1")]);
        make_executable(&dir, "tools/fail.sh");

        let executor = ShellToolExecutor {
            command_path: dir.path().join("tools/fail.sh").canonicalize().unwrap(),
            pack_root: dir.path().to_path_buf(),
            timeout_ms: 5000,
        };

        let input = ToolInput {
            name: ToolName::new("fail_tool").unwrap(),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({}),
        };
        let ctx = ToolContext {
            nous_id: aletheia_koina::id::NousId::new("test").unwrap(),
            session_id: aletheia_koina::id::SessionId::new(),
            workspace: dir.path().to_path_buf(),
            allowed_roots: vec![],
            services: None,
            active_tools: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
        };

        let result = executor.execute(&input, &ctx).await.unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn register_empty_packs() {
        let mut registry = ToolRegistry::new();
        let errors = register_pack_tools(&[], &mut registry);
        assert!(errors.is_empty());
        assert!(registry.definitions().is_empty());
    }
}
