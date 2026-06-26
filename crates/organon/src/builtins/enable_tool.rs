//! Meta-tool for dynamically activating lazy tools per session.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::surface::{ENABLE_TOOL, SurfaceLookup};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::workspace::extract_str;

struct EnableToolExecutor;

enum ToolSource {
    LocalLazy,
    ProviderServer { sensitive: bool },
}

impl ToolExecutor for EnableToolExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let name = extract_str(&input.arguments, "name", &input.name)?;

            let Ok(tool_name) = ToolName::new(name) else {
                return Ok(ToolResult::error(format!("invalid tool name: {name}")));
            };

            if let Some(surface) = ctx.effective_surface() {
                return Ok(activate_from_surface(name, &tool_name, ctx, &surface));
            }

            let Some(services) = ctx.services.as_deref() else {
                return Ok(ToolResult::error("tool services not configured"));
            };

            let local_entry = services
                .lazy_tool_catalog
                .iter()
                .find(|(n, _)| *n == tool_name);
            let server_entry = services
                .server_tool_config
                .catalog_entries_with_metadata()
                .into_iter()
                .find(|entry| entry.name == tool_name);

            let (description, source) = match (local_entry, server_entry) {
                (Some((_, desc)), _) => (desc.clone(), ToolSource::LocalLazy),
                (None, Some(entry)) => (
                    entry.description,
                    ToolSource::ProviderServer {
                        sensitive: entry.sensitive,
                    },
                ),
                (None, None) => {
                    let server_catalog = services.server_tool_config.catalog_entries();
                    let mut available: Vec<&str> = services
                        .lazy_tool_catalog
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect();
                    available.extend(server_catalog.iter().map(|(n, _)| n.as_str()));
                    return Ok(ToolResult::error(format!(
                        "tool '{name}' not found. Available tools: {}",
                        available.join(", ")
                    )));
                }
            };

            // WHY: Single write lock for the check-and-set: acquiring a read
            // lock to check then dropping it before acquiring a write lock
            // creates a TOCTOU window where a concurrent caller can insert
            // the same tool between the two acquisitions.
            {
                let Ok(mut active) = ctx.active_tools.write() else {
                    return Ok(ToolResult::error(
                        "internal error: active_tools lock poisoned",
                    ));
                };
                if active.contains(&tool_name) {
                    return Ok(ToolResult::text(format!("'{name}' is already active.")));
                }
                active.insert(tool_name.clone());
            }

            let (source_str, sensitive) = match &source {
                ToolSource::LocalLazy => ("local_lazy", false),
                ToolSource::ProviderServer { sensitive } => ("provider_server", *sensitive),
            };

            tracing::info!(
                target_tool = %tool_name,
                session_id = %ctx.session_id,
                turn_number = ctx.turn_number,
                source = source_str,
                sensitive,
                "enable_tool: activated tool"
            );

            Ok(ToolResult::text(format!(
                "Activated '{name}': {description}"
            )))
        })
    }
}

fn activate_from_surface(
    raw_name: &str,
    tool_name: &ToolName,
    ctx: &ToolContext,
    surface: &crate::surface::EffectiveToolSurface,
) -> ToolResult {
    let description = match surface.lookup(tool_name) {
        SurfaceLookup::Inactive(entry) => entry.description.clone(),
        SurfaceLookup::Callable(_) => {
            return ToolResult::text(format!("'{raw_name}' is already active."));
        }
        SurfaceLookup::Denied(entry) => {
            let reason = entry
                .availability
                .denial_reason()
                .map_or("policy", crate::surface::DenialReason::as_str);
            return ToolResult::error(format!("tool '{raw_name}' cannot be activated: {reason}"));
        }
        SurfaceLookup::Unknown => {
            let available = surface
                .lazy_catalog()
                .into_iter()
                .map(|(name, _description)| name.as_str().to_owned())
                .collect::<Vec<_>>()
                .join(", ");
            return ToolResult::error(format!(
                "tool '{raw_name}' not found. Available tools: {available}"
            ));
        }
    };

    {
        let Ok(mut active) = ctx.active_tools.write() else {
            return ToolResult::error("internal error: active_tools lock poisoned");
        };
        if active.contains(tool_name) {
            return ToolResult::text(format!("'{raw_name}' is already active."));
        }
        active.insert(tool_name.clone());
    }

    tracing::info!(
        target_tool = %tool_name,
        session_id = %ctx.session_id,
        turn_number = ctx.turn_number,
        source = "effective_surface",
        "enable_tool: activated tool"
    );

    ToolResult::text(format!("Activated '{raw_name}': {description}"))
}

