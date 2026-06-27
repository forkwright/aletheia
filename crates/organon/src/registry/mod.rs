//! Tool registry: the single source of truth for available tools.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use indexmap::IndexMap;
use snafu::{IntoError as _, ensure};
use tracing::{Instrument, info_span};

use koina::id::ToolName;

use crate::error::{self, Result};
use crate::surface::{
    ENABLE_TOOL, EffectiveToolSurface, RegistrySurfaceTool, SurfaceInputs,
    deferred_schema_placeholder,
};
use crate::types::{
    ApprovalRequirement, Reversibility, ToolCallCapability, ToolCallCapabilityRule,
    ToolCallMetadata, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolGroupPolicy, ToolInput,
    ToolOrigin, ToolOutcome, ToolResult, ToolTag,
};

/// Shared, mutable snapshot used by the `tool_schema` meta-tool.
///
/// WHY: The registry owns the `tool_schema` executor, so the executor cannot
/// hold a reference back to the registry.  A lock-protected map lets the
/// registry publish a finalized snapshot after all late tools (domain packs,
/// external HTTP/MCP tools) have been registered without creating an ownership
/// cycle.
pub(crate) type ToolSchemaSnapshot = Arc<RwLock<HashMap<String, String>>>;

/// The trait tool implementations must satisfy.
///
/// Uses `Pin<Box<dyn Future>>` for object-safety with async dispatch.
///
/// # Errors
///
/// Implementations may return `ExecutionFailed` if the tool
/// cannot complete its operation, or `InvalidInput` if the
/// provided arguments fail validation.
///
/// # Examples
///
/// ```no_run
/// use std::future::Future;
/// use std::pin::Pin;
/// use organon::registry::ToolExecutor;
/// use organon::types::{ToolContext, ToolInput, ToolResult};
///
/// struct MyTool;
///
/// impl ToolExecutor for MyTool {
///     fn execute<'a>(
///         &'a self,
///         _input: &'a ToolInput,
///         _ctx: &'a ToolContext,
///     ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
///         Box::pin(async move { Ok(ToolResult::text("done")) })
///     }
/// }
/// ```
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
    call_capability: Option<ToolCallCapabilityRule>,
    executor: Box<dyn ToolExecutor>,
}

