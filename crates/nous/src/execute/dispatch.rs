// kanon:ignore RUST/file-too-long — provider dispatch loop; extraction into submodules tracked in #3752
//! Dispatch helpers: tool execution, signal classification, message conversion.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use tracing::{debug, info, warn};

use tokio::sync::mpsc;

use hermeneus::secret::{
    redact_in_json, redact_resolved_secrets_in_prepared_json, substitute_in_json,
};
use hermeneus::types::{ContentBlock, ToolDefinition, ToolResultBlock, ToolResultContent};
use koina::id::ToolName;
#[cfg(test)]
use koina::ulid::Ulid;
use organon::registry::{PreparedToolInput, ToolRegistry};
use organon::surface::{
    DenialReason, EffectiveToolSurface, SurfaceAvailability, SurfaceEntryKind, SurfaceLookup,
};
use organon::types::{
    ApprovalRequirement, InputSchema, PropertyDef, PropertyType, RedactionPolicy, ToolContext,
    ToolInput, ToolResult,
};

use crate::approval::{ApprovalChoice, ApprovalGate};
use crate::error;
use crate::pipeline::{InteractionSignal, LoopDetector, LoopVerdict, ToolCall};
use crate::stream::{LiveApprovalEvidence, TurnEventIdentity, TurnStreamEvent};

/// Result of dispatching tool calls, including optional loop warning.
// kanon:ignore TOPOLOGY/shallow-struct — internal dispatch result carrier used only within the execute module
pub(super) struct DispatchResult {
    /// Tool result content blocks to send back to the LLM.
    pub blocks: Vec<ContentBlock>,
    /// Loop warning message to inject into conversation, if detected.
    pub loop_warning: Option<String>,
    /// `tool_use` ids recorded in `all_tool_calls` without the tool ever running.
    ///
    /// INVARIANT: every call recorded by `record_denied_call` appears here, and only
    /// those. That function is the single point at which a tool call enters
    /// `all_tool_calls` without being executed — classification denials, approval-gate
    /// denials, the no-gate `Mandatory` fallback, the policy re-check, and the calls a
    /// loop warning abandons all route through it — so membership here is the
    /// authoritative answer to "did this tool run?".
    ///
    /// WHY: `ToolCall::approval` cannot answer that question. It is an outcome label,
    /// not an execution record, and its vocabulary is overloaded — `TOOL_OUTCOME_FAILED`
    /// tags a parse error that never dispatched, and an executed call carries an
    /// approval outcome rather than a denial one. Deriving execution from that string
    /// would put a second, drifting owner on a fact this list already owns.
    pub unexecuted: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) enum ToolDispatchItem {
    Ready {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Denied {
        id: String,
        name: String,
        input: serde_json::Value,
        message: String,
        outcome: &'static str,
    },
}

impl ToolDispatchItem {
    pub(super) fn ready(id: String, name: String, input: serde_json::Value) -> Self {
        Self::Ready { id, name, input }
    }

    pub(super) fn denied_by_hook(
        id: String,
        name: String,
        input: serde_json::Value,
        message: String,
    ) -> Self {
        Self::Denied {
            id,
            name,
            input,
            message,
            outcome: TOOL_OUTCOME_DENIED_BY_HOOK,
        }
    }

    pub(super) fn ready_input_for(&self, tool_id: &str) -> Option<&serde_json::Value> {
        match self {
            Self::Ready { id, input, .. } if id == tool_id => Some(input),
            Self::Ready { .. } | Self::Denied { .. } => None,
        }
    }
}

impl From<(String, String, serde_json::Value)> for ToolDispatchItem {
    fn from((id, name, input): (String, String, serde_json::Value)) -> Self {
        Self::ready(id, name, input)
    }
}

#[derive(Debug, Clone)]
pub(super) struct ToolDispatchPolicy {
    surface: Arc<EffectiveToolSurface>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolPolicyDenial {
    Unknown,
    NameCollision,
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
            Self::NameCollision => {
                format!(
                    "Tool '{tool_name}' is ambiguous across multiple tool planes. Configure unique tool names before calling it."
                )
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
            Self::NameCollision => "name collision",
            Self::Allowlist { .. } => "role policy",
            Self::Group { .. } => "group policy",
            Self::Inactive => "activation policy",
            Self::ServerTool => "server tool",
            Self::ParseError { .. } => "parse error",
        }
    }

    const fn outcome(&self) -> &'static str {
        match self {
            Self::Unknown | Self::ServerTool => TOOL_OUTCOME_NOT_FOUND,
            Self::NameCollision | Self::Allowlist { .. } => TOOL_OUTCOME_DENIED_BY_ROLE,
            Self::Group { .. } => TOOL_OUTCOME_DENIED_BY_GROUP,
            Self::Inactive => TOOL_OUTCOME_DENIED_INACTIVE,
            Self::ParseError { .. } => TOOL_OUTCOME_FAILED,
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
    ) -> Vec<ToolDispatchItem> {
        let mut items = Vec::with_capacity(tool_uses.len());
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
                items.push(ToolDispatchItem::Denied {
                    id,
                    name: name.clone(),
                    input,
                    message: denial.message(&name),
                    outcome: denial.outcome(),
                });
            } else {
                items.push(ToolDispatchItem::Ready { id, name, input });
            }
        }
        items
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
            SurfaceLookup::Ambiguous { .. } => return Some(ToolPolicyDenial::NameCollision),
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
        Some(DenialReason::NameCollision) => ToolPolicyDenial::NameCollision,
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

/// Resolve the declared redaction policy for a tool by its (possibly
/// unvalidated) name. Unknown or undeclared tools read as
/// `RedactionPolicy::None` -- the honest "no per-tool policy" default, never
/// a fabricated redaction.
fn redaction_policy_for(tools: &ToolRegistry, tool_name: &str) -> RedactionPolicy {
    ToolName::new(tool_name)
        .ok()
        .map(|name| tools.capability_metadata(&name).redaction)
        .unwrap_or_default()
}

/// Apply generic and declared redaction policies (#6808) to an input copy for
/// one outward-facing surface.
///
/// Callers choose the source deliberately: durable/replay surfaces pass the
/// placeholder-form model input, while the connected live approver passes the
/// prepared input it is actually authorizing. The two are never reconstructed
/// from one another.
fn redacted_surface_input(
    tools: &ToolRegistry,
    tool_name: &str,
    input: &serde_json::Value,
) -> serde_json::Value {
    let policy = redaction_policy_for(tools, tool_name);
    let mut redacted = input.clone();
    // SECURITY(#6808): declared policy augments the generic content
    // heuristic; it never replaces it. This also handles secret-shaped
    // dynamic object keys before a trace copy reaches any consumer.
    redact_in_json(&mut redacted);
    let missed = policy.apply_to_input(&mut redacted);
    if !missed.is_empty() {
        // WHY debug, not warn: an absent declared field is normally a
        // legitimately-omitted optional argument. The loud failure for a
        // misspelled declaration lives at declaration time --
        // `organon::builtins::capability_governance_tests` fails CI on any
        // `Fields` name absent from the tool's input schema.
        debug!(
            tool = tool_name,
            missed_count = missed.len(),
            "declared redaction field(s) absent from call payload"
        );
    }
    redacted
}

/// Build the connected approver's minimum useful view of executor-bound input.
///
/// Generic durable redaction deliberately treats every long whitespace-free
/// string as credential-shaped. That is safe for replay, but it would erase
/// ordinary approval identities such as canonical paths, URLs, and opaque IDs.
/// Temporarily mask schema-declared string positions from that heuristic, then
/// restore only positions that survived dynamic-key redaction and were not
/// replaced by the declared `Full`/`Fields` policy. Restoration runs the
/// strong-pattern sanitizer, so API keys, JWTs, bearer tokens, and password
/// assignments remain hidden without applying the lossy length-only rule.
/// Vault-origin values have already been replaced before this function, so
/// restoring them restores only the redaction marker. Unknown/dynamic keys and
/// their values still pass through the generic fail-closed walk.
fn redacted_live_approval_input(
    tools: &ToolRegistry,
    tool_name: &ToolName,
    input: &serde_json::Value,
) -> serde_json::Value {
    let Some(def) = tools.get_def(tool_name) else {
        return redacted_surface_input(tools, tool_name.as_str(), input);
    };
    let original = input.clone();
    let mut masked = input.clone();
    mask_declared_string_positions(&mut masked, &def.input_schema);
    let mut redacted = redacted_surface_input(tools, tool_name.as_str(), &masked);
    restore_declared_string_positions(&mut redacted, &original, &def.input_schema);
    redacted
}

fn mask_declared_string_positions(value: &mut serde_json::Value, schema: &InputSchema) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (field, property) in &schema.properties {
        if let Some(value) = object.get_mut(field) {
            mask_declared_property_strings(value, property);
        }
    }
}

