// kanon:ignore RUST/file-too-long — provider dispatch loop; extraction into submodules tracked in #3752
//! Dispatch helpers: tool execution, signal classification, message conversion.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use tracing::{debug, warn};

use tokio::sync::mpsc;

use hermeneus::secret::substitute_in_json;
use hermeneus::types::{ContentBlock, ToolDefinition, ToolResultBlock, ToolResultContent};
use koina::id::ToolName;
use organon::registry::ToolRegistry;
use organon::surface::{
    DenialReason, EffectiveToolSurface, SurfaceAvailability, SurfaceEntryKind, SurfaceLookup,
};
use organon::types::{ApprovalRequirement, ToolContext, ToolInput};

use crate::approval::{ApprovalChoice, ApprovalGate};
use crate::error;
use crate::pipeline::{InteractionSignal, LoopDetector, LoopVerdict, ToolCall};
use crate::stream::TurnStreamEvent;

/// Result of dispatching tool calls, including optional loop warning.
// kanon:ignore TOPOLOGY/shallow-struct — internal dispatch result carrier used only within the execute module
pub(super) struct DispatchResult {
    /// Tool result content blocks to send back to the LLM.
    pub blocks: Vec<ContentBlock>,
    /// Loop warning message to inject into conversation, if detected.
    pub loop_warning: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ToolDispatchPolicy {
    surface: Arc<EffectiveToolSurface>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolPolicyDenial {
    Unknown,
    Allowlist { available: String },
    Group { message: String },
    Inactive,
    ServerTool,
    ParseError { message: String },
}

impl ToolPolicyDenial {
    fn message(&self, tool_name: &str) -> String {
        match self {
            Self::Unknown => {
                format!("unknown_tool: tool '{tool_name}' is not in the effective tool surface")
            }
            Self::Allowlist { available } => {
                format!(
                    "Tool '{tool_name}' is not available for this role. Available tools: {available}"
                )
            }
            Self::Group { message } | Self::ParseError { message } => message.clone(),
            Self::Inactive => {
                format!(
                    "Tool '{tool_name}' is not active for this session. Use enable_tool before calling it."
                )
            }
            Self::ServerTool => {
                format!(
                    "unknown_tool: provider server tool '{tool_name}' cannot be called as a local tool"
                )
            }
        }
    }

    const fn log_reason(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown_tool",
            Self::Allowlist { .. } => "role policy",
            Self::Group { .. } => "group policy",
            Self::Inactive => "activation policy",
            Self::ServerTool => "server tool",
            Self::ParseError { .. } => "parse error",
        }
    }
}

/// Detect a provider-normalized parse-error object produced when tool-call
/// argument JSON cannot be parsed. Nous should not execute these as real calls.
fn parse_error_denial(input: &serde_json::Value) -> Option<ToolPolicyDenial> {
    let obj = input.as_object()?;
    let message = obj
        .get("_parse_error")
        .and_then(|v| v.as_str())
        .filter(|s| s.starts_with("malformed tool input:"))?
        .to_owned();
    if !obj.contains_key("_raw_input") {
        return None;
    }
    Some(ToolPolicyDenial::ParseError { message })
}

impl ToolDispatchPolicy {
    pub(super) fn new(surface: Arc<EffectiveToolSurface>) -> Self {
        Self { surface }
    }

    #[cfg(test)]
    pub(super) fn allow_all_for_tests(registry: &ToolRegistry) -> Self {
        let active = std::collections::HashSet::new();
        let policy = organon::types::ToolGroupPolicy::AllowAll {
            reason: "execute test helper".to_owned(),
        };
        Self {
            surface: Arc::new(registry.effective_surface(organon::surface::SurfaceInputs {
                policy: &policy,
                allowlist: None,
                active: &active,
                server_tools: &[],
                server_tool_config: None,
            })),
        }
    }

    pub(super) fn tool_definitions(&self) -> Vec<ToolDefinition> {
        #[cfg(feature = "deferred-schemas")]
        let tool_defs = self.surface.provider_summaries();
        #[cfg(not(feature = "deferred-schemas"))]
        let tool_defs = self.surface.provider_tools();

        tool_defs
    }

    pub(super) fn server_tool_definitions(&self) -> Vec<hermeneus::types::ServerToolDefinition> {
        self.surface.provider_server_tools()
    }

