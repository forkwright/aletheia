//! Background dispatch loop: routes inbound messages to nous actors.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{Instrument, debug, info, warn};

use agora::command::{self, AgentSnapshot, ChannelSnapshot, CommandContext};
use agora::registry::ChannelRegistry;
use agora::router::{MessageRouter, RouteDecision, reply_target};
use agora::types::{InboundMessage, SendParams};
use koina::id::SessionId;
use koina::redact::redact_sensitive;
use mneme::store::{FinalizeMessage, FinalizeNote, FinalizeTurnRequest, SessionStore};
use mneme::types::Role;
use nous::manager::NousManager;

/// Spawn a background task that dispatches inbound messages to nous actors.
///
/// Runs until the receiver channel closes (all senders dropped).
/// Per-message dispatch tasks are tracked in a `JoinSet` and drained on exit.
pub(crate) fn spawn_dispatcher(
    mut rx: mpsc::Receiver<InboundMessage>,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
    session_store: Arc<Mutex<SessionStore>>,
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

            while let Some(msg) = rx.recv().await {
                let router = Arc::clone(&router);
                let nous_mgr = Arc::clone(&nous_manager);
                let channels = Arc::clone(&channel_registry);
                let store = Arc::clone(&session_store);
                let msg_span = tracing::info_span!(
                    "dispatch",
                    channel = %msg.channel,
                    sender = %msg.sender,
                );
                in_flight.spawn(
                    dispatch_one(msg, router, nous_mgr, channels, store).instrument(msg_span),
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
    session_store: Arc<Mutex<SessionStore>>,
) {
    let Some(decision) = router.resolve(&msg) else {
        warn!(
            channel = %msg.channel,
            sender = %msg.sender,
            "no route for inbound message, dropping"
        );
        return;
    };

    // NOTE: `!`-commands are intercepted before reaching the nous agent.
    // Plain turns fall through to send_turn as before.
    if let Some(cmd) = command::parse(&msg.text) {
        handle_command_message(
            &msg,
            &cmd,
            &decision,
            &nous_manager,
            &channel_registry,
            &session_store,
        )
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
        session_key = %decision.session_key,
        matched_by = ?decision.matched_by,
        "dispatching turn"
    );

    let turn_result = match handle.send_turn(&decision.session_key, &msg.text).await {
        Ok(result) => result,
        Err(e) => {
            warn!(error = %e, nous_id = %decision.nous_id, "turn failed");
            return;
        }
    };

    send_reply(&msg, &turn_result.content, &channel_registry).await;
}

async fn handle_command_message(
    msg: &InboundMessage,
    cmd: &command::Command,
    decision: &RouteDecision<'_>,
    nous_manager: &NousManager,
    channel_registry: &ChannelRegistry,
    session_store: &Mutex<SessionStore>,
) {
    let input = CommandRecordInput::from_message(
        msg,
        decision.nous_id,
        &decision.session_key,
        cmd,
        nous_manager
            .get_config(decision.nous_id)
            .map(|config| config.generation.model.as_str()),
    );
    let command_start = match begin_command_record(session_store, &input).await {
        Ok(start) => Some(start),
        Err(e) => {
            warn!(
                error = %e,
                nous_id = %decision.nous_id,
                session_key = %decision.session_key,
                command = cmd.name(),
                "failed to persist command lifecycle start"
            );
            None
        }
    };
    if let Some(CommandRecordStart::Duplicate { reply_text }) = command_start {
        info!(
            nous_id = %decision.nous_id,
            session_key = %decision.session_key,
            command = cmd.name(),
            "replaying duplicate !-command response"
        );
        send_reply(msg, &reply_text, channel_registry).await;
        return;
    }
    if let Some(CommandRecordStart::InFlight { reply_text }) = command_start {
        info!(
            nous_id = %decision.nous_id,
            session_key = %decision.session_key,
            command = cmd.name(),
            "duplicate !-command is already in flight"
        );
        send_reply(msg, &reply_text, channel_registry).await;
        return;
    }
    let session_id = command_start.and_then(|start| match start {
        CommandRecordStart::New { session_id } => Some(session_id),
        CommandRecordStart::Duplicate { .. } | CommandRecordStart::InFlight { .. } => None,
    });

    debug!(
        nous_id = %decision.nous_id,
        command = cmd.name(),
        "dispatching !-command"
    );
    let started = std::time::Instant::now();
    let reply_text = execute_command(
        cmd,
        decision.nous_id,
        &decision.session_key,
        nous_manager,
        channel_registry,
    )
    .await;
    if let Some(session_id) = session_id {
        let outcome = CommandOutcome::from_command(cmd, started.elapsed());
        if let Err(e) =
            finish_command_record(session_store, &input, &session_id, &reply_text, outcome).await
        {
            warn!(
                error = %e,
                nous_id = %decision.nous_id,
                session_key = %decision.session_key,
                command = cmd.name(),
                "failed to persist command lifecycle result"
            );
        }
    }
    send_reply(msg, &reply_text, channel_registry).await;
}

const COMMAND_NOTE_CATEGORY: &str = "context";
const COMMAND_RECORD_KIND: &str = "dispatch_command";

#[derive(Debug, Clone)]
struct CommandRecordInput {
    idempotency_key: String,
    nous_id: String,
    session_key: String,
    model: Option<String>,
    command_name: String,
    arguments_redacted: Option<String>,
    invocation_redacted: String,
    origin: CommandOrigin,
}

impl CommandRecordInput {
    fn from_message(
        msg: &InboundMessage,
        nous_id: &str,
        session_key: &str,
        cmd: &command::Command,
        model: Option<&str>,
    ) -> Self {
        Self {
            idempotency_key: command_idempotency_key(msg),
            nous_id: nous_id.to_owned(),
            session_key: session_key.to_owned(),
            model: model.map(str::to_owned),
            command_name: cmd.name().to_owned(),
            arguments_redacted: command_arguments_redacted(&msg.text),
            invocation_redacted: redact_sensitive(msg.text.trim()),
            origin: CommandOrigin::from_message(msg),
        }
    }
}

#[derive(Debug)]
enum CommandRecordStart {
    New { session_id: String },
    Duplicate { reply_text: String },
    InFlight { reply_text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandOrigin {
    channel: String,
    sender: String,
    sender_name: Option<String>,
    group_id: Option<String>,
    thread_id: Option<String>,
    session_key: String,
    reply_target: String,
    timestamp_ms: u64,
    raw_event_id: Option<String>,
}

impl CommandOrigin {
    fn from_message(msg: &InboundMessage) -> Self {
        Self {
            channel: msg.channel.clone(),
            sender: msg.sender.clone(),
            sender_name: msg.sender_name.clone(),
            group_id: msg.group_id.clone(),
            thread_id: raw_thread_id(msg.raw.as_ref()).or_else(|| msg.group_id.clone()),
            session_key: String::new(),
            reply_target: reply_target(msg),
            timestamp_ms: msg.timestamp,
            raw_event_id: raw_event_id(msg.raw.as_ref()),
        }
    }

    fn with_session_key(mut self, session_key: &str) -> Self {
        session_key.clone_into(&mut self.session_key);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CommandLifecycleStatus {
    Accepted,
    Completed,
    Failed,
    DuplicateReplayed,
}

impl CommandLifecycleStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandLifecycleRecord {
    version: u32,
    kind: String,
    idempotency_key: String,
    status: CommandLifecycleStatus,
    session_id: String,
    nous_id: String,
    session_key: String,
    command_name: String,
    arguments_redacted: Option<String>,
    origin: CommandOrigin,
    response_status: Option<String>,
    duration_ms: Option<u64>,
    failure_class: Option<String>,
    response_text: Option<String>,
    duplicate_of: Option<String>,
    created_at: String,
}

impl CommandLifecycleRecord {
    fn accepted(input: &CommandRecordInput, session_id: &str) -> Self {
        Self::base(input, session_id, CommandLifecycleStatus::Accepted)
    }

    fn finished(
        input: &CommandRecordInput,
        session_id: &str,
        reply_text: &str,
        outcome: CommandOutcome,
    ) -> Self {
        let mut record = Self::base(input, session_id, outcome.status);
        record.response_status = Some(outcome.response_status.to_owned());
        record.duration_ms = Some(duration_millis(outcome.duration));
        record.failure_class = outcome.failure_class.map(str::to_owned);
        record.response_text = Some(reply_text.to_owned());
        record
    }

    fn duplicate(input: &CommandRecordInput, original: &Self) -> Self {
        let mut record = Self::base(
            input,
            &original.session_id,
            CommandLifecycleStatus::DuplicateReplayed,
        );
        record.response_status.clone_from(&original.response_status);
        record.response_text.clone_from(&original.response_text);
        record.duplicate_of = Some(original.created_at.clone());
        record
    }

    fn base(input: &CommandRecordInput, session_id: &str, status: CommandLifecycleStatus) -> Self {
        Self {
            version: 1,
            kind: COMMAND_RECORD_KIND.to_owned(),
            idempotency_key: input.idempotency_key.clone(),
            status,
            session_id: session_id.to_owned(),
            nous_id: input.nous_id.clone(),
            session_key: input.session_key.clone(),
            command_name: input.command_name.clone(),
            arguments_redacted: input.arguments_redacted.clone(),
            origin: input.origin.clone().with_session_key(&input.session_key),
            response_status: None,
            duration_ms: None,
            failure_class: None,
            response_text: None,
            duplicate_of: None,
            created_at: jiff::Timestamp::now().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CommandOutcome {
    status: CommandLifecycleStatus,
    response_status: &'static str,
    failure_class: Option<&'static str>,
    duration: Duration,
}

impl CommandOutcome {
    fn from_command(cmd: &command::Command, duration: Duration) -> Self {
        match cmd {
            command::Command::Unknown { .. } => Self {
                status: CommandLifecycleStatus::Failed,
                response_status: "error",
                failure_class: Some("unknown_command"),
                duration,
            },
            _ => Self {
                status: CommandLifecycleStatus::Completed,
                response_status: "ok",
                failure_class: None,
                duration,
            },
        }
    }
}

async fn begin_command_record(
    session_store: &Mutex<SessionStore>,
    input: &CommandRecordInput,
) -> Result<CommandRecordStart, String> {
    let store = session_store.lock().await;
    begin_command_record_locked(&store, input)
}

fn begin_command_record_locked(
    store: &SessionStore,
    input: &CommandRecordInput,
) -> Result<CommandRecordStart, String> {
    let generated_session_id = SessionId::new().to_string();
    let session = store
        .find_or_create_session(
            &generated_session_id,
            &input.nous_id,
            &input.session_key,
            input.model.as_deref(),
            None,
        )
        .map_err(|e| e.to_string())?;

    let records = command_records_for_session(store, &session.id, &input.idempotency_key)?;
    if let Some(record) = records
        .iter()
        .rev()
        .find(|record| record.status.is_terminal())
    {
        let duplicate = CommandLifecycleRecord::duplicate(input, record);
        persist_command_note(store, &duplicate)?;
        return Ok(CommandRecordStart::Duplicate {
            reply_text: record
                .response_text
                .clone()
                .unwrap_or_else(|| "Command already processed.".to_owned()),
        });
    }

    if records
        .iter()
        .any(|record| record.status == CommandLifecycleStatus::Accepted)
    {
        return Ok(CommandRecordStart::InFlight {
            reply_text: format!(
                "Command '!{}' is already being processed for this session; retry shortly.",
                input.command_name
            ),
        });
    }

    let record = CommandLifecycleRecord::accepted(input, &session.id);
    persist_command_note(store, &record)?;
    Ok(CommandRecordStart::New {
        session_id: session.id,
    })
}

async fn finish_command_record(
    session_store: &Mutex<SessionStore>,
    input: &CommandRecordInput,
    session_id: &str,
    reply_text: &str,
    outcome: CommandOutcome,
) -> Result<(), String> {
    let store = session_store.lock().await;
    finish_command_record_locked(&store, input, session_id, reply_text, outcome)
}

fn finish_command_record_locked(
    store: &SessionStore,
    input: &CommandRecordInput,
    session_id: &str,
    reply_text: &str,
    outcome: CommandOutcome,
) -> Result<(), String> {
    let record = CommandLifecycleRecord::finished(input, session_id, reply_text, outcome);
    let note_content = serialize_command_record(&record)?;
    let messages = [
        FinalizeMessage {
            role: Role::User,
            content: &input.invocation_redacted,
            tool_call_id: None,
            tool_name: None,
            token_estimate: token_estimate(&input.invocation_redacted),
        },
        FinalizeMessage {
            role: Role::Assistant,
            content: reply_text,
            tool_call_id: None,
            tool_name: None,
            token_estimate: token_estimate(reply_text),
        },
    ];
    let note = FinalizeNote {
        category: COMMAND_NOTE_CATEGORY,
        content: &note_content,
    };
    store
        .finalize_turn(&FinalizeTurnRequest {
            session_id,
            nous_id: &input.nous_id,
            session_key: &input.session_key,
            model: input.model.as_deref(),
            parent_session_id: None,
            messages: &messages,
            usage: None,
            tool_audit_records: &[],
            completion_note: Some(note),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn command_records_for_session(
    store: &SessionStore,
    session_id: &str,
    idempotency_key: &str,
) -> Result<Vec<CommandLifecycleRecord>, String> {
    let records = store
        .get_notes(session_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|note| serde_json::from_str::<CommandLifecycleRecord>(&note.content).ok())
        .filter(|record| {
            record.kind == COMMAND_RECORD_KIND && record.idempotency_key == idempotency_key
        })
        .collect();
    Ok(records)
}

fn persist_command_note(
    store: &SessionStore,
    record: &CommandLifecycleRecord,
) -> Result<(), String> {
    let content = serialize_command_record(record)?;
    store
        .add_note(
            &record.session_id,
            &record.nous_id,
            COMMAND_NOTE_CATEGORY,
            &content,
        )
        .map_err(|e| e.to_string())?;
    store.ensure_durable().map_err(|e| e.to_string())?;
    Ok(())
}

fn serialize_command_record(record: &CommandLifecycleRecord) -> Result<String, String> {
    serde_json::to_string(record).map_err(|e| e.to_string())
}

fn command_arguments_redacted(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let without_bang = trimmed.strip_prefix('!')?.trim();
    let (_, rest) = without_bang
        .split_once(char::is_whitespace)
        .map_or((without_bang, ""), |(name, rest)| (name, rest.trim()));
    if rest.is_empty() {
        None
    } else {
        Some(redact_sensitive(rest))
    }
}

fn command_idempotency_key(msg: &InboundMessage) -> String {
    let mut hasher = Sha256::new();
    update_hash_part(&mut hasher, &msg.channel);
    update_hash_part(&mut hasher, &msg.sender);
    update_hash_part(&mut hasher, msg.group_id.as_deref().unwrap_or(""));
    update_hash_part(&mut hasher, &msg.timestamp.to_string());
    update_hash_part(
        &mut hasher,
        raw_event_id(msg.raw.as_ref()).as_deref().unwrap_or(""),
    );
    update_hash_part(&mut hasher, msg.text.trim());
    format!("sha256:{}", hex_digest(&hasher.finalize()))
}

fn update_hash_part(hasher: &mut Sha256, part: &str) {
    hasher.update(part.len().to_be_bytes());
    hasher.update(part.as_bytes());
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push(char::from_digit(u32::from(*byte >> 4), 16).unwrap_or('0'));
        hex.push(char::from_digit(u32::from(*byte & 0x0f), 16).unwrap_or('0'));
    }
    hex
}

fn raw_event_id(raw: Option<&serde_json::Value>) -> Option<String> {
    raw.and_then(|value| {
        first_string_field(
            value,
            &["event_id", "eventId", "message_id", "messageId", "id"],
        )
        .or_else(|| {
            value.get("envelope").and_then(|envelope| {
                first_string_field(
                    envelope,
                    &[
                        "event_id",
                        "eventId",
                        "message_id",
                        "messageId",
                        "timestamp",
                    ],
                )
            })
        })
    })
}

fn raw_thread_id(raw: Option<&serde_json::Value>) -> Option<String> {
    raw.and_then(|value| {
        first_string_field(value, &["thread_id", "threadId", "room_id", "roomId"]).or_else(|| {
            value.get("content").and_then(|content| {
                first_string_field(content, &["thread_id", "threadId", "room_id", "roomId"])
            })
        })
    })
}

fn first_string_field(value: &serde_json::Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .filter_map(|field| value.get(*field))
        .find_map(json_scalar_to_string)
}

fn json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            None
        }
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn token_estimate(text: &str) -> i64 {
    i64::try_from(nous::budget::CharEstimator::default().estimate(text)).unwrap_or(i64::MAX)
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
        Some(blackboard_store) => match blackboard_store.list() {
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
        },
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
async fn send_reply(msg: &InboundMessage, text: &str, channel_registry: &ChannelRegistry) {
    let to = reply_target(msg);
    let params = SendParams {
        to,
        text: text.to_owned(),
        account_id: None,
        thread_id: None,
        attachments: None,
    };

    match channel_registry.send(&msg.channel, &params).await {
        Ok(result) => {
            if !result.sent {
                warn!(
                    error = result.error.as_deref().unwrap_or("unknown"),
                    "failed to send reply"
                );
            }
        }
        Err(e) => {
            warn!(error = %e, "channel send error");
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    #[cfg(feature = "recall")]
    use std::collections::HashMap;

    use super::*;
    use hermeneus::provider::ProviderRegistry;
    use hermeneus::test_utils::MockProvider;
    use mneme::store::SessionStore;
    use nous::adapters::SessionBlackboardAdapter;
    use nous::config::{NousConfig, NousGenerationConfig, PipelineConfig};
    use nous::manager::NousManager;
    use organon::registry::ToolRegistry;
    use organon::types::{BlackboardStore, ToolHttpClients, ToolServices};
    use taxis::oikos::Oikos;

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

    fn command_message(text: &str, timestamp: u64, raw_id: &str) -> InboundMessage {
        InboundMessage {
            channel: "signal".to_owned(),
            sender: "+15550100".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: Some("group-1".to_owned()),
            text: text.to_owned(),
            timestamp,
            attachments: vec![],
            raw: Some(serde_json::json!({ "messageId": raw_id })),
        }
    }

    fn command_input_for(msg: &InboundMessage, cmd: &command::Command) -> CommandRecordInput {
        CommandRecordInput::from_message(msg, "alice", "signal:group-1", cmd, Some("test-model"))
    }

    fn start_new_command(store: &SessionStore, input: &CommandRecordInput) -> String {
        match begin_command_record_locked(store, input).expect("begin command record") {
            CommandRecordStart::New { session_id } => session_id,
            CommandRecordStart::Duplicate { .. } | CommandRecordStart::InFlight { .. } => {
                panic!("expected new command")
            }
        }
    }

    #[test]
    fn command_record_persists_successful_turn_and_metadata() {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        let msg = command_message("!info alice secret=hunter2", 100, "msg-1");
        let cmd = command::parse(&msg.text).expect("command parses");
        let input = command_input_for(&msg, &cmd);
        let session_id = start_new_command(&store, &input);

        finish_command_record_locked(
            &store,
            &input,
            &session_id,
            "Agent alice is ready.",
            CommandOutcome::from_command(&cmd, Duration::from_millis(12)),
        )
        .expect("finish command record");

        let history = store
            .get_history_raw(&session_id, None)
            .expect("read command history");
        assert_eq!(history.len(), 2);
        let mut history_iter = history.iter();
        let user = history_iter.next().expect("user command message");
        let assistant = history_iter.next().expect("assistant command response");
        assert_eq!(user.role, Role::User);
        assert_eq!(assistant.role, Role::Assistant);
        assert!(user.content.contains("secret=***"), "{}", user.content);
        assert!(!user.content.contains("hunter2"), "{}", user.content);
        assert_eq!(assistant.content, "Agent alice is ready.");

        let records = command_records_for_session(&store, &session_id, &input.idempotency_key)
            .expect("read command records");
        assert_eq!(records.len(), 2);
        let first = records.first().expect("accepted command record");
        assert_eq!(first.status, CommandLifecycleStatus::Accepted);
        let completed = records
            .iter()
            .find(|record| record.status == CommandLifecycleStatus::Completed)
            .expect("completed command record");
        assert_eq!(completed.command_name, "info");
        assert_eq!(completed.response_status.as_deref(), Some("ok"));
        assert_eq!(completed.duration_ms, Some(12));
        assert_eq!(completed.failure_class, None);
        assert_eq!(completed.origin.channel, "signal");
        assert_eq!(completed.origin.group_id.as_deref(), Some("group-1"));
        assert_eq!(completed.origin.session_key, "signal:group-1");
        assert_eq!(completed.origin.raw_event_id.as_deref(), Some("msg-1"));
    }

    #[test]
    fn command_record_marks_unknown_command_as_failure() {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        let msg = command_message(
            "!bogus api_key=sk-proj-1234567890abcdef123456",
            101,
            "msg-2",
        );
        let cmd = command::parse(&msg.text).expect("command parses");
        let input = command_input_for(&msg, &cmd);
        let session_id = start_new_command(&store, &input);

        finish_command_record_locked(
            &store,
            &input,
            &session_id,
            "Unknown command '!bogus'. Type !help for a list of available commands.",
            CommandOutcome::from_command(&cmd, Duration::from_millis(3)),
        )
        .expect("finish command record");

        let records = command_records_for_session(&store, &session_id, &input.idempotency_key)
            .expect("read command records");
        let failed = records
            .iter()
            .find(|record| record.status == CommandLifecycleStatus::Failed)
            .expect("failed command record");
        assert_eq!(failed.response_status.as_deref(), Some("error"));
        assert_eq!(failed.failure_class.as_deref(), Some("unknown_command"));
        assert_eq!(failed.arguments_redacted.as_deref(), Some("api_key=***"));
    }

    #[test]
    fn duplicate_command_replays_reply_without_appending_messages() {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        let msg = command_message("!ping", 102, "msg-3");
        let cmd = command::parse(&msg.text).expect("command parses");
        let input = command_input_for(&msg, &cmd);
        let session_id = start_new_command(&store, &input);

        finish_command_record_locked(
            &store,
            &input,
            &session_id,
            "Pong.",
            CommandOutcome::from_command(&cmd, Duration::from_millis(2)),
        )
        .expect("finish command record");

        match begin_command_record_locked(&store, &input).expect("begin duplicate command") {
            CommandRecordStart::Duplicate { reply_text } => assert_eq!(reply_text, "Pong."),
            CommandRecordStart::New { .. } | CommandRecordStart::InFlight { .. } => {
                panic!("expected duplicate replay")
            }
        }

        let history = store
            .get_history_raw(&session_id, None)
            .expect("read command history");
        assert_eq!(history.len(), 2);
        let records = command_records_for_session(&store, &session_id, &input.idempotency_key)
            .expect("read command records");
        assert!(
            records
                .iter()
                .any(|record| record.status == CommandLifecycleStatus::DuplicateReplayed),
            "duplicate replay record missing: {records:?}"
        );
    }
}