fn mask_declared_property_strings(value: &mut serde_json::Value, property: &PropertyDef) {
    match property.property_type {
        PropertyType::String => *value = serde_json::Value::Null,
        PropertyType::Array => {
            if let (Some(values), Some(item)) = (value.as_array_mut(), property.items.as_deref()) {
                for value in values {
                    mask_declared_property_strings(value, item);
                }
            }
        }
        PropertyType::Object => {
            if let (Some(object), Some(properties)) =
                (value.as_object_mut(), property.properties.as_ref())
            {
                for (field, property) in properties {
                    if let Some(value) = object.get_mut(field) {
                        mask_declared_property_strings(value, property);
                    }
                }
            }
        }
        _ => {}
    }
}

fn restore_declared_string_positions(
    value: &mut serde_json::Value,
    original: &serde_json::Value,
    schema: &InputSchema,
) {
    let (Some(object), Some(original)) = (value.as_object_mut(), original.as_object()) else {
        return;
    };
    for (field, property) in &schema.properties {
        if let (Some(value), Some(original)) = (object.get_mut(field), original.get(field)) {
            restore_declared_property_strings(value, original, property);
        }
    }
}

fn restore_declared_property_strings(
    value: &mut serde_json::Value,
    original: &serde_json::Value,
    property: &PropertyDef,
) {
    match property.property_type {
        PropertyType::String if value.is_null() && original.is_string() => {
            if let Some(original) = original.as_str() {
                *value = serde_json::Value::String(koina::redact::redact_sensitive(original));
            }
        }
        PropertyType::Array => {
            if let (Some(values), Some(originals), Some(item)) = (
                value.as_array_mut(),
                original.as_array(),
                property.items.as_deref(),
            ) {
                for (value, original) in values.iter_mut().zip(originals) {
                    restore_declared_property_strings(value, original, item);
                }
            }
        }
        PropertyType::Object => {
            if let (Some(object), Some(original), Some(properties)) = (
                value.as_object_mut(),
                original.as_object(),
                property.properties.as_ref(),
            ) {
                for (field, property) in properties {
                    if let (Some(value), Some(original)) =
                        (object.get_mut(field), original.get(field))
                    {
                        restore_declared_property_strings(value, original, property);
                    }
                }
            }
        }
        _ => {}
    }
}

fn redacted_trace_result(policy: &RedactionPolicy, result: &str) -> String {
    let mut redacted = koina::redact::redact_sensitive(result);
    policy.apply_to_result(&mut redacted);
    redacted
}