    pub(super) fn filter_tool_uses(
        &self,
        tool_uses: Vec<(String, String, serde_json::Value)>,
        tools: &ToolRegistry,
        tool_ctx: &ToolContext,
        stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
        all_tool_calls: &mut Vec<ToolCall>,
        denied_blocks: &mut Vec<ContentBlock>,
    ) -> Vec<(String, String, serde_json::Value)> {
        let mut allowed = Vec::with_capacity(tool_uses.len());
        for (id, name, input) in tool_uses {
            let denial = self
                .denial_for(tools, &id, &name, &input)
                .or_else(|| parse_error_denial(&input));
            if let Some(denial) = denial {
                warn!(
                    tool = %name,
                    tool_use_id = %id,
                    reason = denial.log_reason(),
                    "tool call denied by dispatch policy"
                );
                record_denied_call(
                    all_tool_calls,
                    denied_blocks,
                    stream_tx,
                    tool_ctx,
                    DeniedToolCall {
                        id: &id,
                        name: &name,
                        input: &input,
                        message: denial.message(&name),
                    },
                );
            } else {
                allowed.push((id, name, input));
            }
        }
        allowed
    }

    fn denial_for(
        &self,
        tools: &ToolRegistry,
        tool_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> Option<ToolPolicyDenial> {
        let Ok(tool_name_id) = ToolName::new(tool_name) else {
            return Some(ToolPolicyDenial::Unknown);
        };

        match self.surface.lookup(&tool_name_id) {
            SurfaceLookup::Unknown => return Some(ToolPolicyDenial::Unknown),
            SurfaceLookup::Denied(entry) => {
                return Some(denial_for_availability(&entry.availability, &self.surface));
            }
            SurfaceLookup::Inactive(_) => return Some(ToolPolicyDenial::Inactive),
            SurfaceLookup::Callable(entry) if entry.kind == SurfaceEntryKind::Server => {
                return Some(ToolPolicyDenial::ServerTool);
            }
            SurfaceLookup::Callable(_) => {}
        }

        let call_input = ToolInput {
            name: tool_name_id.clone(),
            tool_use_id: tool_id.to_owned(),
            arguments: tool_input.clone(),
        };
        match tools.permits_call(&call_input, self.surface.policy()) {
            Ok(true) => {}
            Ok(false) => {
                return Some(ToolPolicyDenial::Group {
                    message: format!(
                        "Tool '{tool_name}' is not in your allowed tool groups. Policy: {}",
                        self.surface.policy().description()
                    ),
                });
            }
            Err(e) => {
                return Some(ToolPolicyDenial::Group {
                    message: format!("Tool '{tool_name}' call rejected by group policy: {e}"),
                });
            }
        }

        None
    }
}

fn denial_for_availability(
    availability: &SurfaceAvailability,
    surface: &EffectiveToolSurface,
) -> ToolPolicyDenial {
    match availability.denial_reason() {
        Some(DenialReason::Allowlist) => ToolPolicyDenial::Allowlist {
            available: surface
                .allowlist()
                .map(|values| values.join(", "))
                .unwrap_or_default(),
        },
        Some(DenialReason::GroupPolicy) => ToolPolicyDenial::Group {
            message: format!(
                "Tool is not in your allowed tool groups. Policy: {}",
                surface.policy().description()
            ),
        },
        None => ToolPolicyDenial::Group {
            message: "Tool call denied by policy".to_owned(),
        },
    }
}

fn approval_risk(approval: ApprovalRequirement) -> &'static str {
    match approval {
        ApprovalRequirement::None | ApprovalRequirement::Advisory => "low",
        ApprovalRequirement::Required => "high",
        _ => "critical",
    }
}

fn approval_reason(tool_name: &str, approval: ApprovalRequirement) -> String {
    format!("Tool '{tool_name}' requires {approval} approval because of its reversibility metadata")
}

fn record_stream_send_error<T>(
    tool_ctx: &ToolContext,
    tool_name: &str,
    kind: &'static str,
    err: &tokio::sync::mpsc::error::TrySendError<T>,
) {
    match err {
        tokio::sync::mpsc::error::TrySendError::Full(_) => {
            warn!(
                tool = tool_name,
                kind, "streaming approval event dropped: channel buffer full"
            );
            crate::metrics::record_stream_event_dropped(
                tool_ctx.nous_id.as_ref(),
                kind,
                "buffer_full",
            );
        }
        tokio::sync::mpsc::error::TrySendError::Closed(_) => {
            debug!(
                tool = tool_name,
                kind, "streaming approval event dropped: receiver disconnected"
            );
            crate::metrics::record_stream_event_dropped(
                tool_ctx.nous_id.as_ref(),
                kind,
                "disconnected",
            );
        }
    }
}

fn emit_approval_required(
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    tool_id: &str,
    tool_name: &str,
    tool_input: &serde_json::Value,
    approval: ApprovalRequirement,
) {
    let Some(stream_tx) = stream_tx else {
        return;
    };
    if let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolApprovalRequired {
        turn_id: tool_ctx.turn_number.to_string(),
        tool_id: tool_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input: tool_input.clone(),
        risk: approval_risk(approval).to_owned(),
        reason: approval_reason(tool_name, approval),
    }) {
        record_stream_send_error(tool_ctx, tool_name, "approval_required", &e);
    }
}

