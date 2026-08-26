// kanon:ignore RUST/file-too-long WHY: inbound dispatch, command lifecycle records, and dispatch tests share private helpers in one module
//! Background dispatch loop: routes inbound messages to nous actors.

use std::sync::Arc;
use std::time::Instant;

use mneme::store::{AppendCommandLifecycleRecord, SessionStore};
use mneme::types::{
    CommandDelivery, CommandDeliveryFailureClass, CommandDeliveryStatus, CommandFailureClass,
    CommandInvocationStatus, CommandLifecycleEvent, CommandResultStatus, RedactedCommand,
    RedactedCommandOrigin,
};
use sha2::{Digest as _, Sha256};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, debug, info, warn};

use agora::command::{self, AgentSnapshot, ChannelSnapshot, CommandContext};
use agora::dedupe::DedupeFilter;
use agora::listener::ChannelListener;
use agora::registry::ChannelRegistry;
use agora::router::{MessageRouter, reply_target};
use agora::types::{InboundMessage, SendParams};
use nous::manager::NousManager;
use organon::types::BlackboardViewer;
use taxis::config::{CommandTier, InboundCommandPolicy};

const UNKNOWN_COMMAND_REPLY: &str = "Unknown command.";

/// Everything the dispatcher loop needs beyond the listener itself.
pub(crate) struct DispatcherParts {
    pub router: Arc<MessageRouter>,
    pub nous_manager: Arc<NousManager>,
    pub channel_registry: Arc<ChannelRegistry>,
    pub session_store: Arc<tokio::sync::Mutex<SessionStore>>,
    pub command_policy: InboundCommandPolicy,
}

/// Spawn a background task that dispatches inbound messages to nous actors.
///
/// Runs until the listener's stream closes (all providers stopped).
/// Dispatch goes through [`ChannelListener::run`], so the configured
/// `MessagingConfig::max_concurrent_handlers` cap bounds in-flight dispatch
/// tasks on this path — before, the runtime consumed the receiver directly
/// and spawned one unbounded task per message.
pub(crate) fn spawn_dispatcher(
    task_tracker: &TaskTracker,
    listener: ChannelListener,
    parts: DispatcherParts,
    mut ready_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let span = tracing::info_span!("message_dispatcher");
    task_tracker.spawn(
        async move {
            while !*ready_rx.borrow_and_update() {
                if ready_rx.changed().await.is_err() {
                    warn!("ready channel dropped before ready signal");
                    return;
                }
            }
            info!("dispatch loop started");

            let parts = Arc::new(parts);
            // WHY: providers can redeliver during the current dispatcher
            // lifetime; this process-local bounded remember-set drops repeat
            // sightings before any agent work is scheduled. It is not a
            // restart-safe ingress journal.
            let dedupe = Arc::new(std::sync::Mutex::new(DedupeFilter::with_capacity(
                agora::dedupe::DEFAULT_DEDUPE_CAPACITY,
            )));

            listener
                .run(move |msg| {
                    let parts = Arc::clone(&parts);
                    let dedupe = Arc::clone(&dedupe);
                    let msg_span = tracing::info_span!(
                        "dispatch",
                        channel = %msg.channel,
                        sender = %agora::redact::identifier(&msg.sender),
                        account = %agora::redact::optional_identifier(msg.account_id.as_deref()),
                    );
                    async move {
                        dispatch_one(
                            msg,
                            Arc::clone(&parts.router),
                            Arc::clone(&parts.nous_manager),
                            Arc::clone(&parts.channel_registry),
                            Arc::clone(&parts.session_store),
                            parts.command_policy.clone(),
                            dedupe,
                        )
                        .instrument(msg_span)
                        .await;
                    }
                })
                .await;

            info!("dispatch loop stopped");
        }
        .instrument(span),
    )
}

async fn dispatch_one(
    msg: InboundMessage,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
    session_store: Arc<tokio::sync::Mutex<SessionStore>>,
    command_policy: InboundCommandPolicy,
    dedupe: Arc<std::sync::Mutex<DedupeFilter>>,
) {
    // WHY: checked before routing so a redelivered message does no agent work
    // at all. Held only for the lookup — never across an await.
    let is_new = {
        let mut guard = dedupe
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.check_and_record(&msg)
    };
    if !is_new {
        debug!(
            channel = %msg.channel,
            "duplicate inbound delivery ignored"
        );
        return;
    }

    let Some(decision) = router.resolve(&msg) else {
        warn!(
            channel = %msg.channel,
            sender = %agora::redact::identifier(&msg.sender),
            "no route for inbound message, dropping"
        );
        return;
    };

    // SECURITY(#5193): unknown input is handled before session normalization,
    // snapshots, and durable command records. Neither the
    // attacker-controlled name nor arguments leave this branch.
    let parsed_command = command::parse(&msg.text);
    if matches!(parsed_command, Some(command::Command::Unknown)) {
        send_reply(
            &msg,
            UNKNOWN_COMMAND_REPLY,
            ReplyPurpose::Command,
            &channel_registry,
        )
        .await;
        return;
    }

    // SECURITY(#5193): authorize known commands before touching a route-derived
    // session key. Denied and unknown command text receive the same fixed reply,
    // so malformed route templates cannot become a command-vocabulary oracle.
    if let Some(cmd) = parsed_command.as_ref()
        && !command_policy.allows(cmd.name(), decision.command_tier)
    {
        agora::metrics::record_command_denied(&msg.channel);
        warn!(
            channel = %msg.channel,
            command = cmd.name(),
            "inbound command denied by policy"
        );
        send_reply(
            &msg,
            UNKNOWN_COMMAND_REPLY,
            ReplyPurpose::Command,
            &channel_registry,
        )
        .await;
        return;
    }

    // Help and liveness are public, fixed operations. Keep them independent
    // of route-template validity, session storage, and private runtime state.
    if let Some(cmd @ (command::Command::Help | command::Command::Ping)) = parsed_command.as_ref() {
        let visible_commands = visible_command_names(&command_policy, decision.command_tier);
        let reply = command_without_runtime_snapshot(cmd, decision.nous_id, "", visible_commands)
            .unwrap_or_else(|| UNKNOWN_COMMAND_REPLY.to_owned());
        send_reply(&msg, &reply, ReplyPurpose::Command, &channel_registry).await;
        return;
    }

    // WHY: Session keys are built from external identifiers by the router's
    // template expansion. Normalize before any use so logs, lifecycle notes,
    // and store lookups never carry raw phone numbers, Matrix IDs, or group IDs.
    let session_key = match normalize_session_key(
        &msg.channel,
        msg.account_id.as_deref(),
        &decision.session_key,
    ) {
        Ok(key) => key,
        Err(e) => {
            warn!(
                error = %e,
                matched_by = ?decision.matched_by,
                "invalid routed session key, dropping message"
            );
            return;
        }
    };

    // NOTE: `!`-commands are intercepted before reaching the nous agent.
    // Plain turns fall through to send_turn as before.
    if let Some(cmd) = parsed_command {
        handle_command_dispatch(CommandDispatch {
            msg: &msg,
            cmd: &cmd,
            nous_id: decision.nous_id,
            session_key: &session_key,
            nous_manager: &nous_manager,
            channel_registry: &channel_registry,
            session_store: &session_store,
            command_policy: &command_policy,
            command_tier: decision.command_tier,
        })
        .await;
        return;
    }

    let Some(handle) = nous_manager.get(decision.nous_id) else {
        warn!(
            nous_id = %decision.nous_id,
            "routed to unknown nous actor, dropping"
        );
        return;
    };

    info!(
        nous_id = %decision.nous_id,
        session_key = %session_key,
        matched_by = ?decision.matched_by,
        "dispatching turn"
    );

    // WHY(#5219): this turn arrived over an external channel — the ingress
    // marker flows through to the routing-outcome record so channel-origin
    // turns carry their privacy boundary posture explicitly instead of
    // silently reading as operator-direct, cloud-default turns.
    let ingress = aletheia_routing::types::IngressSource::ExternalChannel {
        channel: Arc::from(msg.channel.as_str()),
    };
    let turn_result = match handle
        .send_turn_with_ingress(&session_key, &msg.text, ingress)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            warn!(error = %e, nous_id = %decision.nous_id, "turn failed");
            return;
        }
    };

    send_reply(
        &msg,
        &turn_result.content,
        ReplyPurpose::Turn,
        &channel_registry,
    )
    .await;
}