const APPROVAL_OUTCOME_AUTO_APPROVED: &str = "auto_approved";
const APPROVAL_OUTCOME_ADVISORY_AUTO: &str = "advisory_auto";
const APPROVAL_OUTCOME_NO_GATE_DENIED: &str = "no_gate_denied";
const APPROVAL_OUTCOME_EVENT_UNAVAILABLE_DENIED: &str = "approval_event_unavailable_denied";
const TOOL_OUTCOME_DENIED_BY_ROLE: &str = "denied_by_role";
const TOOL_OUTCOME_DENIED_BY_GROUP: &str = "denied_by_group";
pub(super) const TOOL_OUTCOME_DENIED_BY_HOOK: &str = "denied_by_hook";
const TOOL_OUTCOME_DENIED_INACTIVE: &str = "denied_inactive";
const TOOL_OUTCOME_NOT_FOUND: &str = "not_found";
const TOOL_OUTCOME_FAILED: &str = "failed";
const TOOL_OUTCOME_UNDISPATCHED: &str = "undispatched_loop_warning";

/// Whether `outcome` is one of this module's own not-executed classification
/// strings (denied by policy/hook/approval-gate, or skipped as undispatched).
///
/// WHY this exists (#4558): a caller classifying `ToolCall.outcome` cannot
/// assume "approval is `Some`" alone means "this call never ran" —
/// `ToolCall.approval` carries no type-level guarantee it was ever set from
/// this module's not-executed vocabulary rather than some other approval
/// note. Every value listed here is the exact literal `record_denied_call`
/// receives as `DeniedToolCall::approval` (the only place this module ever
/// sets `ToolCall.approval` to `Some`), so this list must be extended
/// alongside any new denial class introduced at one of its call sites.
pub(crate) fn is_denial_outcome(outcome: &str) -> bool {
    matches!(
        outcome,
        TOOL_OUTCOME_DENIED_BY_ROLE
            | TOOL_OUTCOME_DENIED_BY_GROUP
            | TOOL_OUTCOME_DENIED_BY_HOOK
            | TOOL_OUTCOME_DENIED_INACTIVE
            | TOOL_OUTCOME_NOT_FOUND
            | TOOL_OUTCOME_FAILED
            | TOOL_OUTCOME_UNDISPATCHED
            | APPROVAL_OUTCOME_NO_GATE_DENIED
            | APPROVAL_OUTCOME_EVENT_UNAVAILABLE_DENIED
    )
}

/// Close out tool calls that a loop warning stopped us from dispatching.
///
/// INVARIANT: the assistant message carrying this turn's `tool_use` blocks is pushed
/// before dispatch begins, so every one of those blocks needs a `tool_result` with a
/// matching `tool_use_id` in the following user message. A `LoopVerdict::Warn` ends
/// dispatch early, so without this the trailing calls would carry no result at all and
/// the next request would be rejected for unpaired blocks.
#[expect(
    clippy::too_many_arguments,
    reason = "forwards record_denied_call's own parameter list plus the batch of remaining items"
)]
fn record_undispatched_calls(
    all_tool_calls: &mut Vec<ToolCall>,
    unexecuted: &mut Vec<String>,
    tool_results: &mut Vec<ContentBlock>,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    tools: &ToolRegistry,
    identity: &TurnEventIdentity,
    remaining: &[ToolDispatchItem],
) {
    for item in remaining {
        let (id, name, input) = match item {
            ToolDispatchItem::Ready { id, name, input }
            | ToolDispatchItem::Denied {
                id, name, input, ..
            } => (id, name, input),
        };
        record_denied_call(
            all_tool_calls,
            unexecuted,
            tool_results,
            stream_tx,
            tool_ctx,
            tools,
            identity,
            &DeniedToolCall {
                id,
                name,
                input,
                message: format!(
                    "tool_loop: Tool '{name}' was not run because a repetition loop was detected \
                     earlier in this turn"
                ),
                approval: Some(TOOL_OUTCOME_UNDISPATCHED),
            },
        );
    }
}

fn record_approval_policy_outcome(
    tool_id: &str,
    tool_name: &str,
    approval: ApprovalRequirement,
    gate_wired: bool,
    outcome: &str,
) {
    info!(
        tool = tool_name,
        tool_id = tool_id,
        approval_requirement = %approval,
        approval_gate_wired = gate_wired,
        approval_policy_outcome = outcome,
        "tool approval policy outcome"
    );
    organon::metrics::record_approval_decision(tool_name, outcome);
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

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors TurnStreamEvent::ToolApprovalRequired's own field list"
)]
fn emit_approval_required(
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    identity: &TurnEventIdentity,
    tool_id: &str,
    tool_name: &str,
    live_input: LiveApprovalEvidence,
    replay_input: &serde_json::Value,
    approval: ApprovalRequirement,
) -> bool {
    let Some(stream_tx) = stream_tx else {
        return true;
    };
    // WHY(#5016): the event carries the turn's canonical identity — the
    // ULID minted on SessionState (which the gateway supplies for HTTP
    // turns) — never the session-local turn number.
    if let Err(error) = stream_tx.try_send(TurnStreamEvent::ToolApprovalRequired {
        identity: identity.clone(),
        tool_id: tool_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input: live_input,
        replay_input: replay_input.clone(),
        risk: approval_risk(approval).to_owned(),
        reason: approval_reason(tool_name, approval),
    }) {
        // SECURITY(#6808): do not wait for capacity here. A saturated but
        // connected transport must not pin the actor before the approval
        // gate's own timeout begins. If the live evidence cannot be
        // delivered immediately, execution defaults to deny.
        record_stream_send_error(tool_ctx, tool_name, "approval_required", &error);
        return false;
    }
    true
}