/// Registry of available tools.
///
/// Tools are registered at startup and looked up by name during execution.
/// The registry is the single source of truth for what tools an agent can use.
///
/// # Examples
///
/// ```no_run
/// use organon::registry::ToolRegistry;
///
/// let mut registry = ToolRegistry::new();
/// // Tools are registered at startup with their definitions and executors.
/// // See the `builtins` module for built-in tool implementations.
/// ```
pub struct ToolRegistry {
    // kanon:ignore RUST/pub-visibility
    tools: IndexMap<ToolName, RegisteredTool>,
    /// Origin metadata for externally-provided tools, keyed by local name.
    ///
    /// WHY: Origin is stored separately from [`ToolDef`] so that existing
    /// built-in tool definitions do not need to change, while MCP and HTTP
    /// tool planes can still publish server/remote provenance for diagnostics
    /// and approval display.
    origins: HashMap<ToolName, ToolOrigin>,
    /// Snapshot state for the `tool_schema` meta-tool.  `None` until
    /// `tool_schema` is registered.
    tool_schema_snapshot: Option<ToolSchemaSnapshot>,
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
            origins: HashMap::new(),
            tool_schema_snapshot: None,
        }
    }

    /// Register a tool. Fails if a tool with the same name already exists.
    ///
    /// # Errors
    ///
    /// Returns an error if a tool with the same name is already registered.
    pub fn register(&mut self, def: ToolDef, executor: Box<dyn ToolExecutor>) -> Result<()> {
        // kanon:ignore RUST/pub-visibility
        self.register_inner(def, None, executor)
    }

    /// Register a tool with argument-driven call capability metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if a tool with the same name is already registered.
    pub fn register_with_call_capability(
        &mut self,
        def: ToolDef,
        call_capability: ToolCallCapabilityRule,
        executor: Box<dyn ToolExecutor>,
    ) -> Result<()> {
        self.register_inner(def, Some(call_capability), executor)
    }

    fn register_inner(
        &mut self,
        def: ToolDef,
        call_capability: Option<ToolCallCapabilityRule>,
        executor: Box<dyn ToolExecutor>,
    ) -> Result<()> {
        ensure!(
            !self.tools.contains_key(&def.name),
            error::DuplicateToolSnafu {
                name: def.name.clone()
            }
        );
        self.tools.insert(
            def.name.clone(),
            RegisteredTool {
                def,
                call_capability,
                executor,
            },
        );
        Ok(())
    }

    fn tools_for_policy<'s, 'p>(
        &'s self,
        policy: &'p ToolGroupPolicy,
    ) -> impl Iterator<Item = &'s RegisteredTool> + 'p
    where
        's: 'p,
    {
        self.tools
            .values()
            .filter(move |tool| policy.permits(&tool.def.groups))
    }

    fn active_tools_for_policy<'s, 'p>(
        &'s self,
        active: &'p HashSet<ToolName>,
        policy: &'p ToolGroupPolicy,
    ) -> impl Iterator<Item = &'s RegisteredTool> + 'p
    where
        's: 'p,
    {
        self.tools_for_policy(policy).filter(move |tool| {
            tool.def.auto_activate
                || active.contains(&tool.def.name)
                || tool.def.name.as_str() == ENABLE_TOOL
        })
    }

    fn call_capability_for_tool(
        tool: &RegisteredTool,
        input: &ToolInput,
    ) -> Result<ToolCallCapability> {
        match &tool.call_capability {
            Some(rule) => rule.classify(&input.arguments).map_err(|reason| {
                error::InvalidInputSnafu {
                    name: input.name.clone(),
                    reason,
                }
                .build()
            }),
            None => Ok(ToolCallCapability::new(
                tool.def.groups.clone(),
                tool.def.reversibility,
            )),
        }
    }

    /// Look up a tool definition by name.
    #[must_use]
    pub fn get_def(&self, name: &ToolName) -> Option<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        self.tools.get(name).map(|t| &t.def)
    }

    /// Attach origin metadata to an already-registered tool.
    ///
    /// WHY: External tool planes register the executor separately from the
    /// provenance metadata. This lets them publish server/remote names after
    /// a successful registration without changing the [`ToolDef`] shape used
    /// by every built-in tool.
    pub fn set_origin(&mut self, name: ToolName, origin: ToolOrigin) {
        // kanon:ignore RUST/pub-visibility
        if self.tools.contains_key(&name) {
            self.origins.insert(name, origin);
        }
    }

    /// Look up the origin metadata for a tool by name.
    #[must_use]
    pub fn origin(&self, name: &ToolName) -> Option<&ToolOrigin> {
        // kanon:ignore RUST/pub-visibility
        self.origins.get(name)
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

        // Expand file refs in tool arguments before dispatch.
        let expanded_args =
            crate::interp::expand_file_refs_in_json(&input.arguments, &ctx.workspace)
                .map_err(|e| error::InterpSnafu.into_error(e))?;
        let mut expanded_input = input.clone();
        expanded_input.arguments = expanded_args;

        // Validate expanded arguments against the tool's declared input schema.
        // WHY: Catch malformed arguments before the executor runs so callers get
        // consistent, schema-driven errors instead of internal parsing failures.
        tool.def
            .input_schema
            .validate(&input.name, &expanded_input.arguments)?;

        let span = info_span!("tool_execute",
            tool.name = %input.name,
            tool.reversibility = %tool.def.reversibility,
            tool.approval = %ApprovalRequirement::from(tool.def.reversibility),
            tool.duration_ms = tracing::field::Empty,
            tool.status = tracing::field::Empty,
        );
        let start = Instant::now();
        // WHY: Track the invocation as live from just before the executor call
        // until the guard drops. Cancellation or normal completion both remove
        // the entry, so the ops surface never shows stale live calls.
        let _active_guard = crate::metrics::track_invocation(input.name.as_str());
        // WHY: `.instrument(span)` instead of `span.enter()` so the span context
        // propagates correctly across `.await` points. `span.enter()` in async code
        // keeps the span entered on the current thread even when suspended, which
        // causes incorrect parent attribution for concurrent tasks (#3384).
        let result = tool
            .executor
            .execute(&expanded_input, ctx)
            .instrument(span.clone())
            .await;
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        span.record("tool.duration_ms", duration_ms);
        match &result {
            Ok(r) if matches!(r.outcome, ToolOutcome::PartialSuccess(_)) => {
                span.record("tool.status", "partial");
            }
            Ok(r) if !r.is_error => {
                span.record("tool.status", "ok");
            }
            _ => {
                span.record("tool.status", "error");
            }
        }
        let status = match &result {
            Ok(r) if matches!(r.outcome, ToolOutcome::PartialSuccess(_)) => {
                crate::metrics::InvocationStatus::Partial
            }
            Ok(r) if !r.is_error => crate::metrics::InvocationStatus::Ok,
            _ => crate::metrics::InvocationStatus::Error,
        };
        crate::metrics::record_invocation(
            input.name.as_str(),
            start.elapsed().as_secs_f64(),
            status,
        );
        result
    }

    /// Execute a tool by name, checking that the tool's groups satisfy the
    /// policy for the calling role.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ToolGroupViolation`] if the tool's groups do not
    /// satisfy the policy.
    pub async fn execute_checked(
        &self,
        input: &ToolInput,
        ctx: &ToolContext,
        role: &str,
        policy: &ToolGroupPolicy,
    ) -> Result<ToolResult> {
        let tool = self.tools.get(&input.name).ok_or_else(|| {
            error::ToolNotFoundSnafu {
                name: input.name.clone(),
            }
            .build()
        })?;

        let call_capability = Self::call_capability_for_tool(tool, input)?;
        if !policy.permits(&call_capability.groups) {
            return Err(error::ToolGroupViolationSnafu {
                role: role.to_owned(),
                tool: input.name.as_str().to_owned(),
                allowed: policy.allowed_groups().to_vec(),
                tool_groups: call_capability.groups,
            }
            .build());
        }

        self.execute(input, ctx).await
    }

    /// All registered tool definitions, in insertion order.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn definitions(&self) -> Vec<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        self.tools.values().map(|t| &t.def).collect()
    }

    /// Resolve the effective tool surface for one LLM iteration.
    #[must_use]
    pub fn effective_surface(&self, inputs: SurfaceInputs<'_>) -> EffectiveToolSurface {
        EffectiveToolSurface::resolve(
            self.tools.values().map(|tool| RegistrySurfaceTool {
                def: &tool.def,
                call_capability: tool.call_capability.as_ref(),
                origin: self.origins.get(&tool.def.name),
            }),
            inputs,
        )
    }

    /// Tool definitions filtered by category.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn definitions_for_category(&self, category: ToolCategory) -> Vec<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        self.tools
            .values()
            .filter(|t| t.def.category == category)
            .map(|t| &t.def)
            .collect()
    }

    /// Tool definitions whose tags intersect any of `tags` (union semantics).
    ///
    /// Cross-category lookup — different from [`Self::definitions_for_category`]
    /// which is structural.  Returns an empty Vec when `tags` is empty.
    ///
    /// # Complexity
    ///
    /// O(n * m) where n is the number of registered tools and m is the length
    /// of `tags` (typically small).
    #[must_use]
    pub fn definitions_for_tags(&self, tags: &[ToolTag]) -> Vec<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        if tags.is_empty() {
            return Vec::new();
        }
        self.tools
            .values()
            .filter(|t| t.def.tags.iter().any(|tag| tags.contains(tag)))
            .map(|t| &t.def)
            .collect()
    }

    /// Tool definitions filtered by allowed tool groups.
    ///
    /// Returns only tools whose groups satisfy the provided policy.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn definitions_for_groups(&self, allowed_groups: &[ToolGroupId]) -> Vec<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        self.definitions_for_policy(&ToolGroupPolicy::groups(allowed_groups.to_vec()))
    }

    /// Tool definitions filtered by explicit tool-group policy.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn definitions_for_policy(&self, policy: &ToolGroupPolicy) -> Vec<&ToolDef> {
        // kanon:ignore RUST/pub-visibility
        self.tools_for_policy(policy)
            .map(|tool| &tool.def)
            .collect()
    }

    /// Convert registered tools to the LLM wire format.
    ///
    /// Produces `ToolDefinition` structs suitable for `CompletionRequest::tools`.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn to_hermeneus_tools(&self) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.tools
            .values()
            .map(|t| hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.to_json_schema(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Convert registered tools to the LLM wire format, filtered by allowed groups.
    ///
    /// An empty group list denies all tools.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn to_hermeneus_tools_for_groups(
        &self,
        allowed_groups: &[ToolGroupId],
    ) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.to_hermeneus_tools_for_policy(&ToolGroupPolicy::groups(allowed_groups.to_vec()))
    }

    /// Convert registered tools to the LLM wire format, filtered by policy.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn to_hermeneus_tools_for_policy(
        &self,
        policy: &ToolGroupPolicy,
    ) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.tools_for_policy(policy)
            .map(|t| hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.to_json_schema(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Convert tools to LLM wire format with **name + description only** (no `input_schema`).
    ///
    /// Used by the `deferred-schemas` feature path.  The full schema for any
    /// tool is retrievable on demand via the `tool_schema` meta-tool.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[cfg(feature = "deferred-schemas")]
    #[must_use]
    pub fn to_hermeneus_tools_summaries(&self) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.tools
            .values()
            .map(|t| hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                // WHY: deferred-schemas mode — omit full input_schema; agents call
                // `tool_schema` to retrieve the schema before invoking a tool.
                input_schema: deferred_schema_placeholder(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Convert tools to LLM wire format with **name + description only**, filtered by
    /// allowed groups.
    ///
    /// An empty group list denies all tools.
    #[cfg(feature = "deferred-schemas")]
    #[must_use]
    pub fn to_hermeneus_tools_summaries_for_groups(
        &self,
        allowed_groups: &[ToolGroupId],
    ) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.to_hermeneus_tools_summaries_for_policy(&ToolGroupPolicy::groups(
            allowed_groups.to_vec(),
        ))
    }

    /// Convert tools to LLM wire format with **name + description only**, filtered by
    /// policy.
    #[cfg(feature = "deferred-schemas")]
    #[must_use]
    pub fn to_hermeneus_tools_summaries_for_policy(
        &self,
        policy: &ToolGroupPolicy,
    ) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.tools_for_policy(policy)
            .map(|t| hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: deferred_schema_placeholder(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Convert tools to LLM wire format with **name + description only**, filtered by
    /// activation state.
    ///
    /// Mirrors [`Self::to_hermeneus_tools_filtered`] but omits `input_schema`.
    /// Used by the `deferred-schemas` feature path.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[cfg(feature = "deferred-schemas")]
    #[must_use]
    pub fn to_hermeneus_tools_summaries_filtered(
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<hermeneus::types::ToolDefinition> {
        self.tools
            .values()
            .filter(|t| {
                t.def.auto_activate
                    || active.contains(&t.def.name)
                    || t.def.name.as_str() == ENABLE_TOOL
            })
            .map(|t| hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: deferred_schema_placeholder(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Convert tools to LLM wire format with **name + description only**, filtered by
    /// activation state and allowed groups.
    ///
    /// An empty group list denies all activation-filtered tools.
    #[cfg(feature = "deferred-schemas")]
    #[must_use]
    pub fn to_hermeneus_tools_summaries_filtered_for_groups(
        &self,
        active: &HashSet<ToolName>,
        allowed_groups: &[ToolGroupId],
    ) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.to_hermeneus_tools_summaries_filtered_for_policy(
            active,
            &ToolGroupPolicy::groups(allowed_groups.to_vec()),
        )
    }

    /// Convert tools to LLM wire format with **name + description only**, filtered by
    /// activation state and policy.
    #[cfg(feature = "deferred-schemas")]
    #[must_use]
    pub fn to_hermeneus_tools_summaries_filtered_for_policy(
        &self,
        active: &HashSet<ToolName>,
        policy: &ToolGroupPolicy,
    ) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.active_tools_for_policy(active, policy)
            .map(|t| hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: deferred_schema_placeholder(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Compute the serialized byte sizes of the eager (full-schema) and deferred
    /// (name+description-only) tool-declaration payloads.
    ///
    /// Returns `(summary_bytes, schema_bytes)` where:
    /// - `summary_bytes` is the byte count when only name+description are sent.
    /// - `schema_bytes` is the byte count of the full eager payload.
    ///
    /// Emits a `tracing::info!` event with both values and the tool count so
    /// operators can observe the cost difference at session startup.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools; two JSON serializations.
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub fn schema_byte_sizes(&self) -> (usize, usize) {
        // kanon:ignore RUST/pub-visibility
        let tool_count = self.tools.len();

        let summaries: Vec<serde_json::Value> = self
            .tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.def.name.as_str(),
                    "description": t.def.description,
                    "input_schema": deferred_schema_placeholder()
                })
            })
            .collect();
        let summary_bytes = serde_json::to_string(&summaries).map_or(0, |s| s.len());

        let full: Vec<serde_json::Value> = self
            .tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.def.name.as_str(),
                    "description": t.def.description,
                    "input_schema": t.def.input_schema.to_json_schema()
                })
            })
            .collect();
        let schema_bytes = serde_json::to_string(&full).map_or(0, |s| s.len());

        tracing::info!(
            tool_count,
            summary_bytes,
            schema_bytes,
            "organon tool-declaration sizes: eager={schema_bytes}B deferred={summary_bytes}B ({tool_count} tools)"
        );

        (summary_bytes, schema_bytes)
    }

    /// Convert tools to LLM wire format, filtered by activation state.
    ///
    /// Includes tools where:
    /// - `auto_activate == true` (always-on essentials)
    /// - name is in the `active` set (dynamically activated via `enable_tool`)
    /// - name is `enable_tool` (always available so agents can activate more)
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn to_hermeneus_tools_filtered(
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<hermeneus::types::ToolDefinition> {
        self.tools
            .values()
            .filter(|t| {
                t.def.auto_activate
                    || active.contains(&t.def.name)
                    || t.def.name.as_str() == ENABLE_TOOL
            })
            .map(|t| hermeneus::types::ToolDefinition {
                name: t.def.name.as_str().to_owned(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.to_json_schema(),
                disable_passthrough: None,
            })
            .collect()
    }

    /// Convert tools to LLM wire format, filtered by activation state and allowed groups.
    ///
    /// An empty group list denies all activation-filtered tools.
    #[must_use]
    pub fn to_hermeneus_tools_filtered_for_groups(
        &self,
        active: &HashSet<ToolName>,
        allowed_groups: &[ToolGroupId],
    ) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.to_hermeneus_tools_filtered_for_policy(
            active,
            &ToolGroupPolicy::groups(allowed_groups.to_vec()),
        )
    }

    /// Convert tools to LLM wire format, filtered by activation state and policy.
    #[must_use]
    pub fn to_hermeneus_tools_filtered_for_policy(
        &self,
        active: &HashSet<ToolName>,
        policy: &ToolGroupPolicy,
    ) -> Vec<hermeneus::types::ToolDefinition> {
        // kanon:ignore RUST/pub-visibility
        self.active_tools_for_policy(active, policy)
            .map(|t| hermeneus::types::ToolDefinition {
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

    /// Classify one concrete tool call.
    ///
    /// # Errors
    ///
    /// Returns an error if the tool is missing or its call-level selector is invalid.
    pub fn call_capability(&self, input: &ToolInput) -> Result<ToolCallCapability> {
        let tool = self.tools.get(&input.name).ok_or_else(|| {
            error::ToolNotFoundSnafu {
                name: input.name.clone(),
            }
            .build()
        })?;
        Self::call_capability_for_tool(tool, input)
    }

    /// Check whether a concrete call satisfies a policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the tool is missing or its call-level selector is invalid.
    pub fn permits_call(&self, input: &ToolInput, policy: &ToolGroupPolicy) -> Result<bool> {
        let call_capability = self.call_capability(input)?;
        Ok(policy.permits(&call_capability.groups))
    }

    /// Determine what approval is required for a concrete tool call.
    ///
    /// # Errors
    ///
    /// Returns an error if the tool is missing or its call-level selector is invalid.
    pub fn approval_requirement_for_input(&self, input: &ToolInput) -> Result<ApprovalRequirement> {
        let call_capability = self.call_capability(input)?;
        Ok(ApprovalRequirement::from(call_capability.reversibility))
    }

    /// Build session-log metadata for a tool call.
    #[must_use]
    pub fn call_metadata(&self, name: &ToolName, dry_run: bool) -> Option<ToolCallMetadata> {
        // kanon:ignore RUST/pub-visibility
        self.tools.get(name).map(|t| ToolCallMetadata {
            reversibility: t.def.reversibility,
            approval: ApprovalRequirement::from(t.def.reversibility),
            dry_run,
            origin: self.origins.get(name).cloned(),
        })
    }

    /// Build session-log metadata for a concrete tool call.
    ///
    /// # Errors
    ///
    /// Returns an error if the tool is missing or its call-level selector is invalid.
    pub fn call_metadata_for_input(
        &self,
        input: &ToolInput,
        dry_run: bool,
    ) -> Result<ToolCallMetadata> {
        let call_capability = self.call_capability(input)?;
        Ok(ToolCallMetadata {
            reversibility: call_capability.reversibility,
            approval: ApprovalRequirement::from(call_capability.reversibility),
            dry_run,
            origin: self.origins.get(&input.name).cloned(),
        })
    }

    /// Catalog of lazy tools (`auto_activate=false`) for the `enable_tool` executor.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of registered tools.
    #[must_use]
    pub fn lazy_tool_catalog(&self) -> Vec<(ToolName, String)> {
        // kanon:ignore RUST/pub-visibility
        self.tools
            .values()
            .filter(|t| !t.def.auto_activate && t.def.name.as_str() != ENABLE_TOOL)
            .map(|t| (t.def.name.clone(), t.def.description.clone()))
            .collect()
    }

    /// Attach the shared snapshot used by the `tool_schema` executor.
    ///
    /// WHY: The registry must be able to refresh the snapshot after late tools
    /// are registered.  Keeping the `Arc` here avoids a back-reference from the
    /// executor to the registry.
    pub(crate) fn set_tool_schema_snapshot(&mut self, snapshot: Option<ToolSchemaSnapshot>) {
        self.tool_schema_snapshot = snapshot;
    }

    /// Finalize the `tool_schema` snapshot to include every tool currently
    /// registered in the registry.
    ///
    /// Call this after all late registrations (domain packs, external HTTP/MCP
    /// tools) are complete so `tool_schema` can serve schemas for the complete
    /// tool set.
    ///
    /// # Errors
    ///
    /// Returns an error if `tool_schema` has not been registered, or if the
    /// snapshot lock is poisoned.
    pub fn finalize_tool_schema(&mut self) -> Result<()> {
        let snapshot = self
            .tool_schema_snapshot
            .as_ref()
            .ok_or_else(|| error::ToolSchemaNotRegisteredSnafu.build())?;

        let schemas: HashMap<String, String> = self
            .definitions()
            .into_iter()
            .filter_map(|def| {
                let schema = def.input_schema.to_json_schema();
                match serde_json::to_string_pretty(&schema) {
                    Ok(json) => Some((def.name.as_str().to_owned(), json)),
                    Err(e) => {
                        tracing::warn!(
                            tool.name = def.name.as_str(),
                            error = %e,
                            "tool_schema: failed to serialize schema during finalize; tool will be unavailable via tool_schema"
                        );
                        None
                    }
                }
            })
            .collect();

        let schema_count = schemas.len();
        let mut guard = snapshot
            .write()
            .map_err(|_poisoned| error::SchemaSnapshotPoisonedSnafu.build())?;
        *guard = schemas;

        tracing::info!(
            schema_count,
            "tool_schema: finalized snapshot with {schema_count} schemas"
        );
        Ok(())
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[path = "tag_tests.rs"]
mod tag_tests;