struct CommandDispatch<'a> {
    msg: &'a InboundMessage,
    cmd: &'a command::Command,
    nous_id: &'a str,
    session_key: &'a str,
    nous_manager: &'a NousManager,
    channel_registry: &'a ChannelRegistry,
    session_store: &'a Arc<tokio::sync::Mutex<SessionStore>>,
    command_policy: &'a InboundCommandPolicy,
    command_tier: CommandTier,
}

struct StartedCommandRecord {
    session_id: String,
    delivery_key: String,
    origin: RedactedCommandOrigin,
    command: RedactedCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplyPurpose {
    Turn,
    Command,
}

impl ReplyPurpose {
    fn tag(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Command => "command",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplyDelivery {
    Sent,
    Failed(ReplyFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplyFailure {
    Provider,
    Registry,
}

impl ReplyDelivery {
    fn audit_record(self) -> CommandDelivery {
        match self {
            Self::Sent => CommandDelivery {
                status: CommandDeliveryStatus::Sent,
                failure_class: None,
            },
            Self::Failed(ReplyFailure::Provider) => CommandDelivery {
                status: CommandDeliveryStatus::Failed,
                failure_class: Some(CommandDeliveryFailureClass::ProviderFailure),
            },
            Self::Failed(ReplyFailure::Registry) => CommandDelivery {
                status: CommandDeliveryStatus::Failed,
                failure_class: Some(CommandDeliveryFailureClass::RegistryFailure),
            },
        }
    }
}

async fn handle_command_dispatch(dispatch: CommandDispatch<'_>) {
    let started_at = Instant::now();
    debug!(
        nous_id = %dispatch.nous_id,
        command = dispatch.cmd.name(),
        "dispatching !-command"
    );
    let command_record = match begin_command_record(
        dispatch.session_store,
        dispatch.msg,
        dispatch.cmd,
        dispatch.nous_id,
        dispatch.session_key,
        dispatch
            .nous_manager
            .get_config(dispatch.nous_id)
            .map(|config| config.generation.model.as_str()),
    )
    .await
    {
        Ok(record) => record,
        Err(e) => {
            warn!(
                error = %e,
                nous_id = %dispatch.nous_id,
                command = dispatch.cmd.name(),
                "failed to record !-command invocation"
            );
            send_reply(
                dispatch.msg,
                UNKNOWN_COMMAND_REPLY,
                ReplyPurpose::Command,
                dispatch.channel_registry,
            )
            .await;
            return;
        }
    };
    let visible_commands = visible_command_names(dispatch.command_policy, dispatch.command_tier);
    let reply_text = execute_command(
        dispatch.cmd,
        dispatch.nous_id,
        dispatch.session_key,
        dispatch.nous_manager,
        dispatch.channel_registry,
        visible_commands,
    )
    .await;
    let delivery = send_reply(
        dispatch.msg,
        &reply_text,
        ReplyPurpose::Command,
        dispatch.channel_registry,
    )
    .await;
    if let Err(e) = finish_command_record(
        dispatch.session_store,
        &command_record,
        None,
        started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
        delivery,
    )
    .await
    {
        warn!(
            error = %e,
            nous_id = %dispatch.nous_id,
            command = dispatch.cmd.name(),
            "failed to record !-command result"
        );
    }
}

async fn begin_command_record(
    session_store: &Arc<tokio::sync::Mutex<SessionStore>>,
    msg: &InboundMessage,
    cmd: &command::Command,
    nous_id: &str,
    session_key: &str,
    model: Option<&str>,
) -> Result<StartedCommandRecord, mneme::error::Error> {
    let delivery_key = command_delivery_key(msg, session_key, cmd.name());
    let session_id = koina::id::SessionId::new().to_string();
    let store = session_store.lock().await;
    let session = store.find_or_create_session(&session_id, nous_id, session_key, model, None)?;
    let origin = command_origin_record(msg);
    let command = command_record_command(cmd);
    let event = CommandLifecycleEvent::Invocation {
        status: CommandInvocationStatus::Started,
    };
    store.append_command_lifecycle_record(&AppendCommandLifecycleRecord {
        session_id: &session.id,
        delivery_key: &delivery_key,
        origin: &origin,
        command: &command,
        event: &event,
    })?;

    Ok(StartedCommandRecord {
        session_id: session.id,
        delivery_key,
        origin,
        command,
    })
}

async fn finish_command_record(
    session_store: &Arc<tokio::sync::Mutex<SessionStore>>,
    record: &StartedCommandRecord,
    failure_class: Option<CommandFailureClass>,
    duration_ms: u64,
    delivery: ReplyDelivery,
) -> Result<(), mneme::error::Error> {
    let status = if failure_class.is_some() {
        CommandResultStatus::Failed
    } else {
        CommandResultStatus::Succeeded
    };
    let event = CommandLifecycleEvent::Result {
        status,
        failure_class,
        duration_ms,
        delivery: delivery.audit_record(),
    };
    let store = session_store.lock().await;
    store.append_command_lifecycle_record(&AppendCommandLifecycleRecord {
        session_id: &record.session_id,
        delivery_key: &record.delivery_key,
        origin: &record.origin,
        command: &record.command,
        event: &event,
    })?;
    Ok(())
}

fn command_origin_record(msg: &InboundMessage) -> RedactedCommandOrigin {
    // WHY: the audit record is durable storage — channel identities go in
    // redacted, matching the log/span posture; the raw identifiers were never
    // needed to answer "which conversation did this command come from" (the
    // hashed session key already pins it).
    let sender_domain = format!("command-origin-sender:{}", msg.channel);
    let group_domain = format!("command-origin-group:{}", msg.channel);
    let account_domain = format!("command-origin-account:{}", msg.channel);
    let sender = agora::redact::opaque_identifier(&sender_domain, &msg.sender);
    let group = msg
        .group_id
        .as_deref()
        .map(|value| agora::redact::opaque_identifier(&group_domain, value));
    let account = msg
        .account_id
        .as_deref()
        .map(|value| agora::redact::opaque_identifier(&account_domain, value));
    RedactedCommandOrigin {
        channel: msg.channel.clone(),
        account_id: account,
        sender,
        group_id: group,
        thread_id: None,
        conversation_id: command_conversation_id(msg),
        timestamp_ms: msg.timestamp,
    }
}

/// Stable, opaque identity for the originating conversation.
///
/// Direct messages are scoped by sender; group messages by group. Both also
/// include the provider and logical account so identical wire identifiers on
/// different channel accounts cannot alias in durable lifecycle records.
fn command_conversation_id(msg: &InboundMessage) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aletheia.agora.command-conversation.v1\0");
    hash_field(&mut hasher, "channel", &msg.channel);
    hash_optional_field(&mut hasher, "account", msg.account_id.as_deref());
    match msg.group_id.as_deref() {
        Some(group_id) => {
            hash_field(&mut hasher, "kind", "group");
            hash_field(&mut hasher, "conversation", group_id);
        }
        None => {
            hash_field(&mut hasher, "kind", "direct");
            hash_field(&mut hasher, "conversation", &msg.sender);
        }
    }
    format!("sha256:{}", hex_lower(&hasher.finalize()))
}

fn command_record_command(cmd: &command::Command) -> RedactedCommand {
    RedactedCommand {
        name: cmd.name().to_owned(),
        args_redacted: cmd.redacted_args(),
    }
}

fn command_delivery_key(msg: &InboundMessage, session_key: &str, command_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aletheia.agora.command-delivery.v1\0");
    hash_field(&mut hasher, "inbound", &msg.dedupe_key());
    hash_field(&mut hasher, "session_key", session_key);
    hash_field(&mut hasher, "command", command_name);
    let digest = hasher.finalize();
    format!("sha256:{}", hex_lower(&digest))
}

fn reply_idempotency_key(msg: &InboundMessage, purpose: ReplyPurpose) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aletheia.agora.reply-idempotency.v1\0");
    hash_field(&mut hasher, "inbound", &msg.dedupe_key());
    hash_field(&mut hasher, "purpose", purpose.tag());
    format!("sha256:{}", hex_lower(&hasher.finalize()))
}

fn hash_field(hasher: &mut Sha256, label: &str, value: &str) {
    hasher.update(label.as_bytes());
    hasher.update(b"\0");
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}

fn hash_optional_field(hasher: &mut Sha256, label: &str, value: Option<&str>) {
    hash_field(
        hasher,
        &format!("{label}.presence"),
        if value.is_some() { "some" } else { "none" },
    );
    if let Some(value) = value {
        hash_field(hasher, label, value);
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        out.push(hex_digit(byte >> 4));
        out.push(hex_digit(byte & 0x0f));
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    char::from(match nibble {
        0..=9 => b'0' + nibble,
        10..=15 => b'a' + (nibble - 10),
        _ => b'?',
    })
}

/// Maximum byte length for a route-expanded session key before normalization.
///
/// WHY: Session keys are indexed by mneme and logged by dispatch; unbounded
/// template expansion could create unreadable or storage-expensive keys.
const MAX_SESSION_KEY_LEN: usize = 256;

#[derive(Debug)]
enum SessionKeyError {
    Empty,
    TooLong,
    InvalidChars,
}

impl std::fmt::Display for SessionKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "routed session key is empty"),
            Self::TooLong => write!(f, "routed session key exceeds {MAX_SESSION_KEY_LEN} bytes"),
            Self::InvalidChars => write!(f, "routed session key contains control characters"),
        }
    }
}

