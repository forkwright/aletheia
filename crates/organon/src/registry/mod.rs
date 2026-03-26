//! Tool registry: the single source of truth for available tools.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use indexmap::IndexMap;
use snafu::ensure;
use tracing::info_span;

use aletheia_koina::id::ToolName;

use crate::error::{self, Result};
use crate::types::{
    ApprovalRequirement, Reversibility, ToolCallMetadata, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

/// The trait tool implementations must satisfy.
///
/// Uses `Pin<Box<dyn Future>>` for object-safety with async dispatch.
pub trait ToolExecutor: Send + Sync {
    // kanon:ignore RUST/pub-visibility
    /// Execute the tool with the given input and context.
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>>;
}

struct RegisteredTool {
    def: ToolDef,
    executor: Box<dyn ToolExecutor>,
}

/// Registry of available tools.
///
/// Tools are registered at startup and looked up by name during execution.
/// The registry is the single source of truth for what tools an agent can use.
pub struct ToolRegistry {
    // kanon:ignore RUST/pub-visibility
    tools: IndexMap<ToolName, RegisteredTool>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            tools: IndexMap::new(),
        }
    }

    /// Register a tool. Fails if a tool with the same name already exists.
    ///
    /// # Errors
    ///
    /// Returns an error if a tool with the same name is already registered.
    pub fn register(&mut self, def: ToolDef, executor: Box<dyn ToolExecutor>) -> Result<()> {
        // kanon:ignore RUST/pub-visibility

        ensure!(
            !self.tools.contains_key(&def.name),
            error::DuplicateToolSnafu {
                name: def.name.clone()
            }
        );
        self.tools
            .insert(def.name.clone(), RegisteredTool { def, executor });
        Ok(())
    }

    /// Look up a tool definition by name.
    #[must_use]
    pub fn get_def(&self, name: &ToolName) -> Option<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        self.tools.get(name).map(|t| &t.def)
    }

    /// Execute a tool by name.
    ///
    /// # Errors
    ///
    /// Returns an error if no tool with the given name is registered.
    /// Returns the tool executor's error if execution fails.
    pub async fn execute(&self, input: &ToolInput, ctx: &ToolContext) -> Result<ToolResult> {
        let tool = self.tools.get(&input.name).ok_or_else(|| {
            error::ToolNotFoundSnafu {
                name: input.name.clone(),
            }
            .build()
        })?;
        let span = info_span!("tool_execute",
            tool.name = %input.name,
            tool.reversibility = %tool.def.reversibility,
            tool.approval = %ApprovalRequirement::from(tool.def.reversibility),
            tool.duration_ms = tracing::field::Empty,
            tool.status = tracing::field::Empty,
        );
        let _guard = span.enter();
        let start = Instant::now();
        let result = tool.executor.execute(input, ctx).await;
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        span.record("tool.duration_ms", duration_ms);
        match &result {
            Ok(r) if !r.is_error => span.record("tool.status", "ok"),
            _ => span.record("tool.status", "error"),
        };
        crate::metrics::record_invocation(
            input.name.as_str(),
            start.elapsed().as_secs_f64(),
            matches!(&result, Ok(r) if !r.is_error),
        );
        result
    }

    /// All registered tool definitions, in insertion order.
    #[must_use]
    pub fn definitions(&self) -> Vec<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        self.tools.values().map(|t| &t.def).collect()
    }

    /// Tool definitions filtered by category.
    #[must_use]
    pub fn definitions_for_category(&self, category: ToolCategory) -> Vec<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        self.tools
            .values()
            .filter(|t| t.def.category == category)
            .map(|t| &t.def)
            .collect()
    }

    /// Convert registered tools to the LLM wire format.
    ///
    /// Produces `ToolDefinition` structs suitable for `CompletionRequest::tools`.
    #[must_use]
    pub fn to_hermeneus_tools(&self) -> Vec<aletheia_hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.tools
            .values()
            .map(|t| aletheia_hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.to_json_schema(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Convert tools to LLM wire format, filtered by activation state.
    ///
    /// Includes tools where:
    /// - `auto_activate == true` (always-on essentials)
    /// - name is in the `active` set (dynamically activated via `enable_tool`)
    /// - name is `enable_tool` (always available so agents can activate more)
    #[must_use]
    pub fn to_hermeneus_tools_filtered(
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<aletheia_hermeneus::types::ToolDefinition> {
        self.tools
            .values()
            .filter(|t| {
                t.def.auto_activate
                    || active.contains(&t.def.name)
                    || t.def.name.as_str() == "enable_tool"
            })
            .map(|t| aletheia_hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.to_json_schema(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Look up the reversibility level for a tool by name.
    #[must_use]
    pub fn reversibility(&self, name: &ToolName) -> Option<Reversibility> {
        // kanon:ignore RUST/pub-visibility
        self.tools.get(name).map(|t| t.def.reversibility)
    }

    /// Determine what approval is required for a tool call.
    #[must_use]
    pub fn approval_requirement(&self, name: &ToolName) -> Option<ApprovalRequirement> {
        // kanon:ignore RUST/pub-visibility
        self.tools
            .get(name)
            .map(|t| ApprovalRequirement::from(t.def.reversibility))
    }

    /// Build session-log metadata for a tool call.
    #[must_use]
    pub fn call_metadata(&self, name: &ToolName, dry_run: bool) -> Option<ToolCallMetadata> {
        // kanon:ignore RUST/pub-visibility
        self.tools.get(name).map(|t| ToolCallMetadata {
            reversibility: t.def.reversibility,
            approval: ApprovalRequirement::from(t.def.reversibility),
            dry_run,
        })
    }

    /// Catalog of lazy tools (`auto_activate=false`) for the `enable_tool` executor.
    #[must_use]
    pub fn lazy_tool_catalog(&self) -> Vec<(ToolName, String)> {
        // kanon:ignore RUST/pub-visibility
        self.tools
            .values()
            .filter(|t| !t.def.auto_activate && t.def.name.as_str() != "enable_tool")
            .map(|t| (t.def.name.clone(), t.def.description.clone()))
            .collect()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
#[path = "registry_tests.rs"]
mod tests;
