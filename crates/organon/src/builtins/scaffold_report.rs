//! Scaffold report tool: generates a new report project from embedded templates.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use crate::builtins::workspace::validate_path;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCallCapability,
    ToolCallCapabilityRule, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult,
    ToolTag,
};

fn extract_opt_str<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(serde_json::Value::as_str)
}

#[expect(
    clippy::result_large_err,
    reason = "ToolResult grew by receipt field; boxing would change public API"
)]
fn extract_str<'a>(
    args: &'a serde_json::Value,
    key: &str,
) -> std::result::Result<&'a str, ToolResult> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ToolResult::error(format!("missing required argument: {key}")))
}

fn extract_bool(args: &serde_json::Value, key: &str, default: bool) -> bool {
    args.get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(default)
}

struct ScaffoldReportExecutor;

impl ToolExecutor for ScaffoldReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let slug = match extract_str(args, "slug") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let description = extract_opt_str(args, "description").unwrap_or("");
            let format_str = match extract_str(args, "format") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let confidential = extract_bool(args, "confidential", false);

            let format = match format_str.to_lowercase().as_str() {
                "typst" => poiesis_scaffold::Format::Typst,
                "xlsx" => poiesis_scaffold::Format::Xlsx,
                "both" => poiesis_scaffold::Format::Both,
                other => {
                    return Ok(ToolResult::error(format!("unsupported format: {other}")));
                }
            };

            let files =
                match poiesis_scaffold::scaffold_report(slug, description, format, confidential) {
                    Ok(f) => f,
                    Err(e) => return Ok(ToolResult::error(e.to_string())),
                };

            if let Some(dir) = extract_opt_str(args, "directory") {
                let validated_dir = match validate_path(dir, ctx, &input.name) {
                    Ok(path) => path,
                    Err(e) => {
                        return Ok(ToolResult::error(format!("invalid directory {dir:?}: {e}")));
                    }
                };
                if let Err(e) = tokio::fs::create_dir_all(&validated_dir).await {
                    return Ok(ToolResult::error(format!(
                        "failed to create directory {dir:?}: {e}"
                    )));
                }
                for file in &files {
                    let path = validated_dir.join(&file.path);
                    if let Some(parent) = path.parent()
                        && let Err(e) = tokio::fs::create_dir_all(parent).await
                    {
                        return Ok(ToolResult::error(format!(
                            "failed to create parent directory for {}: {e}",
                            file.path.display()
                        )));
                    }
                    if let Err(e) = tokio::fs::write(&path, &file.contents).await {
                        return Ok(ToolResult::error(format!(
                            "failed to write {}: {e}",
                            file.path.display()
                        )));
                    }
                }
                return Ok(ToolResult::text(format!(
                    "Scaffolded {} files to {}",
                    files.len(),
                    dir
                )));
            }

            // Return JSON manifest with base64-encoded contents.
            let manifest: Vec<serde_json::Value> = files
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "path": f.path.to_string_lossy(),
                        "contents_base64": koina::base64::encode(&f.contents),
                    })
                })
                .collect();

            match serde_json::to_string_pretty(&manifest) {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(format!(
                    "failed to serialize manifest: {e}"
                ))),
            }
        })
    }
}

fn scaffold_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("scaffold_report"),
        description:
            "Generate a report project scaffold (Typst, XLSX, or both) from embedded templates."
                .to_owned(),
        extended_description: Some(
            "Returns a JSON list of files (path + base64 contents) unless `directory` is provided, \
             in which case files are written to disk and a summary is returned."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "slug".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project slug / short name (used for filenames)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "description".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project description".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!("")),
                        ..Default::default(),
                    },
                ),
                (
                    "format".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Output format: typst, xlsx, or both".to_owned(),
                        enum_values: Some(vec![
                            "typst".to_owned(),
                            "xlsx".to_owned(),
                            "both".to_owned(),
                        ]),
                        default: Some(serde_json::json!("typst")),
                        ..Default::default(),
                    },
                ),
                (
                    "confidential".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Inject CONFIDENTIAL headers/footers".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default(),
                    },
                ),
                (
                    "directory".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional directory to write scaffolded files to".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["slug".to_owned(), "format".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read, ToolGroupId::Edit],
        tags: vec![ToolTag::Edit, ToolTag::Plan],
    }
}

fn scaffold_report_capability_rule() -> ToolCallCapabilityRule {
    ToolCallCapabilityRule::argument_presence(
        "directory",
        ToolCallCapability::new(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
        ToolCallCapability::new(vec![ToolGroupId::Read], Reversibility::FullyReversible),
    )
}

/// Register the scaffold report tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register_with_call_capability(
        scaffold_report_def(),
        scaffold_report_capability_rule(),
        Box::new(ScaffoldReportExecutor),
    )?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use super::*;
    use crate::types::ApprovalRequirement;

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

    fn input(directory: &str) -> ToolInput {
        ToolInput {
            name: ToolName::from_static("scaffold_report"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({
                "slug": "acme-report",
                "format": "typst",
                "directory": directory,
            }),
        }
    }

    #[tokio::test]
    async fn scaffold_report_rejects_directory_escape() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let ctx = test_ctx(dir.path());

        for directory in ["/etc/aletheia-scaffold", "../aletheia-scaffold"] {
            let result = ScaffoldReportExecutor
                .execute(&input(directory), &ctx)
                .await
                .expect("exec");
            assert!(result.is_error, "{directory} must be rejected");
            assert!(result.content.text_summary().contains("invalid directory"));
        }
    }

    #[test]
    fn scaffold_report_call_capability_requires_approval_when_directory_present() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("register");

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("scaffold_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "slug": "acme-report",
                        "format": "typst",
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::None,
            "no directory means no disk write"
        );

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("scaffold_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "slug": "acme-report",
                        "format": "typst",
                        "directory": "/tmp/reports",
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::Required,
            "directory present means disk write"
        );
    }
}