fn enable_tool_def() -> ToolDef {
    // WHY: enable_tool mutates the session's active tool surface, changing which
    // lazy tools and provider server tools are visible in later turns. It must
    // therefore require a non-read capability and be approval-worthy.
    ToolDef {
        name: ToolName::from_static(ENABLE_TOOL),
        description: "Activate a tool for this session. Some tools are not loaded by default \
                      and must be enabled first. Call with the tool name to activate it."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "name".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "Name of the tool to activate".to_owned(),
                    enum_values: None,
                    default: None,
                    ..Default::default()
                },
            )]),
            required: vec!["name".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Command],
        tags: vec![ToolTag::Edit, ToolTag::Execute],
    }
}

/// Register the `enable_tool` tool into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(enable_tool_def(), Box::new(EnableToolExecutor))?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolRegistry;
    use crate::surface::SurfaceInputs;
    use crate::testing::install_crypto_provider;
    use crate::types::{
        ApprovalRequirement, ServerToolConfig, ToolContext, ToolGroupPolicy, ToolHttpClients,
        ToolInput, ToolServices,
    };

    use super::*;

    fn mock_ctx_with_catalog(catalog: Vec<(ToolName, String)>) -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                working_checkpoint_store: None,
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                http_clients: ToolHttpClients::for_tests(),
                secret_vault: hermeneus::secret::SecretVault::new(),
                lazy_tool_catalog: catalog,
                server_tool_config: ServerToolConfig::default(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn make_input(tool_name: &str) -> ToolInput {
        ToolInput {
            name: ToolName::from_static(ENABLE_TOOL),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({"name": tool_name}),
        }
    }

    #[tokio::test]
    async fn activate_known_tool() {
        let ctx = mock_ctx_with_catalog(vec![(
            ToolName::from_static("web_search"),
            "Search the web".to_owned(),
        )]);

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("execute");

        assert!(!result.is_error, "expected result.is_error to be false");
        assert!(
            result
                .content
                .text_summary()
                .contains("Activated 'web_search'")
        );

        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let active = ctx.active_tools.read().expect("lock poisoned");
        assert!(
            active.contains(&ToolName::from_static("web_search")),
            "expected active.contains(&ToolName::from_static(\"web_search\")) to be true"
        );
    }

    #[tokio::test]
    async fn unknown_tool_lists_available() {
        let ctx = mock_ctx_with_catalog(vec![
            (
                ToolName::from_static("web_search"),
                "Search the web".to_owned(),
            ),
            (ToolName::from_static("web_fetch"), "Fetch a URL".to_owned()),
        ]);

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("nonexistent"), &ctx)
            .await
            .expect("execute");

        assert!(result.is_error, "expected result.is_error to be true");
        let text = result.content.text_summary();
        assert!(
            text.contains("web_search"),
            "expected text.contains(\"web_search\") to be true"
        );
        assert!(
            text.contains("web_fetch"),
            "expected text.contains(\"web_fetch\") to be true"
        );
    }

    #[tokio::test]
    async fn double_activate_is_idempotent() {
        let ctx = mock_ctx_with_catalog(vec![(
            ToolName::from_static("web_search"),
            "Search the web".to_owned(),
        )]);

        let executor = EnableToolExecutor;
        executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("first");

        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("second");

        assert!(!result.is_error, "expected result.is_error to be false");
        assert!(
            result.content.text_summary().contains("already active"),
            "expected result.content.text_summary().contains(\"already active\") to be true"
        );
    }

    fn mock_ctx_with_server_tools(config: ServerToolConfig) -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                working_checkpoint_store: None,
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                http_clients: ToolHttpClients::for_tests(),
                secret_vault: hermeneus::secret::SecretVault::new(),
                lazy_tool_catalog: vec![],
                server_tool_config: config,
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    #[tokio::test]
    async fn enable_tool_activates_server_web_search() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig {
            web_search: true,
            web_search_max_uses: Some(5),
            code_execution: false,
        });

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("execute");

        assert!(!result.is_error, "expected result.is_error to be false");
        assert!(
            result
                .content
                .text_summary()
                .contains("Activated 'web_search'")
        );

        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let active = ctx.active_tools.read().expect("lock poisoned");
        assert!(
            active.contains(&ToolName::from_static("web_search")),
            "expected active.contains(&ToolName::from_static(\"web_search\")) to be true"
        );
    }

    #[tokio::test]
    async fn enable_tool_uses_bound_effective_surface() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig {
            web_search: true,
            web_search_max_uses: Some(5),
            code_execution: false,
        });
        let active = HashSet::new();
        let policy = ToolGroupPolicy::AllowAll {
            reason: "test".to_owned(),
        };
        let registry = ToolRegistry::new();
        let services = ctx.services.as_ref().expect("services");
        let surface = Arc::new(registry.effective_surface(SurfaceInputs {
            policy: &policy,
            allowlist: None,
            active: &active,
            server_tools: &[],
            server_tool_config: Some(&services.server_tool_config),
        }));
        let _binding = ctx.bind_effective_surface(surface);

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("execute");

        assert!(!result.is_error, "expected result.is_error to be false");
        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let active = ctx.active_tools.read().expect("lock poisoned");
        assert!(active.contains(&ToolName::from_static("web_search")));
    }

    #[tokio::test]
    async fn enable_tool_server_tool_not_in_disabled_config() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig::default());

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("web_search"), &ctx)
            .await
            .expect("execute");

        assert!(result.is_error, "expected result.is_error to be true");
        assert!(
            result.content.text_summary().contains("not found"),
            "expected result.content.text_summary().contains(\"not found\") to be true"
        );
    }

    #[tokio::test]
    async fn enable_tool_lists_server_tools_in_available() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig {
            web_search: true,
            web_search_max_uses: None,
            code_execution: true,
        });

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("nonexistent"), &ctx)
            .await
            .expect("execute");

        assert!(result.is_error, "expected result.is_error to be true");
        let text = result.content.text_summary();
        assert!(
            text.contains("web_search"),
            "expected text.contains(\"web_search\") to be true"
        );
        assert!(
            text.contains("code_execution"),
            "expected text.contains(\"code_execution\") to be true"
        );
    }

    #[test]
    fn enable_tool_def_is_capability_mutation() {
        let def = enable_tool_def();
        assert_ne!(
            def.reversibility,
            Reversibility::FullyReversible,
            "enable_tool must not be fully reversible"
        );
        assert!(
            !def.groups.contains(&ToolGroupId::Read),
            "enable_tool must not be in the Read group"
        );
        assert!(
            def.groups.contains(&ToolGroupId::Command),
            "enable_tool must require the Command group"
        );
        assert!(
            def.tags.contains(&ToolTag::Execute),
            "enable_tool should carry the Execute tag"
        );
    }

    #[test]
    fn enable_tool_not_presented_to_read_only_policy() {
        let mut reg = ToolRegistry::new();
        register(&mut reg).expect("register enable_tool");

        let defs = reg.definitions_for_groups(&[ToolGroupId::Read]);
        assert!(
            defs.iter().all(|d| d.name.as_str() != ENABLE_TOOL),
            "enable_tool should not appear under a read-only group policy"
        );
    }

    #[tokio::test]
    async fn enable_tool_code_execution_not_available_when_disabled() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig::default());

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("code_execution"), &ctx)
            .await
            .expect("execute");

        assert!(result.is_error, "expected result.is_error to be true");
        assert!(
            result.content.text_summary().contains("not found"),
            "expected result.content.text_summary().contains(\"not found\") to be true"
        );
    }

    #[tokio::test]
    async fn enable_tool_code_execution_records_active_and_approval_is_required() {
        let ctx = mock_ctx_with_server_tools(ServerToolConfig {
            web_search: false,
            web_search_max_uses: None,
            code_execution: true,
        });

        let executor = EnableToolExecutor;
        let result = executor
            .execute(&make_input("code_execution"), &ctx)
            .await
            .expect("execute");

        assert!(!result.is_error, "expected result.is_error to be false");
        assert!(
            result
                .content
                .text_summary()
                .contains("Activated 'code_execution'"),
            "expected activation message for code_execution"
        );

        #[expect(
            clippy::expect_used,
            reason = "test assertion: poisoned lock means a test bug"
        )]
        let active = ctx.active_tools.read().expect("lock poisoned");
        assert!(
            active.contains(&ToolName::from_static("code_execution")),
            "expected active.contains(&ToolName::from_static(\"code_execution\")) to be true"
        );

        let mut reg = ToolRegistry::new();
        register(&mut reg).expect("register enable_tool");
        let approval = reg
            .approval_requirement_for_input(&make_input("code_execution"))
            .expect("approval requirement");
        assert!(
            matches!(
                approval,
                ApprovalRequirement::Required | ApprovalRequirement::Mandatory
            ),
            "expected approval to be Required or Mandatory, got {approval:?}"
        );
    }
}