fn emit_approval_resolved(
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    tool_id: &str,
    tool_name: &str,
    decision: &str,
) {
    let Some(stream_tx) = stream_tx else {
        return;
    };
    if let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolApprovalResolved {
        tool_id: tool_id.to_owned(),
        decision: decision.to_owned(),
    }) {
        record_stream_send_error(tool_ctx, tool_name, "approval_resolved", &e);
    }
}

/// Record a denied tool call: append a synthetic `ToolResult` block for the
/// model, push it on `all_tool_calls` for observability, and emit a `ToolResult`
/// stream event so the frontend records the denial outcome.
struct DeniedToolCall<'a> {
    id: &'a str,
    name: &'a str,
    input: &'a serde_json::Value,
    message: String,
}

fn record_denied_call(
    all_tool_calls: &mut Vec<ToolCall>,
    tool_results: &mut Vec<ContentBlock>,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    denied: DeniedToolCall<'_>,
) {
    all_tool_calls.push(ToolCall {
        id: denied.id.to_owned(),
        name: denied.name.to_owned(),
        input: denied.input.clone(),
        result: Some(denied.message.clone()),
        is_error: true,
        duration_ms: 0,
        receipt: None,
    });
    tool_results.push(ContentBlock::ToolResult {
        tool_use_id: denied.id.to_owned(),
        content: ToolResultContent::Text(denied.message.clone()),
        is_error: Some(true),
    });
    if let Some(stream_tx) = stream_tx
        && let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolResult {
            tool_id: denied.id.to_owned(),
            tool_name: denied.name.to_owned(),
            result: denied.message,
            is_error: true,
            duration_ms: 0,
        })
    {
        record_stream_send_error(tool_ctx, denied.name, "denied_tool_result", &e);
    }
}

fn emit_tool_start(
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    tool_id: &str,
    tool_name: &str,
    tool_input: &serde_json::Value,
) {
    if let Some(stream_tx) = stream_tx
        && let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolStart {
            tool_id: tool_id.to_owned(),
            tool_name: tool_name.to_owned(),
            input: tool_input.clone(),
        })
    {
        record_stream_send_error(tool_ctx, tool_name, "tool_start", &e);
    }
}

fn emit_tool_result(
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    tool_id: &str,
    tool_name: &str,
    result: String,
    is_error: bool,
    duration_ms: u64,
) {
    if let Some(stream_tx) = stream_tx
        && let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolResult {
            tool_id: tool_id.to_owned(),
            tool_name: tool_name.to_owned(),
            result,
            is_error,
            duration_ms,
        })
    {
        record_stream_send_error(tool_ctx, tool_name, "tool_result", &e);
    }
}

fn record_tool_outcome(
    all_tool_calls: &mut Vec<ToolCall>,
    tool_results: &mut Vec<ContentBlock>,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    outcome: SingleToolOutcome,
) -> bool {
    let is_error = outcome.is_error;
    all_tool_calls.push(outcome.call);
    if let Some(call) = all_tool_calls.last()
        && let Some(result) = call.result.clone()
    {
        emit_tool_result(
            stream_tx,
            tool_ctx,
            &call.id,
            &call.name,
            result,
            call.is_error,
            call.duration_ms,
        );
    }
    tool_results.push(outcome.result_block);
    is_error
}

/// Inject a bounded diagnostic preamble into tool result content.
///
/// Diagnostics are placed at the front of the payload so they survive
/// truncation that cuts from the end.
pub(crate) fn inject_diagnostics(content: ToolResultContent, diag_text: &str) -> ToolResultContent {
    match content {
        ToolResultContent::Text(text) => ToolResultContent::Text(format!("{diag_text}\n\n{text}")),
        ToolResultContent::Blocks(mut blocks) => {
            blocks.insert(
                0,
                ToolResultBlock::Text {
                    text: diag_text.to_owned(),
                },
            );
            ToolResultContent::Blocks(blocks)
        }
        // WHY: ToolResultContent is #[non_exhaustive]; forward-compatibility arm.
        other => other,
    }
}

