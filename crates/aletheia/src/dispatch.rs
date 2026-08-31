// kanon:ignore RUST/file-too-long WHY: inbound dispatch, command auditing, and dispatch tests share private helpers in one module
//! Background dispatch loop: routes inbound messages to nous actors.

use std::sync::Arc;
use std::time::Instant;

use mneme::store::SessionStore;
use mneme::types::Role;
use sha2::{Digest as _, Sha256};
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{Instrument, debug, info, warn};

use agora::command::{self, AgentSnapshot, ChannelSnapshot, CommandContext};
use agora::dedupe::{DEFAULT_DEDUPE_CAPACITY, DedupeFilter};
use agora::registry::ChannelRegistry;
use agora::router::{MessageRouter, reply_target};
use agora::types::{InboundMessage, SendParams};
use koina::redact::redact_channel_id;
use nous::manager::NousManager;
use organon::types::BlackboardViewer;

const COMMAND_RECORD_SCHEMA: &str = "aletheia.agora.command.v1";

/// Spawn a background task that dispatches inbound messages to nous actors.
///
/// Runs until the receiver channel closes (all senders dropped).
/// Per-message dispatch tasks are tracked in a `JoinSet` and drained on exit.
pub(crate) fn spawn_dispatcher(
    mut rx: mpsc::Receiver<InboundMessage>,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
    session_store: Arc<tokio::sync::Mutex<SessionStore>>,
    mut ready_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let span = tracing::info_span!("message_dispatcher");
    tokio::spawn(
        async move {
            while !*ready_rx.borrow_and_update() {
                if ready_rx.changed().await.is_err() {
                    warn!("ready channel dropped before ready signal");
                    return;
                }
            }
            info!("dispatch loop started");

            let mut in_flight = JoinSet::new();
            // WHY(#7102): providers redeliver on reconnects, retries, and
            // cursor resets (at-least-once transports recover that way), and
            // nothing downstream dedupes -- unchecked, a redelivery runs the
            // same turn twice, billing a second model call and posting a
            // duplicate reply. Consulted here so a duplicate is dropped
            // before it becomes work, whatever shape the loop itself takes
            // (#7103).
            let mut dedupe = DedupeFilter::with_capacity(DEFAULT_DEDUPE_CAPACITY);

            while let Some(msg) = rx.recv().await {
                if !dedupe.check_and_record(&msg) {
                    debug!(
                        channel = %msg.channel,
                        sender = %redact_channel_id(&msg.sender),
                        "dropping redelivered inbound message"
                    );
                    continue;
                }
                let router = Arc::clone(&router);
                let nous_mgr = Arc::clone(&nous_manager);
                let channels = Arc::clone(&channel_registry);
                let session_store = Arc::clone(&session_store);
                let msg_span = tracing::info_span!(
                    "dispatch",
                    channel = %msg.channel,
                    sender = %redact_channel_id(&msg.sender),
                );
                in_flight.spawn(
                    dispatch_one(msg, router, nous_mgr, channels, session_store)
                        .instrument(msg_span),
                );

                // WHY: Reap completed tasks periodically to prevent unbounded growth.
                while let Some(result) = in_flight.try_join_next() {
                    if let Err(e) = result {
                        warn!(error = %e, "dispatch task panicked");
                    }
                }
            }

            // WHY: Drain remaining in-flight dispatch tasks before exiting.
            info!(
                remaining = in_flight.len(),
                "dispatch loop draining in-flight tasks"
            );
            while let Some(result) = in_flight.join_next().await {
                if let Err(e) = result {
                    warn!(error = %e, "dispatch task panicked during drain");
                }
            }

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
) {
    let Some(decision) = router.resolve(&msg) else {
        warn!(
            channel = %msg.channel,
            sender = %redact_channel_id(&msg.sender),
            "no route for inbound message, dropping"
        );
        return;
    };

    // WHY: Session keys are built from external identifiers by the router's
    // template expansion. Normalize before any use so logs, command records,
    // and store lookups never carry raw phone numbers, Matrix IDs, or group IDs.
    let session_key = match normalize_session_key(&decision.session_key) {
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
    if let Some(cmd) = command::parse(&msg.text) {
        handle_command_dispatch(CommandDispatch {
            msg: &msg,
            cmd: &cmd,
            nous_id: decision.nous_id,
            session_key: &session_key,
            nous_manager: &nous_manager,
            channel_registry: &channel_registry,
            session_store: &session_store,
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

    send_reply(&msg, &turn_result.content, &channel_registry).await;
}

struct CommandDispatch<'a> {
    msg: &'a InboundMessage,
    cmd: &'a command::Command,
    nous_id: &'a str,
    session_key: &'a str,
    nous_manager: &'a NousManager,
    channel_registry: &'a ChannelRegistry,
    session_store: &'a Arc<tokio::sync::Mutex<SessionStore>>,
}

struct StartedCommandRecord {
    session_id: String,
    idempotency_key: String,
}

enum CommandRecordStart {
    Started {
        session_id: String,
        idempotency_key: String,
    },
    Duplicate {
        reply_text: String,
    },
}

struct ReplyDelivery {
    status: &'static str,
    error: Option<String>,
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
        Ok(CommandRecordStart::Started {
            session_id,
            idempotency_key,
        }) => StartedCommandRecord {
            session_id,
            idempotency_key,
        },
        Ok(CommandRecordStart::Duplicate { reply_text }) => {
            debug!(
                nous_id = %dispatch.nous_id,
                command = dispatch.cmd.name(),
                "duplicate !-command delivery ignored"
            );
            send_reply(dispatch.msg, &reply_text, dispatch.channel_registry).await;
            return;
        }
        Err(e) => {
            warn!(
                error = %e,
                nous_id = %dispatch.nous_id,
                command = dispatch.cmd.name(),
                "failed to record !-command invocation"
            );
            let reply_text = format!(
                "Command '!{}' was not executed because its session record could not be written.",
                dispatch.cmd.name()
            );
            send_reply(dispatch.msg, &reply_text, dispatch.channel_registry).await;
            return;
        }
    };
    let reply_text = execute_command(
        dispatch.cmd,
        dispatch.nous_id,
        dispatch.session_key,
        dispatch.nous_manager,
        dispatch.channel_registry,
    )
    .await;
    let delivery = send_reply(dispatch.msg, &reply_text, dispatch.channel_registry).await;
    if let Err(e) = finish_command_record(
        dispatch.session_store,
        &command_record,
        dispatch.msg,
        dispatch.cmd,
        dispatch.nous_id,
        dispatch.session_key,
        &reply_text,
        command_failure_class(dispatch.cmd),
        started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
        &delivery,
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
) -> Result<CommandRecordStart, mneme::error::Error> {
    let idempotency_key = command_idempotency_key(msg, session_key);
    let session_id = koina::id::SessionId::new().to_string();
    let store = session_store.lock().await;
    let session = store.find_or_create_session(&session_id, nous_id, session_key, model, None)?;
    let history = store.get_history(&session.id, None)?;
    if let Some(reply_text) = duplicate_command_reply(&history, &idempotency_key, cmd.name()) {
        return Ok(CommandRecordStart::Duplicate { reply_text });
    }

    let content =
        command_invocation_record(msg, cmd, nous_id, session_key, &idempotency_key).to_string();
    store.append_message(
        &session.id,
        Role::System,
        &content,
        None,
        None,
        token_estimate(&content),
    )?;

    Ok(CommandRecordStart::Started {
        session_id: session.id,
        idempotency_key,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "result record mirrors the audit payload shape"
)]
async fn finish_command_record(
    session_store: &Arc<tokio::sync::Mutex<SessionStore>>,
    record: &StartedCommandRecord,
    msg: &InboundMessage,
    cmd: &command::Command,
    nous_id: &str,
    session_key: &str,
    reply_text: &str,
    failure_class: Option<&str>,
    duration_ms: u64,
    delivery: &ReplyDelivery,
) -> Result<(), mneme::error::Error> {
    let content = command_result_record(
        msg,
        cmd,
        nous_id,
        session_key,
        &record.idempotency_key,
        reply_text,
        failure_class,
        duration_ms,
        delivery,
    )
    .to_string();
    let store = session_store.lock().await;
    store.append_message(
        &record.session_id,
        Role::System,
        &content,
        None,
        None,
        token_estimate(&content),
    )?;
    Ok(())
}

fn duplicate_command_reply(
    history: &[mneme::types::Message],
    idempotency_key: &str,
    command_name: &str,
) -> Option<String> {
    let mut pending = false;
    for message in history.iter().rev() {
        if message.role != Role::System {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content) else {
            continue;
        };
        if value.get("schema").and_then(serde_json::Value::as_str) != Some(COMMAND_RECORD_SCHEMA)
            || value
                .get("idempotency_key")
                .and_then(serde_json::Value::as_str)
                != Some(idempotency_key)
        {
            continue;
        }
        match value.get("event").and_then(serde_json::Value::as_str) {
            Some("result") => {
                let reply_text = value
                    .pointer("/response/reply_text")
                    .and_then(serde_json::Value::as_str)
                    .map_or_else(
                        || format!("Command '!{command_name}' was already handled."),
                        ToOwned::to_owned,
                    );
                return Some(reply_text);
            }
            Some("invocation") => pending = true,
            _ => {} // kanon:ignore RUST/empty-match-arm WHY: unrelated system records do not affect command duplicate detection
        }
    }

    pending.then(|| {
        format!("Command '!{command_name}' is already in progress; duplicate delivery ignored.")
    })
}

fn command_invocation_record(
    msg: &InboundMessage,
    cmd: &command::Command,
    nous_id: &str,
    session_key: &str,
    idempotency_key: &str,
) -> serde_json::Value {
    serde_json::json!({
        "schema": COMMAND_RECORD_SCHEMA,
        "event": "invocation",
        "idempotency_key": idempotency_key,
        "nous_id": nous_id,
        "session_key": session_key,
        "origin": command_origin_record(msg),
        "command": command_record_command(cmd),
        "status": "started",
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "result record mirrors the audit payload shape"
)]
fn command_result_record(
    msg: &InboundMessage,
    cmd: &command::Command,
    nous_id: &str,
    session_key: &str,
    idempotency_key: &str,
    reply_text: &str,
    failure_class: Option<&str>,
    duration_ms: u64,
    delivery: &ReplyDelivery,
) -> serde_json::Value {
    let status = if failure_class.is_some() {
        "failed"
    } else {
        "succeeded"
    };
    serde_json::json!({
        "schema": COMMAND_RECORD_SCHEMA,
        "event": "result",
        "idempotency_key": idempotency_key,
        "nous_id": nous_id,
        "session_key": session_key,
        "origin": command_origin_record(msg),
        "command": command_record_command(cmd),
        "response": {
            "status": status,
            "failure_class": failure_class,
            "duration_ms": duration_ms,
            "reply_text": reply_text,
            "delivery": {
                "status": delivery.status,
                "error": delivery.error.as_deref(),
            },
        },
    })
}

/// Build the `origin` block of a command record.
///
/// WHY redacted (#5198): command records are persisted to the session
/// store and read back into agent context (`command_history`), so a raw
/// phone number or Matrix ID here reaches the same surface a channel
/// identity redaction is meant to keep clear of. `sender`/`group_id` are
/// redacted via the shared [`redact_channel_id`]; `conversation_id`
/// derives from the already-redacted values so it never reintroduces the
/// raw identifier. `sender_name` carries a real display name with no
/// correlation value from partial redaction, so its presence is recorded
/// without the name itself.
fn command_origin_record(msg: &InboundMessage) -> serde_json::Value {
    let redacted_sender = redact_channel_id(&msg.sender);
    let redacted_group_id = msg.group_id.as_deref().map(redact_channel_id);
    let conversation_id = redacted_group_id
        .clone()
        .unwrap_or_else(|| redacted_sender.clone());
    serde_json::json!({
        "channel": msg.channel.as_str(),
        "sender": redacted_sender,
        "sender_name": msg.sender_name.as_ref().map(|_| "[REDACTED]"),
        "group_id": redacted_group_id.clone(),
        "thread_id": redacted_group_id,
        "conversation_id": conversation_id,
        "timestamp_ms": msg.timestamp,
    })
}

fn command_record_command(cmd: &command::Command) -> serde_json::Value {
    serde_json::json!({
        "name": cmd.name(),
        "args_redacted": cmd.redacted_args(),
    })
}

fn command_failure_class(cmd: &command::Command) -> Option<&'static str> {
    match cmd {
        command::Command::Unknown { .. } => Some("unknown_command"),
        _ => None,
    }
}

fn command_idempotency_key(msg: &InboundMessage, session_key: &str) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "channel", &msg.channel);
    hash_field(&mut hasher, "sender", &msg.sender);
    hash_field(
        &mut hasher,
        "group_id",
        msg.group_id.as_deref().unwrap_or(""),
    );
    hash_field(&mut hasher, "session_key", session_key);
    hash_field(&mut hasher, "timestamp_ms", &msg.timestamp.to_string());
    hash_field(&mut hasher, "text", &msg.text);
    let digest = hasher.finalize();
    format!("sha256:{}", hex_lower(&digest))
}

fn hash_field(hasher: &mut Sha256, label: &str, value: &str) {
    hasher.update(label.as_bytes());
    hasher.update(b"\0");
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
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

fn token_estimate(content: &str) -> i64 {
    let len = i64::try_from(content.len()).unwrap_or(i64::MAX - 3);
    len.saturating_add(3) / 4
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
fn normalize_session_key(raw: &str) -> Result<String, SessionKeyError> {
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
    hasher.update(raw.as_bytes());
    let digest = hasher.finalize();
    Ok(format!("h:{}", hex_lower(&digest)))
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
) -> String {
    // Gather current-agent snapshot.
    let current_agent = if let Some(handle) = nous_manager.get(nous_id) {
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
    };

    // Gather all-agents snapshot.
    let all_agents = {
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
    let skills: Vec<String> = {
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
    };
    #[cfg(not(feature = "recall"))]
    let skills: Vec<String> = Vec::new();

    let blackboard_entries: Vec<String> = match nous_manager.blackboard_store() {
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
    };

    let ctx = CommandContext {
        current_nous_id: nous_id.to_owned(),
        session_key: session_key.to_owned(),
        current_agent,
        all_agents,
        skills,
        blackboard_entries,
        channels,
    };

    command::execute(cmd, &ctx)
}

/// Send a reply back through the originating channel.
async fn send_reply(
    msg: &InboundMessage,
    text: &str,
    channel_registry: &ChannelRegistry,
) -> ReplyDelivery {
    let to = reply_target(msg);
    let params = SendParams {
        to,
        text: text.to_owned(),
        account_id: None,
        sender_id: None,
        thread_id: None,
        attachments: None,
    };

    match channel_registry.send(&msg.channel, &params).await {
        Ok(result) => {
            if !result.sent {
                let error = result.error.unwrap_or_else(|| "unknown".to_owned());
                warn!(
                    error = %error,
                    "failed to send reply"
                );
                return ReplyDelivery {
                    status: "failed",
                    error: Some(error),
                };
            }
            ReplyDelivery {
                status: "sent",
                error: None,
            }
        }
        Err(e) => {
            warn!(error = %e, "channel send error");
            ReplyDelivery {
                status: "error",
                error: Some(e.to_string()),
            }
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
    use mneme::types::{BlackboardVisibility, Role};
    use nous::adapters::SessionBlackboardAdapter;
    use nous::config::{NousConfig, NousGenerationConfig, PipelineConfig};
    use nous::manager::NousManager;
    use organon::registry::ToolRegistry;
    use organon::types::{BlackboardStore, ToolHttpClients, ToolServices};
    use taxis::config::ChannelBinding;
    use taxis::oikos::Oikos;
    use tokio::sync::Mutex;

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
            http_clients: ToolHttpClients::new(),
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
    }

    async fn make_dispatch_harness() -> DispatchHarness {
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
            }],
            None,
        ));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let provider: Arc<dyn ChannelProvider> =
            Arc::new(RecordingChannel::new(Arc::clone(&sent), SendResult::ok()));
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

    fn command_message(text: &str, timestamp: u64) -> InboundMessage {
        InboundMessage {
            channel: "signal".to_owned(),
            sender: "+15550100".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: None,
            message_id: None,
            text: text.to_owned(),
            timestamp,
            attachments: vec![],
            raw: None,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn redelivered_message_runs_exactly_one_turn() {
        let harness = make_dispatch_harness().await;
        let (tx, rx) = mpsc::channel(8);
        let (ready_tx, ready_rx) = watch::channel(true);

        let dispatcher = spawn_dispatcher(
            rx,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
            ready_rx,
        );

        let mut msg = command_message("hello there", 1_709_312_345_678);
        msg.message_id = Some("provider-msg-1".to_owned());
        tx.send(msg.clone()).await.expect("send original");
        // WHY: identical provider message identity -- an at-least-once
        // transport redelivering after a reconnect, not a new message.
        tx.send(msg).await.expect("send redelivery");
        drop(tx);

        dispatcher.await.expect("dispatcher drains and exits");
        drop(ready_tx);

        {
            let sent = harness.sent.lock().await;
            assert_eq!(
                sent.len(),
                1,
                "a redelivered inbound message must not run a second turn: {sent:?}"
            );
        }

        shutdown_harness(harness).await;
    }

    async fn command_history(harness: &DispatchHarness) -> Vec<mneme::types::Message> {
        let store = harness.session_store.lock().await;
        let session_key =
            normalize_session_key("signal:+15550100").expect("routed session key is valid");
        let session = store
            .find_session("alice", &session_key)
            .expect("find session")
            .expect("session exists");
        store.get_history(&session.id, None).expect("history")
    }

    fn record_json(message: &mneme::types::Message) -> serde_json::Value {
        serde_json::from_str(&message.content).expect("command record json")
    }

    fn json_str<'a>(value: &'a serde_json::Value, pointer: &str) -> Option<&'a str> {
        value.pointer(pointer).and_then(serde_json::Value::as_str)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatch_records_successful_command_history() {
        let harness = make_dispatch_harness().await;
        let msg = command_message("!ping", 1_709_312_345_678);

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert!(sent[0].text.contains("Pong"), "{:?}", sent[0].text);
        }

        let history = command_history(&harness).await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, Role::System);
        assert_eq!(history[1].role, Role::System);

        let invocation = record_json(&history[0]);
        let result = record_json(&history[1]);
        let session_key =
            normalize_session_key("signal:+15550100").expect("routed session key is valid");
        assert_eq!(
            json_str(&invocation, "/schema"),
            Some(COMMAND_RECORD_SCHEMA)
        );
        assert_eq!(json_str(&invocation, "/event"), Some("invocation"));
        assert_eq!(json_str(&invocation, "/command/name"), Some("ping"));
        assert_eq!(json_str(&invocation, "/origin/channel"), Some("signal"));
        assert_eq!(
            json_str(&invocation, "/origin/sender"),
            Some(redact_channel_id("+15550100").as_str()),
            "sender must be redacted, not stored raw in the command record"
        );
        assert_ne!(json_str(&invocation, "/origin/sender"), Some("+15550100"));
        assert_eq!(
            json_str(&invocation, "/session_key"),
            Some(session_key.as_str())
        );
        assert_eq!(json_str(&result, "/event"), Some("result"));
        assert_eq!(json_str(&result, "/response/status"), Some("succeeded"));
        assert_eq!(json_str(&result, "/response/delivery/status"), Some("sent"));
        assert!(
            result
                .pointer("/response/duration_ms")
                .and_then(serde_json::Value::as_u64)
                .is_some(),
            "{result}"
        );
        assert_eq!(
            invocation
                .pointer("/idempotency_key")
                .and_then(serde_json::Value::as_str),
            result
                .pointer("/idempotency_key")
                .and_then(serde_json::Value::as_str)
        );

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatch_records_failed_command_history_with_redacted_args() {
        let harness = make_dispatch_harness().await;
        let msg = command_message("!frobnicate --token secret-value target", 1_709_312_345_679);

        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 1);
            assert!(sent[0].text.contains("!help"), "{:?}", sent[0].text);
        }

        let history = command_history(&harness).await;
        assert_eq!(history.len(), 2);
        let invocation = record_json(&history[0]);
        let result = record_json(&history[1]);
        assert_eq!(json_str(&invocation, "/command/name"), Some("frobnicate"));
        assert_eq!(
            json_str(&invocation, "/command/args_redacted"),
            Some("--token [REDACTED] target")
        );
        assert_eq!(json_str(&result, "/response/status"), Some("failed"));
        assert_eq!(
            json_str(&result, "/response/failure_class"),
            Some("unknown_command")
        );
        assert!(
            !history[0].content.contains("secret-value"),
            "{}",
            history[0].content
        );
        assert!(
            !history[1].content.contains("secret-value"),
            "{}",
            history[1].content
        );

        shutdown_harness(harness).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatch_deduplicates_retried_command_delivery() {
        let harness = make_dispatch_harness().await;
        let msg = command_message("!ping", 1_709_312_345_680);

        dispatch_one(
            msg.clone(),
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
        )
        .await;
        dispatch_one(
            msg,
            Arc::clone(&harness.router),
            Arc::clone(&harness.nous_manager),
            Arc::clone(&harness.channel_registry),
            Arc::clone(&harness.session_store),
        )
        .await;

        {
            let sent = harness.sent.lock().await;
            assert_eq!(sent.len(), 2);
            assert_eq!(sent[0].text, sent[1].text);
        }

        let history = command_history(&harness).await;
        assert_eq!(history.len(), 2, "duplicate must not append records");
        assert_eq!(
            json_str(&record_json(&history[0]), "/event"),
            Some("invocation")
        );
        assert_eq!(
            json_str(&record_json(&history[1]), "/event"),
            Some("result")
        );

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
        )
        .await;
        assert!(
            blackboard_reply.contains("Blackboard empty"),
            "{blackboard_reply}"
        );
    }

    #[test]
    fn command_origin_record_redacts_identity_fields() {
        let msg = InboundMessage {
            channel: "matrix".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: Some("!room:example.org".to_owned()),
            message_id: None,
            text: "!ping".to_owned(),
            timestamp: 1_709_312_345_678,
            attachments: vec![],
            raw: None,
        };

        let record = command_origin_record(&msg);
        let dump = record.to_string();
        assert!(!dump.contains("@alice:example.org"), "{dump}");
        assert!(!dump.contains("Alice"), "{dump}");
        assert!(!dump.contains("!room:example.org"), "{dump}");
        assert_eq!(
            json_str(&record, "/sender"),
            Some(redact_channel_id("@alice:example.org").as_str())
        );
        assert_eq!(
            json_str(&record, "/group_id"),
            Some(redact_channel_id("!room:example.org").as_str())
        );
        assert_eq!(
            json_str(&record, "/conversation_id"),
            json_str(&record, "/group_id")
        );
    }

    #[test]
    fn command_origin_record_conversation_id_falls_back_to_redacted_sender() {
        let msg = InboundMessage {
            channel: "signal".to_owned(),
            sender: "+15550100".to_owned(),
            sender_name: None,
            group_id: None,
            message_id: None,
            text: "!ping".to_owned(),
            timestamp: 1_709_312_345_678,
            attachments: vec![],
            raw: None,
        };

        let record = command_origin_record(&msg);
        assert_eq!(
            json_str(&record, "/conversation_id"),
            Some(redact_channel_id("+15550100").as_str())
        );
    }

    #[test]
    fn normalize_session_key_hashes_phone_numbers() {
        let key = normalize_session_key("signal:+15550100").expect("valid phone session key");
        assert!(key.starts_with("h:"), "hashed keys are prefixed: {key}");
        assert!(
            !key.contains("+15550100"),
            "raw phone number must be redacted: {key}"
        );
        assert_eq!(
            key,
            normalize_session_key("signal:+15550100").expect("stable")
        );
    }

    #[test]
    fn normalize_session_key_hashes_matrix_ids() {
        let key =
            normalize_session_key("matrix:@alice:example.org").expect("valid matrix session key");
        assert!(
            !key.contains("@alice:example.org"),
            "raw matrix id must be redacted: {key}"
        );
    }

    #[test]
    fn normalize_session_key_hashes_group_ids() {
        let key = normalize_session_key("signal:group-abc!xyz").expect("valid group session key");
        assert!(
            !key.contains("group-abc"),
            "raw group id must be redacted: {key}"
        );
    }

    #[test]
    fn normalize_session_key_hashes_path_like_values() {
        let key = normalize_session_key("webhook:/api/v1/incoming/abc")
            .expect("valid path-like session key");
        assert!(!key.contains("/api/v1"), "raw path must be redacted: {key}");
    }

    #[test]
    fn normalize_session_key_rejects_empty() {
        assert!(normalize_session_key("").is_err());
    }

    #[test]
    fn normalize_session_key_rejects_oversized_keys() {
        let oversized = "x".repeat(MAX_SESSION_KEY_LEN + 1);
        assert!(
            normalize_session_key(&oversized).is_err(),
            "keys over {MAX_SESSION_KEY_LEN} bytes must be rejected"
        );

        let at_limit = "y".repeat(MAX_SESSION_KEY_LEN);
        assert!(
            normalize_session_key(&at_limit).is_ok(),
            "keys at the limit must be accepted"
        );
    }

    #[test]
    fn normalize_session_key_rejects_control_characters() {
        assert!(normalize_session_key("signal:\0sender").is_err());
        assert!(normalize_session_key("signal:\nsender").is_err());
    }
}