fn emit_approval_resolved(
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    identity: &TurnEventIdentity,
    tool_id: &str,
    tool_name: &str,
    decision: &str,
) {
    let Some(stream_tx) = stream_tx else {
        return;
    };
    if let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolApprovalResolved {
        identity: identity.clone(),
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
    approval: Option<&'a str>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "carries the same stream/context/identity parameters every emit_* sibling in this module takes"
)]
fn record_denied_call(
    all_tool_calls: &mut Vec<ToolCall>,
    unexecuted: &mut Vec<String>,
    tool_results: &mut Vec<ContentBlock>,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    tools: &ToolRegistry,
    identity: &TurnEventIdentity,
    denied: &DeniedToolCall<'_>,
) {
    unexecuted.push(denied.id.to_owned());
    organon::metrics::record_policy_denial(denied.name, denied.approval.unwrap_or("unknown"));
    // WHY the denied input is policy-redacted too: a denial record lands in
    // the same persisted trace as an executed call, and a denied call's
    // arguments can carry the same sensitive values (e.g. a header the
    // model inlined on a call the role policy then rejected).
    let trace_input = redacted_surface_input(tools, denied.name, denied.input);
    let redaction = redaction_policy_for(tools, denied.name);
    let recorded_message = redacted_trace_result(&redaction, &denied.message);
    all_tool_calls.push(ToolCall {
        id: denied.id.to_owned(),
        name: denied.name.to_owned(),
        input: trace_input,
        result: Some(recorded_message.clone()),
        is_error: true,
        duration_ms: 0,
        approval: denied.approval.map(str::to_owned),
        receipt: None,
        outcome_detail: None,
    });
    // WHY read back from the just-pushed record rather than re-deriving:
    // `outcome_label()` is the one place that classification logic lives
    // (#4558) — a denial always resolves to `denied.approval` verbatim
    // here since that string IS one of `is_denial_outcome`'s known classes,
    // but reading it back keeps this call site from silently drifting from
    // that function if the classification ever changes.
    let outcome_label = all_tool_calls
        .last()
        .map_or("error", |call| call.outcome_label())
        .to_owned();
    tool_results.push(ContentBlock::ToolResult {
        tool_use_id: denied.id.to_owned(),
        content: ToolResultContent::Text(denied.message.clone()),
        is_error: Some(true),
    });
    if let Some(stream_tx) = stream_tx
        && let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolResult {
            identity: identity.clone(),
            tool_id: denied.id.to_owned(),
            tool_name: denied.name.to_owned(),
            result: recorded_message,
            is_error: true,
            duration_ms: 0,
            outcome: outcome_label,
        })
    {
        record_stream_send_error(tool_ctx, denied.name, "denied_tool_result", &e);
    }
}

fn emit_tool_start(
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    identity: &TurnEventIdentity,
    tool_id: &str,
    tool_name: &str,
    tool_input: &serde_json::Value,
) {
    if let Some(stream_tx) = stream_tx
        && let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolStart {
            identity: identity.clone(),
            tool_id: tool_id.to_owned(),
            tool_name: tool_name.to_owned(),
            input: tool_input.clone(),
        })
    {
        record_stream_send_error(tool_ctx, tool_name, "tool_start", &e);
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors TurnStreamEvent::ToolResult's own field list"
)]
fn emit_tool_result(
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    tool_ctx: &ToolContext,
    identity: &TurnEventIdentity,
    tool_id: &str,
    tool_name: &str,
    result: String,
    is_error: bool,
    duration_ms: u64,
    outcome: String,
) {
    if let Some(stream_tx) = stream_tx
        && let Err(e) = stream_tx.try_send(TurnStreamEvent::ToolResult {
            identity: identity.clone(),
            tool_id: tool_id.to_owned(),
            tool_name: tool_name.to_owned(),
            result,
            is_error,
            duration_ms,
            outcome,
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
    identity: &TurnEventIdentity,
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
            identity,
            &call.id,
            &call.name,
            result,
            call.is_error,
            call.duration_ms,
            call.outcome_label().to_owned(),
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
    use hermeneus::types::{Message, Role};

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
            content: content_for_pipeline_message(m),
            cache_breakpoint: m.cache_breakpoint,
        })
        .collect()
}

fn content_for_pipeline_message(m: &crate::pipeline::PipelineMessage) -> hermeneus::types::Content {
    use hermeneus::types::{Content, ContentBlock};

    match m.role.as_str() {
        "assistant" => match (&m.tool_call_id, &m.tool_name) {
            (Some(tool_call_id), Some(tool_name)) => match serde_json::from_str(&m.content) {
                Ok(input) => Content::Blocks(vec![ContentBlock::ToolUse {
                    id: tool_call_id.clone(),
                    name: tool_name.clone(),
                    input,
                }]),
                Err(error) => {
                    warn!(
                        tool_call_id = %tool_call_id,
                        tool_name = %tool_name,
                        %error,
                        "historical tool-use input is not valid JSON; using assistant text content"
                    );
                    Content::Text(m.content.clone())
                }
            },
            _ => Content::Text(m.content.clone()),
        },
        "tool_result" => {
            if let Some(tool_call_id) = &m.tool_call_id {
                Content::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: tool_call_id.clone(),
                    content: ToolResultContent::Text(m.content.clone()),
                    is_error: m.tool_is_error,
                }])
            } else {
                warn!("historical tool-result message is missing tool_call_id; using text content");
                Content::Text(m.content.clone())
            }
        }
        _ => Content::Text(m.content.clone()),
    }
}

/// Outcome of executing a single tool call: the persisted [`ToolCall`]
/// record, the LLM-facing [`ContentBlock::ToolResult`] block, and the
/// `is_error` flag the outer loop feeds into the loop detector.
struct SingleToolOutcome {
    call: ToolCall,
    result_block: ContentBlock,
    is_error: bool,
}