/// Truncate a tool result if it exceeds `max_bytes`.
///
/// Only text content is truncated; image and document blocks are left
/// intact because they are binary data that cannot be meaningfully
/// split at arbitrary byte boundaries. When truncation occurs, the
/// text is cut at the last char boundary within the limit and a
/// `[truncated: {original} -> {truncated} bytes]` indicator is appended.
///
/// A `max_bytes` of `0` disables truncation entirely.
pub(crate) fn truncate_tool_result(
    content: ToolResultContent,
    max_bytes: u32,
) -> ToolResultContent {
    if max_bytes == 0 {
        return content;
    }
    #[expect(
        clippy::as_conversions,
        reason = "u32→usize: max_bytes always fits in usize"
    )]
    let limit = max_bytes as usize; // kanon:ignore RUST/as-cast

    match content {
        ToolResultContent::Text(text) => {
            if text.len() <= limit {
                return ToolResultContent::Text(text);
            }
            let original_len = text.len();
            // WHY: truncate at a char boundary to avoid producing invalid UTF-8.
            let truncated = truncate_at_char_boundary(&text, limit);
            let indicator = format!(
                "\n[truncated: {} -> {} bytes]",
                original_len,
                truncated.len()
            );
            debug!(
                original_bytes = original_len,
                truncated_bytes = truncated.len(),
                "tool result truncated"
            );
            ToolResultContent::Text(format!("{truncated}{indicator}"))
        }
        ToolResultContent::Blocks(blocks) => {
            // WHY: estimate total serialized size across ALL block types, not just text.
            // Non-text blocks (images, documents) contribute their JSON-serialized length
            // so the truncation limit applies to the full payload.
            let total: usize = blocks
                .iter()
                .map(|b| match b {
                    ToolResultBlock::Text { text } => text.len(),
                    other => serde_json::to_string(other).map_or(0, |s| s.len()),
                })
                .sum();

            if total <= limit {
                return ToolResultContent::Blocks(blocks);
            }

            debug!(
                original_bytes = total,
                limit_bytes = limit,
                "tool result blocks truncated"
            );

            let mut remaining = limit;
            let mut out = Vec::with_capacity(blocks.len());
            for block in blocks {
                match block {
                    ToolResultBlock::Text { text } => {
                        if remaining == 0 {
                            continue;
                        }
                        if text.len() <= remaining {
                            remaining -= text.len();
                            out.push(ToolResultBlock::Text { text });
                        } else {
                            let truncated = truncate_at_char_boundary(&text, remaining);
                            remaining = 0;
                            out.push(ToolResultBlock::Text {
                                text: truncated.to_owned(),
                            });
                        }
                    }
                    other => {
                        let block_size = serde_json::to_string(&other).map_or(0, |s| s.len());
                        if block_size <= remaining {
                            remaining -= block_size;
                            out.push(other);
                        } else {
                            // WHY: non-text blocks cannot be meaningfully split, so skip
                            // when they would exceed the remaining budget.
                            remaining = 0;
                        }
                    }
                }
            }
            let indicator = format!("\n[truncated: {total} -> {limit} bytes]");
            out.push(ToolResultBlock::Text { text: indicator });
            ToolResultContent::Blocks(out)
        }
        _ => content,
    }
}

/// Find the largest prefix of `s` that is at most `max_bytes` bytes and
/// ends on a UTF-8 char boundary.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // WHY: floor_char_boundary rounds down to the nearest char boundary,
    // avoiding a panic or invalid slice from splitting mid-codepoint.
    let end = s.floor_char_boundary(max_bytes);
    s.get(..end).unwrap_or(s)
}