/// Normalize and validate a route-derived session key.
///
/// Routing templates expand raw external identifiers (phone numbers, Matrix IDs,
/// group IDs) directly into the session key. This replaces those values with a
/// stable SHA-256 digest so the same sender/group always resolves to the same
/// session while the raw identifier is redacted from logs, command records, and
/// the store. Invalid expansions (empty, oversized, or containing control bytes)
/// are rejected before they reach the agent turn.
fn normalize_session_key(
    channel: &str,
    account_id: Option<&str>,
    raw: &str,
) -> Result<String, SessionKeyError> {
    if raw.is_empty() {
        return Err(SessionKeyError::Empty);
    }
    if raw.len() > MAX_SESSION_KEY_LEN {
        return Err(SessionKeyError::TooLong);
    }
    if raw != raw.trim_matches(|c: char| c.is_ascii_whitespace()) {
        return Err(SessionKeyError::InvalidChars);
    }
    if raw.bytes().any(|b| b.is_ascii_control()) {
        return Err(SessionKeyError::InvalidChars);
    }

    let mut hasher = Sha256::new();
    hasher.update(b"aletheia.agora.session-key.v1\0");
    hash_field(&mut hasher, "channel", channel);
    hash_optional_field(&mut hasher, "account", account_id);
    hash_field(&mut hasher, "expanded", raw);
    let digest = hasher.finalize();
    Ok(format!("h:{}", hex_lower(&digest)))
}

/// Command names a sender may see in `!help`: the full surface for
/// operators, only the public subset otherwise.
fn visible_command_names(policy: &InboundCommandPolicy, tier: CommandTier) -> Vec<String> {
    if tier == CommandTier::Operator {
        command::known_command_names()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect()
    } else {
        policy
            .public_command_names()
            .map(ToOwned::to_owned)
            .collect()
    }
}