fn normalize_tool_result(
    result: organon::error::Result<ToolResult>,
    duration_ms: u64,
) -> (ToolResultContent, bool, Option<String>) {
    match result {
        Ok(mut result) => {
            if let Some(ref mut diagnostics) = result.diagnostics {
                diagnostics.duration_ms = duration_ms;
                let diagnostic_text = diagnostics.to_llm_text();
                result.content = inject_diagnostics(result.content, &diagnostic_text);
            }
            // WHY the accessor methods, not a direct match: `ToolOutcome` is
            // `#[non_exhaustive]` — matching its variants directly here would
            // need a wildcard arm that silently swallows any future variant
            // organon adds. `is_partial()`/`partial_reasons()`/
            // `failure_reason()` are the crate's own forward-compatible
            // surface for exactly this.
            let outcome_detail = if result.outcome.is_partial() {
                Some(result.outcome.partial_reasons().join("; "))
            } else {
                let reason = result.outcome.failure_reason();
                (!reason.is_empty()).then(|| reason.to_owned())
            };
            (result.content, result.is_error, outcome_detail)
        }
        // WHY None, not a synthesized detail: this branch is a dispatch-level
        // failure (the executor itself errored, e.g. tool not found in the
        // registry) rather than an organon `ToolOutcome::Failure` — there is
        // no `FailureInfo::reason` to carry, and `msg` (via `content`) is
        // already the fuller human-readable message.
        Err(error) => (
            ToolResultContent::text(format!("Tool error: {error}")),
            true,
            None,
        ),
    }
}