/// Hash a JSON value for loop detection using the standard library hasher.
pub(super) fn simple_hash(value: &serde_json::Value) -> String {
    let mut hasher = DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Classify the interaction signals based on tool calls and content.
pub(super) fn classify_signals(
    tool_calls: &[ToolCall],
    _content: &str,
    used_server_web_search: bool,
    used_server_code_execution: bool,
) -> Vec<InteractionSignal> {
    let mut signals = Vec::new();
    let used_any_server_tool = used_server_web_search || used_server_code_execution;

    if tool_calls.is_empty() && !used_any_server_tool {
        signals.push(InteractionSignal::Conversation);
    } else {
        if !tool_calls.is_empty() || used_any_server_tool {
            signals.push(InteractionSignal::ToolExecution);
        }

        let code_tools = ["write", "edit", "exec"];
        if used_server_code_execution
            || tool_calls
                .iter()
                .any(|tc| code_tools.contains(&tc.name.as_str()))
        {
            signals.push(InteractionSignal::CodeGeneration);
        }

        let research_tools = ["web_search", "web_fetch"];
        if used_server_web_search
            || tool_calls
                .iter()
                .any(|tc| research_tools.contains(&tc.name.as_str()))
        {
            signals.push(InteractionSignal::Research);
        }

        if tool_calls.iter().any(|tc| tc.is_error) {
            signals.push(InteractionSignal::ErrorRecovery);
        }
    }

    signals
}

/// Convert pipeline messages to hermeneus messages.
pub(super) fn build_messages(
    pipeline_messages: &[crate::pipeline::PipelineMessage],
) -> Vec<hermeneus::types::Message> {
    use hermeneus::types::{Content, Message, Role};

    pipeline_messages
        .iter()
        .map(|m| Message {
            // WHY: unknown role strings default to User to preserve forward
            // compatibility with pipeline sources that may add new roles.
            role: match m.role.as_str() {
                "assistant" => Role::Assistant,
                "system" => Role::System,
                _ => Role::User,
            },
            content: Content::Text(m.content.clone()),
            cache_breakpoint: m.cache_breakpoint,
        })
        .collect()
}

/// Outcome of executing a single tool call: the persisted [`ToolCall`]
/// record, the LLM-facing [`ContentBlock::ToolResult`] block, and the
/// `is_error` flag the outer loop feeds into the loop detector.
struct SingleToolOutcome {
    call: ToolCall,
    result_block: ContentBlock,
    is_error: bool,
}

/// Execute one prepared tool call: invoke the executor, truncate + log + build
/// the (`ToolCall`, `ContentBlock::ToolResult`) pair. Loop-detection
/// bookkeeping is handled by the caller.
#[expect(
    clippy::too_many_arguments,
    reason = "dispatch needs tool id, name, input, registry, context, limits, and receipt infra"
)]
async fn dispatch_single_tool(
    tool_id: &str,
    tool_name: &str,
    execution_input: &ToolInput,
    persisted_input: &serde_json::Value,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    max_tool_result_bytes: u32,
    receipt_signer: Option<&organon::receipts::ReceiptSigner>,
    receipt_ledger: Option<&std::sync::Mutex<organon::receipts::ReceiptLedger>>,
) -> error::Result<SingleToolOutcome> {
    let start = std::time::Instant::now();
    let result = tools.execute(execution_input, tool_ctx).await;

    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "u128→u64: tool execution duration won't exceed u64::MAX milliseconds"
    )]
    let duration_ms = start.elapsed().as_millis() as u64; // kanon:ignore RUST/as-cast

    let (content, is_error) = match result {
        Ok(mut r) => {
            if let Some(ref mut d) = r.diagnostics {
                d.duration_ms = duration_ms;
                let diag_text = d.to_llm_text();
                r.content = inject_diagnostics(r.content, &diag_text);
            }
            (r.content, r.is_error)
        }
        Err(e) => (ToolResultContent::text(format!("Tool error: {e}")), true),
    };

    let content = truncate_tool_result(content, max_tool_result_bytes);

    // WHY: tool failures must be visible at production log levels so operators
    // can detect systematic tool problems (DNS, permissions, etc.) without
    // enabling debug-level tracing. (#3284)
    if is_error {
        warn!(
            tool = tool_name,
            tool_id = tool_id,
            duration_ms,
            "tool execution failed"
        );
        crate::metrics::record_tool_failure(tool_ctx.nous_id.as_ref(), tool_name);
    } else {
        debug!(tool = tool_name, duration_ms, "tool executed");
    }

    let (content, receipt) = if let Some(signer) = receipt_signer {
        let ts = jiff::Timestamp::now();
        let result_text = content.text_summary();
        let receipt_str = signer.sign(tool_name, &persisted_input.to_string(), &result_text, ts);
        if let Some(ledger) = receipt_ledger {
            let mut guard = ledger.lock().unwrap_or_else(|poisoned| {
                tracing::warn!("receipt_ledger lock poisoned, recovering with last value");
                poisoned.into_inner()
            });
            guard.record(
                receipt_str.clone(),
                tool_name.to_owned(),
                persisted_input.to_string(),
                result_text.clone(),
                ts,
            );
        }
        let tagged = match content {
            ToolResultContent::Text(text) => {
                ToolResultContent::Text(format!("{text}\n\n[receipt:{receipt_str}]"))
            }
            ToolResultContent::Blocks(mut blocks) => {
                blocks.push(ToolResultBlock::Text {
                    text: format!("\n\n[receipt:{receipt_str}]"),
                });
                ToolResultContent::Blocks(blocks)
            }
            // WHY: ToolResultContent is #[non_exhaustive]; forward-compatibility arm.
            other => other,
        };
        (tagged, Some(receipt_str))
    } else {
        (content, None)
    };

    let call = ToolCall {
        id: tool_id.to_owned(),
        name: tool_name.to_owned(),
        input: persisted_input.clone(),
        result: Some(content.text_summary()),
        is_error,
        duration_ms,
        receipt,
    };

    let result_block = ContentBlock::ToolResult {
        tool_use_id: tool_id.to_owned(),
        content,
        is_error: Some(is_error),
    };

    Ok(SingleToolOutcome {
        call,
        result_block,
        is_error,
    })
}