/// Build a `CommandContext` and execute a parsed command, returning the reply text.
#[expect(
    clippy::too_many_lines,
    reason = "command snapshot assembly stays local to dispatch"
)]
async fn execute_command(
    cmd: &command::Command,
    nous_id: &str,
    session_key: &str,
    nous_manager: &NousManager,
    channel_registry: &ChannelRegistry,
    visible_commands: Vec<String>,
) -> String {
    if let Some(reply) =
        command_without_runtime_snapshot(cmd, nous_id, session_key, visible_commands.clone())
    {
        return reply;
    }

    let needs_current_agent = matches!(
        cmd,
        command::Command::Status
            | command::Command::Sessions
            | command::Command::Uptime
            | command::Command::Model
            | command::Command::Think
            | command::Command::Info { agent_id: None }
    );
    let current_agent = if needs_current_agent {
        agent_snapshot(nous_manager, nous_id).await
    } else {
        None
    };

    // Fleet-wide enumeration is deliberately variant-gated. A command such
    // as `!whoami` or `!blackboard` must not trigger unrelated manager reads.
    let all_agents = match cmd {
        command::Command::Agents => {
            let statuses = nous_manager.list().await;
            statuses
                .into_iter()
                .map(|st| {
                    let model = nous_manager
                        .get_config(&st.id)
                        .map_or_else(String::new, |c| c.generation.model.clone());
                    let thinking_enabled = nous_manager
                        .get_config(&st.id)
                        .is_some_and(|c| c.generation.thinking_enabled);
                    let thinking_budget = nous_manager
                        .get_config(&st.id)
                        .map_or(0, |c| c.generation.thinking_budget);
                    AgentSnapshot {
                        id: st.id,
                        lifecycle: st.lifecycle.to_string(),
                        session_count: st.session_count,
                        active_session: st.active_session,
                        panic_count: st.panic_count,
                        uptime_secs: st.uptime.as_secs(),
                        model,
                        thinking_enabled,
                        thinking_budget,
                    }
                })
                .collect()
        }
        command::Command::Info {
            agent_id: Some(agent_id),
        } => agent_snapshot(nous_manager, agent_id)
            .await
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };

    // Gather channel health snapshots only for commands that need them.
    let channels = match cmd {
        command::Command::Channels => channel_registry
            .probe_all()
            .await
            .into_iter()
            .map(|(id, probe)| ChannelSnapshot {
                id,
                healthy: probe.ok,
                latency_ms: probe.latency_ms,
            })
            .collect(),
        _ => vec![],
    };

    #[cfg(feature = "recall")]
    let skills: Vec<String> = if matches!(cmd, command::Command::Skills) {
        let store = nous_manager
            .get_config(nous_id)
            .and_then(|cfg| nous_manager.knowledge_store_for_cohort(cfg.episteme_cohort.as_ref()));
        match store {
            Some(knowledge_store) => match knowledge_store.find_skills_for_nous(nous_id, 50) {
                Ok(facts) => facts
                    .iter()
                    .map(|fact| {
                        serde_json::from_str::<mneme::skill::SkillContent>(&fact.content)
                            .map_or_else(|_| fact.id.to_string(), |skill| skill.name)
                    })
                    .collect(),
                Err(e) => {
                    warn!(error = %e, "failed to load skills for nous");
                    Vec::new()
                }
            },
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    #[cfg(not(feature = "recall"))]
    let skills: Vec<String> = Vec::new();

    let blackboard_entries: Vec<String> = if matches!(cmd, command::Command::Blackboard) {
        match nous_manager.blackboard_store() {
            Some(blackboard_store) => {
                // WHY(#5032): scoped to the acting agent — no session id is
                // available at this chat-command layer (only the hashed
                // `session_key`, not the store's session UUID), so `Nous`
                // fails closed on SessionPrivate rows rather than guessing.
                let viewer = BlackboardViewer::Nous {
                    nous_id: nous_id.to_owned(),
                };
                match blackboard_store.list(&viewer) {
                    Ok(entries) => entries
                        .iter()
                        .map(|entry| {
                            format!(
                                "[{}] = {} (by {})",
                                entry.key, entry.value, entry.author_nous_id
                            )
                        })
                        .collect(),
                    Err(e) => {
                        warn!(error = %e, "failed to list blackboard entries");
                        Vec::new()
                    }
                }
            }
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let ctx = CommandContext {
        current_nous_id: nous_id.to_owned(),
        session_key: session_key.to_owned(),
        current_agent,
        all_agents,
        skills,
        blackboard_entries,
        channels,
        visible_commands,
    };

    command::execute(cmd, &ctx)
}

async fn agent_snapshot(nous_manager: &NousManager, nous_id: &str) -> Option<AgentSnapshot> {
    if let Some(handle) = nous_manager.get(nous_id) {
        match handle.status().await {
            Ok(st) => {
                let model = nous_manager
                    .get_config(nous_id)
                    .map_or_else(String::new, |c| c.generation.model.clone());
                let thinking_enabled = nous_manager
                    .get_config(nous_id)
                    .is_some_and(|c| c.generation.thinking_enabled);
                let thinking_budget = nous_manager
                    .get_config(nous_id)
                    .map_or(0, |c| c.generation.thinking_budget);
                Some(AgentSnapshot {
                    id: st.id,
                    lifecycle: st.lifecycle.to_string(),
                    session_count: st.session_count,
                    active_session: st.active_session,
                    panic_count: st.panic_count,
                    uptime_secs: st.uptime.as_secs(),
                    model,
                    thinking_enabled,
                    thinking_budget,
                })
            }
            Err(e) => {
                warn!(error = %e, nous_id, "failed to query agent status for command");
                None
            }
        }
    } else {
        None
    }
}

/// Execute commands whose response needs no runtime or private-store state.
///
/// This check runs before agent, knowledge, blackboard, or channel snapshots
/// are acquired. Public callers therefore cannot use `!help` or `!ping` to
/// drive privileged reads or couple liveness latency to fleet state.
fn command_without_runtime_snapshot(
    cmd: &command::Command,
    nous_id: &str,
    session_key: &str,
    visible_commands: Vec<String>,
) -> Option<String> {
    if !matches!(cmd, command::Command::Help | command::Command::Ping) {
        return None;
    }
    let ctx = CommandContext {
        current_nous_id: nous_id.to_owned(),
        session_key: session_key.to_owned(),
        current_agent: None,
        all_agents: Vec::new(),
        skills: Vec::new(),
        blackboard_entries: Vec::new(),
        channels: Vec::new(),
        visible_commands,
    };
    Some(command::execute(cmd, &ctx))
}

/// Send a reply back through the originating channel.
///
/// WHY: the reply carries the inbound `account_id` so multi-account
/// deployments answer from the account that received the message; only an
/// unattributed inbound message falls back to the provider default account.
async fn send_reply(
    msg: &InboundMessage,
    text: &str,
    purpose: ReplyPurpose,
    channel_registry: &ChannelRegistry,
) -> ReplyDelivery {
    let to = reply_target(msg);
    // WHY: the idempotency key is derived from the inbound message
    // identity, so a replayed inbound event retries the reply under the
    // same provider-level key instead of posting a duplicate (Matrix
    // honors this via its transaction ID; providers without idempotent
    // sends ignore it).
    let params = SendParams {
        to,
        // SECURITY(#5193): command responses can contain values read from
        // operational stores (notably the blackboard). Apply the fleet's
        // central secret-pattern redactor at the final command-egress boundary;
        // raw runtime values never reach a provider send request.
        text: match purpose {
            ReplyPurpose::Command => koina::redact::redact_sensitive(text),
            ReplyPurpose::Turn => text.to_owned(),
        },
        account_id: msg.account_id.clone(),
        sender_id: None,
        idempotency_key: Some(reply_idempotency_key(msg, purpose)),
        thread_id: None,
        attachments: None,
    };

    match channel_registry.send(&msg.channel, &params).await {
        Ok(result) if result.sent => ReplyDelivery::Sent,
        Ok(_) => {
            // SECURITY: provider-controlled error text is deliberately not
            // inspected, logged, or persisted.
            warn!("provider failed to send channel reply");
            ReplyDelivery::Failed(ReplyFailure::Provider)
        }
        Err(_) => {
            warn!("channel registry failed to route reply");
            ReplyDelivery::Failed(ReplyFailure::Registry)
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions index after length checks"
)]
mod tests {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::Arc;

    #[cfg(feature = "recall")]
    use std::collections::HashMap;

    use agora::types::{ChannelCapabilities, ChannelProvider, ProbeResult, SendResult};
    use hermeneus::provider::ProviderRegistry;
    use hermeneus::test_utils::MockProvider;
    use mneme::store::SessionStore;
    use mneme::types::{BlackboardVisibility, COMMAND_LIFECYCLE_SCHEMA, CommandLifecycleRecord};
    use nous::adapters::SessionBlackboardAdapter;
    use nous::config::{NousConfig, NousGenerationConfig, PipelineConfig};
    use nous::manager::NousManager;
    use organon::registry::ToolRegistry;
    use organon::types::{BlackboardStore, ToolHttpClients, ToolServices};
    use taxis::config::ChannelBinding;
    use taxis::oikos::Oikos;
    use tokio::sync::{Mutex, mpsc};
    use tokio::task::JoinSet;

    use super::*;

    #[expect(
        clippy::disallowed_methods,
        reason = "test setup writes temp files synchronously"
    )]
    fn make_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("nous/alice")).expect("create alice workspace");
        std::fs::create_dir_all(root.join("shared")).expect("create shared");
        std::fs::create_dir_all(root.join("theke")).expect("create theke");
        std::fs::write(root.join("nous/alice/SOUL.md"), "I am Alice.").expect("write soul");
        (dir, Arc::new(Oikos::from_root(&root)))
    }

    fn make_providers() -> Arc<ProviderRegistry> {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(
            MockProvider::new("Hello!").models(&["test-model"]),
        ));
        Arc::new(providers)
    }

    fn make_tool_services(session_store: &Arc<Mutex<SessionStore>>) -> Arc<ToolServices> {
        let blackboard_store: Arc<dyn BlackboardStore> =
            Arc::new(SessionBlackboardAdapter(Arc::clone(session_store)));
        Arc::new(ToolServices {
            cross_nous: None,
            messenger: None,
            note_store: None,
            blackboard_store: Some(blackboard_store),
            spawn: None,
            planning: None,
            knowledge: None,
            working_checkpoint_store: None,
            http_clients: ToolHttpClients {
                general: reqwest::Client::new(),
                ssrf_safe: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            },
            secret_vault: hermeneus::secret::SecretVault::new(),
            lazy_tool_catalog: Vec::new(),
            server_tool_config: organon::types::ServerToolConfig::default(),
        })
    }

    fn make_config() -> NousConfig {
        NousConfig {
            id: Arc::from("alice"),
            generation: NousGenerationConfig {
                model: "test-model".to_owned(),
                ..NousGenerationConfig::default()
            },
            workspace: PathBuf::from("nous/alice"),
            ..NousConfig::default()
        }
    }

    static RECORDING_CAPS: ChannelCapabilities = ChannelCapabilities {
        threads: false,
        reactions: false,
        typing: false,
        media: false,
        streaming: false,
        rich_formatting: false,
        max_text_length: 2000,
    };

    struct RecordingChannel {
        sent: Arc<Mutex<Vec<SendParams>>>,
        send_result: SendResult,
    }

    impl RecordingChannel {
        fn new(sent: Arc<Mutex<Vec<SendParams>>>, send_result: SendResult) -> Self {
            Self { sent, send_result }
        }
    }

    impl ChannelProvider for RecordingChannel {
        fn id(&self) -> &'static str {
            "signal"
        }

        fn name(&self) -> &'static str {
            "Signal"
        }

        fn capabilities(&self) -> &ChannelCapabilities {
            &RECORDING_CAPS
        }

        fn send<'a>(
            &'a self,
            params: &'a SendParams,
        ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
            Box::pin(async move {
                self.sent.lock().await.push(params.clone());
                self.send_result.clone()
            })
        }

        fn listen(
            &self,
            _poll_interval: Option<std::time::Duration>,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
            let (_tx, rx) = mpsc::channel(1);
            (rx, JoinSet::new())
        }

        fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
            Box::pin(async {
                ProbeResult {
                    ok: true,
                    latency_ms: Some(1),
                    error: None,
                    details: None,
                }
            })
        }
    }

    struct DispatchHarness {
        _dir: tempfile::TempDir,
        nous_manager: Arc<NousManager>,
        router: Arc<MessageRouter>,
        channel_registry: Arc<ChannelRegistry>,
        session_store: Arc<Mutex<SessionStore>>,
        sent: Arc<Mutex<Vec<SendParams>>>,
        command_policy: InboundCommandPolicy,
        dedupe: Arc<std::sync::Mutex<DedupeFilter>>,
    }

    async fn make_dispatch_harness() -> DispatchHarness {
        make_dispatch_harness_with_send_result(SendResult::ok()).await
    }

    async fn make_dispatch_harness_with_send_result(send_result: SendResult) -> DispatchHarness {
        let (dir, oikos) = make_oikos();
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory session store"),
        ));
        let mut mgr = make_dispatch_manager(oikos, None);
        mgr.spawn(make_config(), PipelineConfig::default())
            .await
            .expect("spawn alice");
        let nous_manager = Arc::new(mgr);
        let router = Arc::new(MessageRouter::new(
            vec![ChannelBinding {
                channel: "signal".to_owned(),
                source: "*".to_owned(),
                nous_id: "alice".to_owned(),
                session_key: "signal:{source}".to_owned(),
                account: None,
                participants: vec![],
                command_tier: taxis::config::CommandTier::Public,
            }],
            None,
        ));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let provider: Arc<dyn ChannelProvider> =
            Arc::new(RecordingChannel::new(Arc::clone(&sent), send_result));
        let mut channel_registry = ChannelRegistry::new();
        channel_registry
            .register(provider)
            .expect("register channel");
        DispatchHarness {
            _dir: dir,
            nous_manager,
            router,
            channel_registry: Arc::new(channel_registry),
            session_store,
            sent,
            command_policy: InboundCommandPolicy::default(),
            dedupe: Arc::new(std::sync::Mutex::new(DedupeFilter::with_capacity(16))),
        }
    }

    async fn shutdown_harness(harness: DispatchHarness) {
        drop(harness.router);
        drop(harness.channel_registry);
        drop(harness.session_store);
        drop(harness.sent);
        match Arc::try_unwrap(harness.nous_manager) {
            Ok(mut mgr) => mgr.shutdown_all().await,
            Err(remaining) => panic!(
                "manager still has {} references",
                Arc::strong_count(&remaining)
            ),
        }
    }

    /// All command names visible, as an operator-tier sender sees them.
    fn all_visible() -> Vec<String> {
        command::known_command_names()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect()
    }

    fn command_message(text: &str, timestamp: u64) -> InboundMessage {
        InboundMessage {
            channel: "signal".to_owned(),
            sender: "+15550100".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: None,
            account_id: None,
            message_id: None,
            text: text.to_owned(),
            timestamp,
            attachments: vec![],
            raw: None,
        }
    }

    fn configure_operator_route(harness: &mut DispatchHarness, account_id: &str) {
        harness.router = Arc::new(MessageRouter::new(
            vec![ChannelBinding {
                channel: "signal".to_owned(),
                source: "+15550100".to_owned(),
                nous_id: "alice".to_owned(),
                session_key: "signal:{source}".to_owned(),
                account: Some(account_id.to_owned()),
                participants: vec![],
                command_tier: CommandTier::Operator,
            }],
            None,
        ));
    }

    async fn command_records(
        harness: &DispatchHarness,
        account_id: Option<&str>,
    ) -> Vec<CommandLifecycleRecord> {
        let store = harness.session_store.lock().await;
        let session_key = normalize_session_key("signal", account_id, "signal:+15550100")
            .expect("routed session key is valid");
        let session = store
            .find_session("alice", &session_key)
            .expect("find session")
            .expect("session exists");
        store
            .command_lifecycle_records_for_session(&session.id)
            .expect("command lifecycle records")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn public_ping_carries_inbound_account_without_persisting_state() {
        let harness = make_dispatch_harness().await;
        let mut msg = command_message("!ping", 1_709_312_345_700);
        msg.account_id = Some("work".to_owned());

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert_eq!(
                sent[0].account_id.as_deref(),
                Some("work"),
                "reply must leave from the account that received the message"
            );
        }

        let store = harness.session_store.lock().await;
        let session_key = normalize_session_key("signal", Some("work"), "signal:+15550100")
            .expect("routed session key is valid");
        assert!(
            store
                .find_session("alice", &session_key)
                .expect("session lookup")
                .is_none(),
            "public ping must not create a session or lifecycle record"
        );
        drop(store);

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatch_records_successful_typed_command_lifecycle() {
        let mut harness = make_dispatch_harness().await;
        configure_operator_route(&mut harness, "primary");
        let mut msg = command_message("!uptime", 1_709_312_345_678);
        msg.account_id = Some("primary".to_owned());

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert!(sent[0].text.contains("uptime"), "{:?}", sent[0].text);
        }

        let records = command_records(&harness, Some("primary")).await;
        assert_eq!(records.len(), 2);
        let invocation = &records[0];
        let result = &records[1];
        assert_eq!(invocation.schema, COMMAND_LIFECYCLE_SCHEMA);
        assert!(matches!(
            invocation.event,
            CommandLifecycleEvent::Invocation {
                status: CommandInvocationStatus::Started
            }
        ));
        assert_eq!(invocation.command.name, "uptime");
        assert_eq!(invocation.origin.channel, "signal");
        let expected_sender =
            agora::redact::opaque_identifier("command-origin-sender:signal", "+15550100");
        assert_eq!(
            invocation.origin.sender, expected_sender,
            "command audit must carry an opaque sender handle"
        );
        assert!(invocation.origin.conversation_id.starts_with("sha256:"));
        assert_eq!(invocation.delivery_key, result.delivery_key);
        let CommandLifecycleEvent::Result {
            status,
            failure_class,
            duration_ms: _,
            delivery,
        } = &result.event
        else {
            panic!("second lifecycle row must be a result: {result:?}");
        };
        assert_eq!(*status, CommandResultStatus::Succeeded);
        assert_eq!(*failure_class, None);
        assert_eq!(delivery.status, CommandDeliveryStatus::Sent);
        assert_eq!(delivery.failure_class, None);
        let serialized = serde_json::to_string(&records).expect("serialize records");
        assert!(
            !serialized.contains("+15550100"),
            "raw sender must not persist in command lifecycle records: {serialized}"
        );
        let store = harness.session_store.lock().await;
        assert!(
            store
                .get_history(&invocation.session_id, None)
                .expect("conversation history")
                .is_empty(),
            "command lifecycle rows must stay out of conversation history"
        );
        drop(store);

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn provider_failure_persists_only_stable_failure_class() {
        let sentinel = "provider-sentinel-secret";
        let mut harness = make_dispatch_harness_with_send_result(SendResult::err(sentinel)).await;
        configure_operator_route(&mut harness, "primary");
        let mut msg = command_message("!uptime", 1_709_312_345_675);
        msg.account_id = Some("primary".to_owned());

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        let records = command_records(&harness, Some("primary")).await;
        let CommandLifecycleEvent::Result { delivery, .. } = &records[1].event else {
            panic!("second lifecycle row must be a result");
        };
        assert_eq!(delivery.status, CommandDeliveryStatus::Failed);
        assert_eq!(
            delivery.failure_class,
            Some(CommandDeliveryFailureClass::ProviderFailure)
        );
        let serialized = serde_json::to_string(&records).expect("serialize records");
        assert!(
            !serialized.contains(sentinel),
            "provider-controlled detail must not enter durable records"
        );

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn registry_failure_persists_only_stable_failure_class() {
        let mut harness = make_dispatch_harness().await;
        configure_operator_route(&mut harness, "primary");
        harness.channel_registry = Arc::new(ChannelRegistry::new());
        let mut msg = command_message("!uptime", 1_709_312_345_676);
        msg.account_id = Some("primary".to_owned());

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        let records = command_records(&harness, Some("primary")).await;
        let CommandLifecycleEvent::Result { delivery, .. } = &records[1].event else {
            panic!("second lifecycle row must be a result");
        };
        assert_eq!(delivery.status, CommandDeliveryStatus::Failed);
        assert_eq!(
            delivery.failure_class,
            Some(CommandDeliveryFailureClass::RegistryFailure)
        );

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_command_is_fixed_and_not_audited() {
        let harness = make_dispatch_harness().await;
        let msg = command_message("!frobnicate --token secret-value target", 1_709_312_345_679);

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].text, UNKNOWN_COMMAND_REPLY);
            assert!(!sent[0].text.contains("frobnicate"));
            assert!(!sent[0].text.contains("secret-value"));
        }

        let store = harness.session_store.lock().await;
        let session_key = normalize_session_key("signal", None, "signal:+15550100")
            .expect("routed session key is valid");
        assert!(
            store
                .find_session("alice", &session_key)
                .expect("lookup")
                .is_none(),
            "unknown input must not create a session or audit record"
        );
        drop(store);

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatch_deduplicates_retried_command_delivery() {
        let mut harness = make_dispatch_harness().await;
        configure_operator_route(&mut harness, "primary");
        let mut msg = command_message("!uptime", 1_709_312_345_680);
        msg.account_id = Some("primary".to_owned());

        dispatch_one(
            msg.clone(),
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;
        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            // WHY: the dispatcher-level DedupeFilter drops the identical
            // redelivery before the command-record idempotency path, so only
            // one reply is sent.
            assert_eq!(sent.len(), 1);
        }

        let records = command_records(&harness, Some("primary")).await;
        assert_eq!(records.len(), 2, "duplicate must not append records");
        assert!(matches!(
            records[0].event,
            CommandLifecycleEvent::Invocation { .. }
        ));
        assert!(matches!(
            records[1].event,
            CommandLifecycleEvent::Result { .. }
        ));

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatch_deduplicates_plain_turn_redelivery() {
        let harness = make_dispatch_harness().await;
        let msg = command_message("a plain conversational turn", 1_709_312_345_690);

        for _ in 0..3 {
            dispatch_one(
                msg.clone(),
                Arc::clone(&harness.router),
                Arc::clone(&harness.nous_manager),
                Arc::clone(&harness.channel_registry),
                Arc::clone(&harness.session_store),
                harness.command_policy.clone(),
                Arc::clone(&harness.dedupe),
            )
            .await;
        }

        let sent = harness.sent.lock().await;
        assert_eq!(
            sent.len(),
            1,
            "three identical deliveries must run exactly one agent turn"
        );
        drop(sent);

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatch_dedupe_uses_provider_message_id_when_present() {
        let harness = make_dispatch_harness().await;
        let mut first = command_message("!ping", 1_709_312_345_691);
        first.message_id = Some("$event1".to_owned());
        let mut second = command_message("!ping", 1_709_312_345_692);
        second.message_id = Some("$event1".to_owned());

        for msg in [first, second] {
            dispatch_one(
                msg,
                Arc::clone(&harness.router),
                Arc::clone(&harness.nous_manager),
                Arc::clone(&harness.channel_registry),
                Arc::clone(&harness.session_store),
                harness.command_policy.clone(),
                Arc::clone(&harness.dedupe),
            )
            .await;
        }

        let sent = harness.sent.lock().await;
        assert_eq!(
            sent.len(),
            1,
            "same provider message ID must dedupe even when timestamps differ"
        );
        drop(sent);

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn wildcard_route_cannot_run_operator_commands_without_policy_grant() {
        let harness = make_dispatch_harness().await;
        let msg = command_message("!agents", 1_709_312_345_700);

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].text, UNKNOWN_COMMAND_REPLY);
            assert!(!sent[0].text.contains("agent(s) running"));
        }

        let store = harness.session_store.lock().await;
        let session_key = normalize_session_key("signal", None, "signal:+15550100")
            .expect("routed session key is valid");
        assert!(
            store
                .find_session("alice", &session_key)
                .expect("lookup")
                .is_none(),
            "a denied command must not create an audit session"
        );
        drop(store);

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn exact_account_route_runs_operator_commands() {
        let mut harness = make_dispatch_harness().await;
        configure_operator_route(&mut harness, "primary");
        let mut msg = command_message("!uptime", 1_709_312_345_701);
        msg.account_id = Some("primary".to_owned());

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert!(
                sent[0].text.contains("uptime"),
                "allowlisted operator command must execute: {:?}",
                sent[0].text
            );
        }

        let records = command_records(&harness, Some("primary")).await;
        assert_eq!(records.len(), 2, "executed command is audited");

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn public_help_hides_operator_command_surface() {
        let harness = make_dispatch_harness().await;
        let msg = command_message("!help", 1_709_312_345_702);

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert!(sent[0].text.contains("!ping"), "{:?}", sent[0].text);
            assert!(sent[0].text.contains("!help"), "{:?}", sent[0].text);
            assert!(
                !sent[0].text.contains("!agents"),
                "public help must not enumerate operator commands: {:?}",
                sent[0].text
            );
            assert!(!sent[0].text.contains("!blackboard"), "{:?}", sent[0].text);
        }

        let store = harness.session_store.lock().await;
        let session_key = normalize_session_key("signal", None, "signal:+15550100")
            .expect("routed session key is valid");
        assert!(
            store
                .find_session("alice", &session_key)
                .expect("session lookup")
                .is_none(),
            "public help must not create a session or lifecycle record"
        );
        drop(store);

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_commands_get_fixed_non_echoing_reply() {
        let harness = make_dispatch_harness().await;
        let msg = command_message("!frobnicate", 1_709_312_345_703);

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            harness.command_policy.clone(),
            Arc::clone(&harness.dedupe),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].text, UNKNOWN_COMMAND_REPLY);
        }

        shutdown_harness(harness).await;
    }

    #[cfg(feature = "recall")]
    fn make_dispatch_manager(
        oikos: Arc<Oikos>,
        tool_services: Option<Arc<ToolServices>>,
    ) -> NousManager {
        use mneme::knowledge_store::KnowledgeStore;

        let mut knowledge_stores = HashMap::new();
        knowledge_stores.insert(
            "shared".to_owned(),
            KnowledgeStore::open_mem().expect("open in-memory knowledge store"),
        );

        NousManager::new(
            make_providers(),
            Arc::new(ToolRegistry::new()),
            oikos,
            None,
            None,
            None,
            Some(knowledge_stores),
            Arc::new(Vec::new()),
            None,
            tool_services,
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        )
    }

    #[cfg(not(feature = "recall"))]
    fn make_dispatch_manager(
        oikos: Arc<Oikos>,
        tool_services: Option<Arc<ToolServices>>,
    ) -> NousManager {
        NousManager::new(
            make_providers(),
            Arc::new(ToolRegistry::new()),
            oikos,
            None,
            None,
            None,
            Arc::new(Vec::new()),
            None,
            tool_services,
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        )
    }

    #[cfg(feature = "recall")]
    fn make_skill_manager(
        oikos: Arc<Oikos>,
        knowledge_stores: HashMap<String, Arc<mneme::knowledge_store::KnowledgeStore>>,
    ) -> NousManager {
        NousManager::new(
            make_providers(),
            Arc::new(ToolRegistry::new()),
            oikos,
            None,
            None,
            None,
            Some(knowledge_stores),
            Arc::new(Vec::new()),
            None,
            None,
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        )
    }

    #[cfg(feature = "recall")]
    fn make_skill_fact(skill_name: &str) -> mneme::knowledge::Fact {
        use mneme::knowledge::{
            EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
            FactTemporal, Visibility, far_future,
        };

        let content = serde_json::to_string(&mneme::skill::SkillContent {
            name: skill_name.to_owned(),
            description: "Send a signal reply".to_owned(),
            steps: vec!["do the thing".to_owned()],
            tools_used: vec!["signal".to_owned()],
            domain_tags: vec!["communication".to_owned()],
            origin: "seeded".to_owned(),
            triggers: vec![],
            always: false,
        })
        .expect("skill content serializes");

        Fact {
            id: mneme::id::FactId::new("skill-alice-signal").expect("valid fact id"),
            nous_id: "alice".to_owned(),
            fact_type: "skill".to_owned(),
            content,
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: jiff::Timestamp::from_second(1_700_000_000).expect("valid timestamp"),
                valid_to: far_future(),
                recorded_at: jiff::Timestamp::from_second(1_700_000_100).expect("valid timestamp"),
            },
            provenance: FactProvenance {
                confidence: 0.9,
                tier: EpistemicTier::Verified,
                source_session_id: None,
                stability_hours: 24.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[cfg(feature = "recall")]
    async fn skills_command_uses_seeded_knowledge_store() {
        let (_dir, oikos) = make_oikos();
        let mut knowledge_stores = HashMap::new();
        let store = mneme::knowledge_store::KnowledgeStore::open_mem()
            .expect("open in-memory knowledge store");
        let skill_fact = make_skill_fact("signal-send");
        store.insert_fact(&skill_fact).expect("insert skill fact");
        knowledge_stores.insert("shared".to_owned(), store);

        let mut mgr = make_skill_manager(oikos, knowledge_stores);
        let _handle = mgr
            .spawn(make_config(), PipelineConfig::default())
            .await
            .expect("spawn alice");

        let reply = execute_command(
            &command::Command::Skills,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
            all_visible(),
        )
        .await;

        assert!(reply.contains("signal-send"), "{reply}");
        assert!(!reply.contains("No skills available"), "{reply}");

        mgr.shutdown_all().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn blackboard_command_uses_session_adapter() {
        organon::testing::install_crypto_provider();
        let (_dir, oikos) = make_oikos();
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory session store"),
        ));
        let tool_services = make_tool_services(&session_store);
        let mgr = make_dispatch_manager(oikos, Some(tool_services));

        let blackboard_store = mgr.blackboard_store().expect("blackboard store");
        blackboard_store
            .write("goal", "finish the demo", "alice", 3600)
            .expect("write blackboard entry");

        let reply = execute_command(
            &command::Command::Blackboard,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
            all_visible(),
        )
        .await;

        assert!(
            reply.contains("[goal] = finish the demo (by alice)"),
            "{reply}"
        );
        assert!(!reply.contains("Blackboard empty"), "{reply}");
    }

    /// Pins aletheia#5032 closed for the `!blackboard` chat command: it must
    /// not surface a `ws:`-style `SessionPrivate` row from another session,
    /// nor another agent's `NousPrivate` row, even though it only has a
    /// nous-scoped viewer (no session UUID at this layer).
    #[tokio::test(flavor = "multi_thread")]
    async fn blackboard_command_excludes_private_rows() {
        organon::testing::install_crypto_provider();
        let (_dir, oikos) = make_oikos();
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory session store"),
        ));
        {
            let store = session_store.lock().await;
            store
                .blackboard_write("goal", "finish the demo", "alice", 3600)
                .expect("write shared entry");
            store
                .blackboard_write_scoped(
                    "ws:alice:some-other-session",
                    "leaked-task-stack",
                    "alice",
                    3600,
                    BlackboardVisibility::SessionPrivate,
                    Some("some-other-session"),
                )
                .expect("write session-private entry");
            store
                .blackboard_write_scoped(
                    "bobs-secret",
                    "leaked-private-note",
                    "bob",
                    3600,
                    BlackboardVisibility::NousPrivate,
                    None,
                )
                .expect("write nous-private entry");
        }
        let tool_services = make_tool_services(&session_store);
        let mgr = make_dispatch_manager(oikos, Some(tool_services));

        let reply = execute_command(
            &command::Command::Blackboard,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
            all_visible(),
        )
        .await;

        assert!(
            reply.contains("[goal] = finish the demo (by alice)"),
            "shared entries must still show: {reply}"
        );
        assert!(
            !reply.contains("leaked-task-stack"),
            "a SessionPrivate row from another session must not appear: {reply}"
        );
        assert!(
            !reply.contains("leaked-private-note"),
            "another agent's NousPrivate row must not appear: {reply}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_state_falls_back_without_stores() {
        let (_dir, oikos) = make_oikos();
        let mgr = make_dispatch_manager(oikos, None);

        let skills_reply = execute_command(
            &command::Command::Skills,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
            all_visible(),
        )
        .await;
        assert!(
            skills_reply.contains("No skills available"),
            "{skills_reply}"
        );

        let blackboard_reply = execute_command(
            &command::Command::Blackboard,
            "alice",
            "main",
            &mgr,
            &ChannelRegistry::new(),
            all_visible(),
        )
        .await;
        assert!(
            blackboard_reply.contains("Blackboard empty"),
            "{blackboard_reply}"
        );
    }

    #[test]
    fn normalize_session_key_hashes_phone_numbers() {
        let key = normalize_session_key("signal", None, "signal:+15550100")
            .expect("valid phone session key");
        assert!(key.starts_with("h:"), "hashed keys are prefixed: {key}");
        assert!(
            !key.contains("+15550100"),
            "raw phone number must be redacted: {key}"
        );
        assert_eq!(
            key,
            normalize_session_key("signal", None, "signal:+15550100").expect("stable")
        );
    }

    #[test]
    fn command_delivery_key_is_scoped_by_session_and_canonical_command() {
        let msg = command_message("!ping", 1_709_312_345_600);
        let first = command_delivery_key(&msg, "session-a", "ping");
        assert_eq!(
            first,
            command_delivery_key(&msg, "session-a", "ping"),
            "the same canonical command identity must be stable"
        );
        assert_ne!(first, command_delivery_key(&msg, "session-b", "ping"));
        assert_ne!(first, command_delivery_key(&msg, "session-a", "help"));
    }

    #[test]
    fn reply_idempotency_is_scoped_by_reply_purpose() {
        let msg = command_message("!ping", 1_709_312_345_601);
        let command = reply_idempotency_key(&msg, ReplyPurpose::Command);
        assert_eq!(
            command,
            reply_idempotency_key(&msg, ReplyPurpose::Command),
            "command replay must reuse its original provider key"
        );
        assert_ne!(command, reply_idempotency_key(&msg, ReplyPurpose::Turn));
    }

    #[test]
    fn normalize_session_key_hashes_matrix_ids() {
        let key = normalize_session_key("matrix", None, "matrix:@alice:example.org")
            .expect("valid matrix session key");
        assert!(
            !key.contains("@alice:example.org"),
            "raw matrix id must be redacted: {key}"
        );
    }

    #[test]
    fn normalize_session_key_is_scoped_by_channel_and_account() {
        let expanded = "shared-template-output";
        let signal_primary =
            normalize_session_key("signal", Some("primary"), expanded).expect("valid");
        let signal_secondary =
            normalize_session_key("signal", Some("secondary"), expanded).expect("valid");
        let matrix_primary =
            normalize_session_key("matrix", Some("primary"), expanded).expect("valid");

        assert_ne!(signal_primary, signal_secondary);
        assert_ne!(signal_primary, matrix_primary);
        assert_ne!(
            normalize_session_key("signal", None, expanded).expect("valid"),
            normalize_session_key("signal", Some(""), expanded).expect("valid"),
            "an absent account must not alias an explicitly empty account"
        );
    }

    #[test]
    fn normalize_session_key_hashes_group_ids() {
        let key = normalize_session_key("signal", None, "signal:group-abc!xyz")
            .expect("valid group session key");
        assert!(
            !key.contains("group-abc"),
            "raw group id must be redacted: {key}"
        );
    }

    #[test]
    fn normalize_session_key_hashes_path_like_values() {
        let key = normalize_session_key("webhook", None, "webhook:/api/v1/incoming/abc")
            .expect("valid path-like session key");
        assert!(!key.contains("/api/v1"), "raw path must be redacted: {key}");
    }

    #[test]
    fn normalize_session_key_rejects_empty() {
        assert!(normalize_session_key("signal", None, "").is_err());
    }

    #[test]
    fn normalize_session_key_rejects_oversized_keys() {
        let oversized = "x".repeat(MAX_SESSION_KEY_LEN + 1);
        assert!(
            normalize_session_key("signal", None, &oversized).is_err(),
            "keys over {MAX_SESSION_KEY_LEN} bytes must be rejected"
        );

        let at_limit = "y".repeat(MAX_SESSION_KEY_LEN);
        assert!(
            normalize_session_key("signal", None, &at_limit).is_ok(),
            "keys at the limit must be accepted"
        );
    }

    #[test]
    fn normalize_session_key_rejects_control_characters() {
        assert!(normalize_session_key("signal", None, "signal:\0sender").is_err());
        assert!(normalize_session_key("signal", None, "signal:\nsender").is_err());
    }
}