/// Execute one prepared tool call: invoke the executor, truncate + log + build
/// the (`ToolCall`, `ContentBlock::ToolResult`) pair. Loop-detection
/// bookkeeping is handled by the caller.
#[expect(
    clippy::too_many_arguments,
    reason = "dispatch needs tool id, name, input, registry, context, limits, receipt infra, and approval outcome"
)]
#[expect(
    clippy::expect_used,
    reason = "ToolResultContent contains only JSON-native strings and arrays; serialization cannot fail"
)]
async fn dispatch_single_tool(
    tool_id: &str,
    tool_name: &str,
    execution_input: &PreparedToolInput,
    persisted_input: &serde_json::Value,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    max_tool_result_bytes: u32,
    // WHY non-optional (#4835): every production caller of this function
    // already has a signer -- `SessionState::receipt_signer` is
    // constructed unconditionally per session (never `Option`), so the
    // one real call site (`execute/mod.rs`) has always passed
    // `Some(&session.receipt_signer)`. Making the parameter itself
    // non-optional turns "receipts are conventionally always emitted on
    // this path" into "a future caller cannot pass no signer and
    // silently skip receipt issuance" -- a compile-time invariant instead
    // of a runtime convention. The `#[cfg(test)]`-only `dispatch_tools`
    // wrapper still accepts `Option` for its ~20 existing test call
    // sites and resolves a throwaway signer when `None`.
    receipt_signer: &organon::receipts::ReceiptSigner,
    receipt_ledger: Option<&std::sync::Mutex<organon::receipts::ReceiptLedger>>,
    approval_requirement: ApprovalRequirement,
    // WHY plain &str not Option (#4835): the one real caller
    // (`dispatch_tool_items`) always has an already-resolved approval
    // outcome by this point in the loop -- every branch of its approval
    // match assigns one before calling this function.
    approval_outcome: &str,
) -> error::Result<SingleToolOutcome> {
    let start = std::time::Instant::now();

    // WHY(#5225): journal the call as `Started` *before* the side-effecting
    // future is polled, with no `.await` in between -- the same
    // cancel-safety shape `record`/`record_v2` below already rely on. If
    // this future is dropped while `execute_prepared` is in flight (turn
    // cancellation, actor restart), the next turn's `reconcile_interrupted`
    // call finds this entry still `Started` and surfaces it rather than
    // losing the fact that the side effect was ever attempted.
    if let Some(ledger) = receipt_ledger {
        let mut guard = ledger.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("receipt_ledger lock poisoned, recovering with last value");
            poisoned.into_inner()
        });
        guard.journal_started(
            tool_id.to_owned(),
            tool_name.to_owned(),
            persisted_input.to_string(),
            jiff::Timestamp::now(),
        );
    }

    let result = tools.execute_prepared(execution_input, tool_ctx).await;

    // WHY(#5225): resolve the journal entry immediately on return, before
    // any further `.await` -- see the `Started` write above.
    if let Some(ledger) = receipt_ledger {
        let mut guard = ledger.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("receipt_ledger lock poisoned, recovering with last value");
            poisoned.into_inner()
        });
        guard.journal_completed(tool_id, jiff::Timestamp::now());
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "u128→u64: tool execution duration won't exceed u64::MAX milliseconds"
    )]
    let duration_ms = start.elapsed().as_millis() as u64; // kanon:ignore RUST/as-cast

    let (content, is_error, outcome_detail) = normalize_tool_result(result, duration_ms);

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

    // WHY unconditional (#4835): receipt issuance is a runtime invariant
    // on this path now that `receipt_signer` is non-optional -- see this
    // function's parameter docs.
    //
    // WHY(#6808): the ledger retains only redacted display copies. Receipt V2
    // separately commits (under the ephemeral session key) to the exact
    // prepared executor-bound JSON and bounded result content, plus the tool,
    // policy, and approval identities that admitted execution. This does not
    // claim inode-level physical-effect identity for path-based OS calls.
    let redaction = tools
        .capability_metadata(&execution_input.as_tool_input().name)
        .redaction;
    let (content, receipt) = {
        organon::metrics::record_receipt(tool_name, "emitted");
        let ts = jiff::Timestamp::now();
        let actual_result = content.text_summary();
        let result_text = redacted_trace_result(&redaction, &actual_result);
        let output_value = serde_json::to_value(&content)
            .expect("ToolResultContent is composed exclusively of JSON-native values");
        let attestation = receipt_signer.attest_v2(
            tool_id,
            tool_name,
            &execution_input.as_tool_input().arguments,
            &output_value,
            approval_requirement.to_string(),
            approval_outcome,
            redaction.clone(),
            ts,
        );
        let receipt_str = receipt_signer.sign_v2(&attestation);
        if let Some(ledger) = receipt_ledger {
            let mut guard = ledger.lock().unwrap_or_else(|poisoned| {
                tracing::warn!("receipt_ledger lock poisoned, recovering with last value");
                poisoned.into_inner()
            });
            guard.record_v2(
                receipt_str.clone(),
                attestation,
                persisted_input.to_string(),
                result_text.clone(),
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
    };

    // WHY(#6808): the persisted record's result text follows the declared
    // policy even though the LLM-facing `result_block` above does not --
    // the model mid-turn needs the real output; the trace does not.
    let recorded_result = redacted_trace_result(&redaction, &content.text_summary());
    let outcome_detail = outcome_detail
        .as_deref()
        .map(|detail| redacted_trace_result(&redaction, detail));
    let call = ToolCall {
        id: tool_id.to_owned(),
        name: tool_name.to_owned(),
        input: persisted_input.clone(),
        result: Some(recorded_result),
        is_error,
        duration_ms,
        approval: None,
        receipt,
        outcome_detail,
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
/// status is known). On [`LoopVerdict::Warn`], stops executing the remaining
/// tools, records an error result for each so no `tool_use` block is left
/// unpaired, and returns the warning. On [`LoopVerdict::Halt`], returns an error.
///
/// Legacy tuple adapter used by direct dispatch tests and callers that have no
/// pre-dispatch denials to preserve.
#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "dispatch needs tool uses, registry, context, detector, calls, iterations, limits, and receipt infra"
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
    let items: Vec<ToolDispatchItem> = tool_uses.iter().cloned().map(Into::into).collect();
    // WHY: test-only adapter — its ~20 call sites exercise dispatch behavior,
    // not identity propagation (that is covered by execute-level streaming
    // tests), so a placeholder identity keeps them unchanged.
    let identity = TurnEventIdentity {
        turn_id: Ulid::new(),
        session_id: "test-session".to_owned(),
        request_id: None,
    };
    // WHY resolve a throwaway signer on `None` (#4835): this test-only
    // adapter keeps its `Option` parameter so its ~20 existing call sites
    // (most passing `None` -- receipt behavior isn't what they're
    // testing) don't all need a real signer threaded through, while
    // `dispatch_tool_items` itself gets the non-optional, always-signs
    // guarantee real callers rely on.
    let owned_signer;
    let signer = if let Some(s) = receipt_signer {
        s
    } else {
        owned_signer = organon::receipts::ReceiptSigner::new_session();
        &owned_signer
    };
    dispatch_tool_items(
        &items,
        tools,
        tool_ctx,
        loop_detector,
        all_tool_calls,
        iterations,
        stream_tx,
        approval_gate,
        policy,
        max_tool_result_bytes,
        signer,
        receipt_ledger,
        &identity,
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "dispatch needs tool items, registry, context, detector, calls, iterations, limits, and receipt infra"
)]
#[expect(
    clippy::too_many_lines,
    reason = "single approval-aware dispatch loop owns the full per-tool lifecycle"
)]
pub(super) async fn dispatch_tool_items(
    tool_items: &[ToolDispatchItem],
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    loop_detector: &mut LoopDetector,
    all_tool_calls: &mut Vec<ToolCall>,
    iterations: u32,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
    approval_gate: Option<&ApprovalGate>,
    policy: &ToolDispatchPolicy,
    max_tool_result_bytes: u32,
    // WHY non-optional: see `dispatch_single_tool`'s parameter docs (#4835).
    receipt_signer: &organon::receipts::ReceiptSigner,
    receipt_ledger: Option<&std::sync::Mutex<organon::receipts::ReceiptLedger>>,
    identity: &TurnEventIdentity,
) -> error::Result<DispatchResult> {
    let mut tool_results: Vec<ContentBlock> = Vec::new();
    let mut unexecuted: Vec<String> = Vec::new();

    for (index, item) in tool_items.iter().enumerate() {
        let (tool_id, tool_name, tool_input) = match item {
            ToolDispatchItem::Ready { id, name, input } => (id, name, input),
            ToolDispatchItem::Denied {
                id,
                name,
                input,
                message,
                outcome,
            } => {
                record_denied_call(
                    all_tool_calls,
                    &mut unexecuted,
                    &mut tool_results,
                    stream_tx,
                    tool_ctx,
                    tools,
                    identity,
                    &DeniedToolCall {
                        id,
                        name,
                        input,
                        message: message.clone(),
                        approval: Some(outcome),
                    },
                );
                continue;
            }
        };

        if let Some(denial) = policy
            .denial_for(tools, tool_id, tool_name, tool_input)
            .or_else(|| parse_error_denial(tool_input))
        {
            warn!(
                tool = %tool_name,
                tool_id = %tool_id,
                reason = denial.log_reason(),
                "tool call denied by dispatch policy"
            );
            record_denied_call(
                all_tool_calls,
                &mut unexecuted,
                &mut tool_results,
                stream_tx,
                tool_ctx,
                tools,
                identity,
                &DeniedToolCall {
                    id: tool_id,
                    name: tool_name,
                    input: tool_input,
                    message: denial.message(tool_name),
                    approval: Some(denial.outcome()),
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
        let substitution_failed = tool_ctx.services.as_ref().is_some_and(|services| {
            substitute_in_json(&mut substituted_args, &services.secret_vault).is_err()
        });
        let unprepared_input = ToolInput {
            name: tool_name_id,
            tool_use_id: tool_id.clone(),
            arguments: substituted_args,
        };
        let prepared_input = if substitution_failed {
            Err("Tool error: secret substitution failed")
        } else {
            tools
                .prepare_input(&unprepared_input, tool_ctx)
                .map_err(|_error| "Tool error: input preparation failed")
        };
        let prepared_input = match prepared_input {
            Ok(prepared) => prepared,
            Err(message) => {
                let msg = message.to_owned();
                let recorded_message =
                    redacted_trace_result(&redaction_policy_for(tools, tool_name), &msg);
                crate::metrics::record_tool_failure(tool_ctx.nous_id.as_ref(), tool_name);
                let outcome = SingleToolOutcome {
                    call: ToolCall {
                        id: tool_id.clone(),
                        name: tool_name.clone(),
                        input: redacted_surface_input(tools, tool_name, tool_input),
                        result: Some(recorded_message),
                        is_error: true,
                        duration_ms: 0,
                        approval: None,
                        receipt: None,
                        outcome_detail: None,
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
                    identity,
                    outcome,
                );
                let input_hash = simple_hash(tool_input);
                match loop_detector.record(tool_name, &input_hash, is_error) {
                    LoopVerdict::Ok => {}
                    LoopVerdict::Warn { message, .. } => {
                        record_undispatched_calls(
                            all_tool_calls,
                            &mut unexecuted,
                            &mut tool_results,
                            stream_tx,
                            tool_ctx,
                            tools,
                            identity,
                            tool_items.get(index + 1..).unwrap_or(&[]),
                        );
                        return Ok(DispatchResult {
                            blocks: tool_results,
                            loop_warning: Some(message),
                            unexecuted,
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
        };
        let approval = match tools.approval_requirement_for_input(prepared_input.as_tool_input()) {
            Ok(approval) => approval,
            Err(e) => {
                record_denied_call(
                    all_tool_calls,
                    &mut unexecuted,
                    &mut tool_results,
                    stream_tx,
                    tool_ctx,
                    tools,
                    identity,
                    &DeniedToolCall {
                        id: tool_id,
                        name: tool_name,
                        input: tool_input,
                        message: format!("tool_policy: Tool '{tool_name}' call rejected: {e}"),
                        approval: Some(TOOL_OUTCOME_DENIED_BY_GROUP),
                    },
                );
                continue;
            }
        };

        // WHY(#6808): compute the durable/replay copy once from the
        // placeholder-form model input. It feeds `ToolStart`, persisted
        // `ToolCall`, hooks, replay approval, and the receipt ledger's display
        // copy. The executor and live approver use separately redacted views
        // of the prepared input; Receipt V2 binds that exact input with a
        // session-keyed commitment. The loop detector still hashes the
        // model-emitted original because redaction is not its concern.
        let trace_input = redacted_surface_input(tools, tool_name, tool_input);

        // WHY(#3958, ADR-005): one decision boundary protects streaming,
        // fallback, and batch dispatch. Unknown future requirements block.
        let approval_outcome = match approval {
            ApprovalRequirement::None => {
                record_approval_policy_outcome(
                    tool_id,
                    tool_name,
                    approval,
                    approval_gate.is_some(),
                    APPROVAL_OUTCOME_AUTO_APPROVED,
                );
                emit_approval_resolved(
                    stream_tx,
                    tool_ctx,
                    identity,
                    tool_id,
                    tool_name,
                    APPROVAL_OUTCOME_AUTO_APPROVED,
                );
                APPROVAL_OUTCOME_AUTO_APPROVED
            }
            ApprovalRequirement::Advisory => {
                record_approval_policy_outcome(
                    tool_id,
                    tool_name,
                    approval,
                    approval_gate.is_some(),
                    APPROVAL_OUTCOME_ADVISORY_AUTO,
                );
                emit_approval_resolved(
                    stream_tx,
                    tool_ctx,
                    identity,
                    tool_id,
                    tool_name,
                    APPROVAL_OUTCOME_ADVISORY_AUTO,
                );
                APPROVAL_OUTCOME_ADVISORY_AUTO
            }
            ApprovalRequirement::Required | ApprovalRequirement::Mandatory | _ => {
                // The connected approver sees the minimum policy-permitted
                // evidence from the exact prepared input it is authorizing.
                // Replay/history receives the independently produced trace
                // copy, which never contains vault/file-expanded values.
                let mut live_input = prepared_input.as_tool_input().arguments.clone();
                // Vault values are known secrets regardless of length or
                // shape. Preserve that provenance from the placeholder-form
                // input before the generic/declared policy pass.
                redact_resolved_secrets_in_prepared_json(
                    tool_input,
                    &unprepared_input.arguments,
                    &mut live_input,
                );
                let live_approval_input = redacted_live_approval_input(
                    tools,
                    &prepared_input.as_tool_input().name,
                    &live_input,
                );
                let approval_event_available = emit_approval_required(
                    stream_tx,
                    tool_ctx,
                    identity,
                    tool_id,
                    tool_name,
                    LiveApprovalEvidence::new(live_approval_input),
                    &trace_input,
                    approval,
                );
                let (choice, outcome) = if !approval_event_available {
                    warn!(
                        tool = tool_name.as_str(),
                        tool_id = tool_id.as_str(),
                        "approval-required tool call could not reach approver - default-deny"
                    );
                    (
                        ApprovalChoice::Denied,
                        APPROVAL_OUTCOME_EVENT_UNAVAILABLE_DENIED,
                    )
                } else if let Some(gate) = approval_gate {
                    let choice = gate.await_decision(tool_id).await;
                    (choice, choice.as_wire_str())
                } else {
                    warn!(
                        tool = tool_name.as_str(),
                        tool_id = tool_id.as_str(),
                        approval_requirement = %approval,
                        "approval-required tool call with no approval gate wired - default-deny"
                    );
                    (ApprovalChoice::Denied, APPROVAL_OUTCOME_NO_GATE_DENIED)
                };
                record_approval_policy_outcome(
                    tool_id,
                    tool_name,
                    approval,
                    approval_gate.is_some(),
                    outcome,
                );
                emit_approval_resolved(stream_tx, tool_ctx, identity, tool_id, tool_name, outcome);
                if matches!(choice, ApprovalChoice::Denied) {
                    let message = if outcome == APPROVAL_OUTCOME_NO_GATE_DENIED {
                        format!(
                            "Tool '{tool_name}' execution denied by approval policy: \
                             {approval} approval requires an approval gate."
                        )
                    } else if outcome == APPROVAL_OUTCOME_EVENT_UNAVAILABLE_DENIED {
                        format!(
                            "Tool '{tool_name}' execution denied by approval policy: \
                             the live approval event was unavailable."
                        )
                    } else {
                        format!("Tool '{tool_name}' execution denied by user.")
                    };
                    record_denied_call(
                        all_tool_calls,
                        &mut unexecuted,
                        &mut tool_results,
                        stream_tx,
                        tool_ctx,
                        tools,
                        identity,
                        &DeniedToolCall {
                            id: tool_id,
                            name: tool_name,
                            input: tool_input,
                            message,
                            approval: Some(outcome),
                        },
                    );
                    continue;
                }
                outcome
            }
        };

        emit_tool_start(
            stream_tx,
            tool_ctx,
            identity,
            tool_id,
            tool_name,
            &trace_input,
        );

        let mut outcome = dispatch_single_tool(
            tool_id,
            tool_name,
            &prepared_input,
            &trace_input,
            tools,
            tool_ctx,
            max_tool_result_bytes,
            receipt_signer,
            receipt_ledger,
            approval,
            approval_outcome,
        )
        .await?;
        outcome.call.approval = Some(approval_outcome.to_owned());

        let is_error = record_tool_outcome(
            all_tool_calls,
            &mut tool_results,
            stream_tx,
            tool_ctx,
            identity,
            outcome,
        );

        let input_hash = simple_hash(tool_input);
        match loop_detector.record(tool_name, &input_hash, is_error) {
            LoopVerdict::Ok => {}
            LoopVerdict::Warn { message, .. } => {
                record_undispatched_calls(
                    all_tool_calls,
                    &mut unexecuted,
                    &mut tool_results,
                    stream_tx,
                    tool_ctx,
                    tools,
                    identity,
                    tool_items.get(index + 1..).unwrap_or(&[]),
                );
                return Ok(DispatchResult {
                    blocks: tool_results,
                    loop_warning: Some(message),
                    unexecuted,
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
        unexecuted,
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use organon::testing::MockToolExecutor;
    use organon::types::{InputSchema, Reversibility, ToolCategory, ToolDef, ToolGroupId};

    #[test]
    fn is_denial_outcome_recognizes_every_denial_class() {
        for outcome in [
            TOOL_OUTCOME_DENIED_BY_ROLE,
            TOOL_OUTCOME_DENIED_BY_GROUP,
            TOOL_OUTCOME_DENIED_BY_HOOK,
            TOOL_OUTCOME_DENIED_INACTIVE,
            TOOL_OUTCOME_NOT_FOUND,
            TOOL_OUTCOME_FAILED,
            TOOL_OUTCOME_UNDISPATCHED,
            APPROVAL_OUTCOME_NO_GATE_DENIED,
        ] {
            assert!(
                is_denial_outcome(outcome),
                "{outcome} should be recognized as a denial class"
            );
        }
    }

    #[test]
    fn is_denial_outcome_rejects_approval_grant_outcomes() {
        // WHY: these mean the call WAS approved (and may go on to execute),
        // the opposite of a denial — conflating them would misreport an
        // executed-and-failed call as though policy had denied it (#4558).
        for outcome in [
            APPROVAL_OUTCOME_AUTO_APPROVED,
            APPROVAL_OUTCOME_ADVISORY_AUTO,
            "unknown_arbitrary_string",
        ] {
            assert!(
                !is_denial_outcome(outcome),
                "{outcome} should not be recognized as a denial class"
            );
        }
    }

    fn test_tool_def(name: &str) -> ToolDef {
        ToolDef {
            name: ToolName::new(name).expect("valid test tool name"),
            description: format!("Test tool: {name}"),
            extended_description: None,
            input_schema: InputSchema {
                properties: indexmap::IndexMap::default(),
                required: vec![],
            },
            category: ToolCategory::Workspace,
            reversibility: Reversibility::FullyReversible,
            auto_activate: true,
            groups: vec![ToolGroupId::Read],
            tags: vec![],
        }
    }

    #[test]
    fn dispatch_policy_denies_provider_parse_error_payload() {
        let mut tools = ToolRegistry::new();
        tools
            .register(
                test_tool_def("read_file"),
                Box::new(MockToolExecutor::text("ok")),
            )
            .expect("register test tool");
        let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

        let items = policy.filter_tool_uses(
            vec![(
                "call-malformed".to_owned(),
                "read_file".to_owned(),
                serde_json::json!({
                    "_parse_error": "malformed tool input: expected value at line 1 column 1",
                    "_raw_input": "{not json"
                }),
            )],
            &tools,
        );

        assert_eq!(items.len(), 1);
        match items.first().expect("one dispatch item") {
            ToolDispatchItem::Denied {
                id,
                name,
                input,
                message,
                outcome,
            } => {
                assert_eq!(id, "call-malformed");
                assert_eq!(name, "read_file");
                assert_eq!(*outcome, TOOL_OUTCOME_FAILED);
                assert!(
                    message.starts_with("malformed tool input:"),
                    "expected provider parse error denial, got {message:?}"
                );
                assert!(
                    input.get("_raw_input").is_some(),
                    "denied call should retain diagnostic input payload"
                );
            }
            ready @ ToolDispatchItem::Ready { .. } => {
                panic!("malformed provider arguments must not be dispatch-ready: {ready:?}");
            }
        }
    }

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