/// Dispatch tool calls from an LLM response and collect results.
///
/// Records each tool call in the loop detector AFTER execution (so error
/// status is known). On [`LoopVerdict::Warn`], stops processing remaining
/// tools and returns the warning. On [`LoopVerdict::Halt`], returns an error.
///
/// Per-tool work lives in [`dispatch_single_tool`]; this function owns the
/// iteration over `tool_uses` and the loop-detection branch that can halt
/// or warn before subsequent tools run.
#[expect(
    clippy::too_many_arguments,
    reason = "dispatch needs tool uses, registry, context, detector, calls, iterations, limits, and receipt infra"
)]
#[expect(
    clippy::too_many_lines,
    reason = "single approval-aware dispatch loop owns the full per-tool lifecycle"
)]
pub(super) async fn dispatch_tools(
    tool_uses: &[(String, String, serde_json::Value)],
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    loop_detector: &mut LoopDetector,
    all_tool_calls: &mut Vec<ToolCall>,
    iterations: u32,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    approval_gate: Option<&ApprovalGate>,
    policy: &ToolDispatchPolicy,
    max_tool_result_bytes: u32,
    receipt_signer: Option<&organon::receipts::ReceiptSigner>,
    receipt_ledger: Option<&std::sync::Mutex<organon::receipts::ReceiptLedger>>,
) -> error::Result<DispatchResult> {
    let mut tool_results: Vec<ContentBlock> = Vec::new();

    for (tool_id, tool_name, tool_input) in tool_uses {
        if let Some(denial) = policy.denial_for(tools, tool_id, tool_name, tool_input) {
            warn!(
                tool = %tool_name,
                tool_id = %tool_id,
                reason = denial.log_reason(),
                "tool call denied by dispatch policy"
            );
            record_denied_call(
                all_tool_calls,
                &mut tool_results,
                stream_tx,
                tool_ctx,
                DeniedToolCall {
                    id: tool_id,
                    name: tool_name,
                    input: tool_input,
                    message: denial.message(tool_name),
                },
            );
            continue;
        }

        let tool_name_id = ToolName::new(tool_name.as_str()).map_err(|_err| {
            error::PipelineStageSnafu {
                stage: "execute",
                message: format!("invalid tool name: {tool_name}"),
            }
            .build()
        })?;

        // WHY(#3569): substitute secrets at the LAST moment before tool
        // execution. The original `tool_input` (with placeholders) is preserved
        // for persistence in `all_tool_calls`; only the executor sees resolved
        // values.
        let mut substituted_args = tool_input.clone();
        if let Some(services) = &tool_ctx.services
            && let Err(e) = substitute_in_json(&mut substituted_args, &services.secret_vault)
        {
            let msg = format!("Tool error: {e}");
            crate::metrics::record_tool_failure(tool_ctx.nous_id.as_ref(), tool_name);
            let outcome = SingleToolOutcome {
                call: ToolCall {
                    id: tool_id.clone(),
                    name: tool_name.clone(),
                    input: tool_input.clone(),
                    result: Some(msg.clone()),
                    is_error: true,
                    duration_ms: 0,
                    receipt: None,
                },
                result_block: ContentBlock::ToolResult {
                    tool_use_id: tool_id.clone(),
                    content: ToolResultContent::text(msg),
                    is_error: Some(true),
                },
                is_error: true,
            };
            let is_error = record_tool_outcome(
                all_tool_calls,
                &mut tool_results,
                stream_tx,
                tool_ctx,
                outcome,
            );
            let input_hash = simple_hash(tool_input);
            match loop_detector.record(tool_name, &input_hash, is_error) {
                LoopVerdict::Ok => {}
                LoopVerdict::Warn { message, .. } => {
                    return Ok(DispatchResult {
                        blocks: tool_results,
                        loop_warning: Some(message),
                    });
                }
                LoopVerdict::Halt { pattern, .. } => {
                    return Err(error::LoopDetectedSnafu {
                        iterations,
                        pattern,
                    }
                    .build());
                }
            }
            continue;
        }

        let approval_input = ToolInput {
            name: tool_name_id,
            tool_use_id: tool_id.clone(),
            arguments: substituted_args,
        };
        let approval = match tools.approval_requirement_for_input(&approval_input) {
            Ok(approval) => approval,
            Err(e) => {
                record_denied_call(
                    all_tool_calls,
                    &mut tool_results,
                    stream_tx,
                    tool_ctx,
                    DeniedToolCall {
                        id: tool_id,
                        name: tool_name,
                        input: tool_input,
                        message: format!("tool_policy: Tool '{tool_name}' call rejected: {e}"),
                    },
                );
                continue;
            }
        };

        // WHY(#3958, ADR-005): one decision boundary protects streaming,
        // fallback, and batch dispatch. Unknown future requirements block.
        match approval {
            ApprovalRequirement::None => {
                emit_approval_resolved(stream_tx, tool_ctx, tool_id, tool_name, "auto_approved");
            }
            ApprovalRequirement::Advisory => {
                emit_approval_resolved(stream_tx, tool_ctx, tool_id, tool_name, "advisory_auto");
            }
            ApprovalRequirement::Required | ApprovalRequirement::Mandatory | _ => {
                emit_approval_required(
                    stream_tx, tool_ctx, tool_id, tool_name, tool_input, approval,
                );
                let choice = match approval_gate {
                    Some(gate) => gate.await_decision(tool_id).await,
                    None => match approval {
                        ApprovalRequirement::Mandatory => {
                            warn!(
                                tool = tool_name.as_str(),
                                tool_id = tool_id.as_str(),
                                "mandatory tool call with no approval gate wired - default-deny"
                            );
                            ApprovalChoice::Denied
                        }
                        _ => ApprovalChoice::Approved,
                    },
                };
                emit_approval_resolved(
                    stream_tx,
                    tool_ctx,
                    tool_id,
                    tool_name,
                    choice.as_wire_str(),
                );
                if matches!(choice, ApprovalChoice::Denied) {
                    record_denied_call(
                        all_tool_calls,
                        &mut tool_results,
                        stream_tx,
                        tool_ctx,
                        DeniedToolCall {
                            id: tool_id,
                            name: tool_name,
                            input: tool_input,
                            message: format!("Tool '{tool_name}' execution denied by user."),
                        },
                    );
                    continue;
                }
            }
        }

        emit_tool_start(stream_tx, tool_ctx, tool_id, tool_name, tool_input);

        let outcome = dispatch_single_tool(
            tool_id,
            tool_name,
            &approval_input,
            tool_input,
            tools,
            tool_ctx,
            max_tool_result_bytes,
            receipt_signer,
            receipt_ledger,
        )
        .await?;

        let is_error = record_tool_outcome(
            all_tool_calls,
            &mut tool_results,
            stream_tx,
            tool_ctx,
            outcome,
        );

        let input_hash = simple_hash(tool_input);
        match loop_detector.record(tool_name, &input_hash, is_error) {
            LoopVerdict::Ok => {}
            LoopVerdict::Warn { message, .. } => {
                return Ok(DispatchResult {
                    blocks: tool_results,
                    loop_warning: Some(message),
                });
            }
            LoopVerdict::Halt { pattern, .. } => {
                return Err(error::LoopDetectedSnafu {
                    iterations,
                    pattern,
                }
                .build());
            }
        }
    }

    Ok(DispatchResult {
        blocks: tool_results,
        loop_warning: None,
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn text_within_limit_passes_through() {
        let content = ToolResultContent::text("hello world");
        let result = truncate_tool_result(content, 100);
        match result {
            ToolResultContent::Text(s) => assert_eq!(s, "hello world"),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn text_at_exact_limit_passes_through() {
        let text = "a".repeat(50);
        let content = ToolResultContent::text(text.clone());
        let result = truncate_tool_result(content, 50);
        match result {
            ToolResultContent::Text(s) => assert_eq!(s, text),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn text_over_limit_is_truncated_with_indicator() {
        let text = "a".repeat(100);
        let result = truncate_tool_result(ToolResultContent::text(text), 50);
        match result {
            ToolResultContent::Text(s) => {
                assert!(
                    s.contains("[truncated: 100 -> 50 bytes]"),
                    "missing truncation indicator in: {s}"
                );
                assert!(
                    s.starts_with("aaaa"),
                    "truncated content should preserve prefix"
                );
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn zero_limit_disables_truncation() {
        let text = "a".repeat(100_000);
        let content = ToolResultContent::text(text.clone());
        let result = truncate_tool_result(content, 0);
        match result {
            ToolResultContent::Text(s) => assert_eq!(s.len(), 100_000),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn multibyte_chars_truncated_at_char_boundary() {
        let text = "\u{1F600}\u{1F601}\u{1F602}";
        assert_eq!(text.len(), 12, "test setup: 3 emojis = 12 bytes");

        let result = truncate_tool_result(ToolResultContent::text(text), 5);
        match result {
            ToolResultContent::Text(s) => {
                assert!(
                    s.starts_with('\u{1F600}'),
                    "should keep first complete emoji"
                );
                assert!(
                    s.contains("[truncated: 12 -> 4 bytes]"),
                    "indicator should show char-boundary size: {s}"
                );
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn blocks_within_limit_pass_through() {
        let blocks = vec![
            ToolResultBlock::Text {
                text: "hello".to_owned(),
            },
            ToolResultBlock::Text {
                text: "world".to_owned(),
            },
        ];
        let content = ToolResultContent::Blocks(blocks);
        let result = truncate_tool_result(content, 100);
        match result {
            ToolResultContent::Blocks(bs) => {
                assert_eq!(bs.len(), 2, "both blocks should pass through");
            }
            _ => panic!("expected Blocks variant"),
        }
    }

    #[test]
    fn blocks_over_limit_truncates_text_and_accounts_for_non_text_size() {
        let image_block = ToolResultBlock::Image {
            source: hermeneus::types::ImageSource {
                source_type: "base64".to_owned(),
                media_type: "image/png".to_owned(),
                data: "base64data".to_owned(),
            },
        };
        let image_size = serde_json::to_string(&image_block)
            .expect("serialize")
            .len();

        let blocks = vec![
            ToolResultBlock::Text {
                text: "a".repeat(80),
            },
            image_block,
            ToolResultBlock::Text {
                text: "b".repeat(40),
            },
        ];
        let total_size = 80 + image_size + 40;

        // WHY: limit high enough to fit text but the image block pushes total over
        let limit = 80 + image_size + 10;
        let content = ToolResultContent::Blocks(blocks);
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "usize→u32: test value fits"
        )]
        let result = truncate_tool_result(content, limit as u32); // kanon:ignore RUST/as-cast
        match result {
            ToolResultContent::Blocks(bs) => {
                let has_image = bs
                    .iter()
                    .any(|b| matches!(b, ToolResultBlock::Image { .. }));
                assert!(
                    has_image,
                    "image block should be preserved when within budget"
                );

                let indicator_block = bs.last().expect("should have indicator block");
                match indicator_block {
                    ToolResultBlock::Text { text } => {
                        let expected = format!("[truncated: {total_size} -> {limit} bytes]");
                        assert!(
                            text.contains(&expected),
                            "indicator should show total including non-text sizes: {text}"
                        );
                    }
                    _ => panic!("last block should be text indicator"),
                }
            }
            _ => panic!("expected Blocks variant"),
        }
    }

    #[test]
    fn blocks_over_limit_skips_non_text_blocks_exceeding_budget() {
        let image_block = ToolResultBlock::Image {
            source: hermeneus::types::ImageSource {
                source_type: "base64".to_owned(),
                media_type: "image/png".to_owned(),
                data: "base64data".to_owned(),
            },
        };
        let blocks = vec![
            ToolResultBlock::Text {
                text: "a".repeat(30),
            },
            image_block,
        ];
        // WHY: limit too small for the image block's serialized size
        let content = ToolResultContent::Blocks(blocks);
        let result = truncate_tool_result(content, 40);
        match result {
            ToolResultContent::Blocks(bs) => {
                let has_image = bs
                    .iter()
                    .any(|b| matches!(b, ToolResultBlock::Image { .. }));
                assert!(!has_image, "image block should be skipped when over budget");
            }
            _ => panic!("expected Blocks variant"),
        }
    }

    // ── inject_diagnostics tests ───────────────────────────────────────

    #[test]
    fn inject_diagnostics_into_text_prepends_diag() {
        let content = ToolResultContent::text("tool output");
        let result = inject_diagnostics(content, "[diagnostics: exit_code=1]");
        match result {
            ToolResultContent::Text(s) => {
                assert!(
                    s.starts_with("[diagnostics: exit_code=1]"),
                    "diagnostics should be prepended: {s}"
                );
                assert!(
                    s.contains("tool output"),
                    "original content should remain: {s}"
                );
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn inject_diagnostics_into_blocks_inserts_first_block() {
        let blocks = vec![ToolResultBlock::Text {
            text: "block 1".to_owned(),
        }];
        let content = ToolResultContent::Blocks(blocks);
        let result = inject_diagnostics(content, "[diagnostics: exit_code=2]");
        match result {
            ToolResultContent::Blocks(bs) => {
                assert_eq!(bs.len(), 2, "should have two blocks");
                match bs.first().expect("should have first block") {
                    ToolResultBlock::Text { text } => {
                        assert_eq!(text, "[diagnostics: exit_code=2]");
                    }
                    _ => panic!("first block should be diagnostic text"),
                }
            }
            _ => panic!("expected Blocks variant"),
        }
    }

    #[test]
    fn diagnostics_survive_text_truncation() {
        let content = ToolResultContent::text("a".repeat(200));
        let with_diag = inject_diagnostics(content, "[diagnostics: exit_code=127]");
        let truncated = truncate_tool_result(with_diag, 50);
        match truncated {
            ToolResultContent::Text(s) => {
                assert!(
                    s.starts_with("[diagnostics: exit_code=127]"),
                    "diagnostics should survive truncation: {s}"
                );
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn diagnostics_survive_block_truncation() {
        let blocks = vec![
            ToolResultBlock::Text {
                text: "a".repeat(100),
            },
            ToolResultBlock::Text {
                text: "b".repeat(100),
            },
        ];
        let content = ToolResultContent::Blocks(blocks);
        let with_diag = inject_diagnostics(content, "[diagnostics: exit_code=1]");
        let truncated = truncate_tool_result(with_diag, 80);
        match truncated {
            ToolResultContent::Blocks(bs) => match bs.first().expect("should have first block") {
                ToolResultBlock::Text { text } => {
                    assert_eq!(text, "[diagnostics: exit_code=1]");
                }
                _ => panic!("first block should be diagnostics"),
            },
            _ => panic!("expected Blocks variant"),
        }
    }
}
