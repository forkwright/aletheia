//! Background dispatch loop: routes inbound messages to nous actors.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{Instrument, debug, info, warn};

use agora::command::{self, AgentSnapshot, ChannelSnapshot, CommandContext};
use agora::registry::ChannelRegistry;
use agora::router::{MessageRouter, RouteDecision, reply_target};
use agora::types::{InboundMessage, SendParams};
use mneme::types::Role;
use nous::manager::NousManager;

use crate::error::{Error as AletheiaError, Result as AletheiaResult};

const COMMAND_RECORD_CATEGORY: &str = "context";
const COMMAND_RECORD_VERSION: u32 = 1;

struct PreparedCommandRecord {
    store: Arc<tokio::sync::Mutex<mneme::store::SessionStore>>,
    session_id: String,
    idempotency_key: String,
}

enum CommandPrepareOutcome {
    Fresh(PreparedCommandRecord),
    Duplicate { reply_text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandLifecycleRecord {
    version: u32,
    record_type: String,
    idempotency_key: String,
    session_id: String,
    nous_id: String,
    session_key: String,
    origin: CommandOriginRecord,
    command: CommandInvocationRecord,
    result: CommandResultRecord,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandOriginRecord {
    channel: String,
    sender: String,
    sender_name: Option<String>,
    group_id: Option<String>,
    thread_id: Option<String>,
    transport_message_id: Option<String>,
    inbound_timestamp_ms: u64,
    reply_target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandInvocationRecord {
    name: String,
    arguments_redacted: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandResultRecord {
    status: CommandResponseStatus,
    duration_ms: u128,
    failure_class: Option<String>,
    reply_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CommandResponseStatus {
    Success,
    Failed,
}

/// Spawn a background task that dispatches inbound messages to nous actors.
///
/// Runs until the receiver channel closes (all senders dropped).
/// Per-message dispatch tasks are tracked in a `JoinSet` and drained on exit.
pub(crate) fn spawn_dispatcher(
    mut rx: mpsc::Receiver<InboundMessage>,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
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
                let msg_span = tracing::info_span!(
                    "dispatch",
                    channel = %msg.channel,
                    sender = %msg.sender,
                );
                in_flight.spawn(dispatch_one(msg, router, nous_mgr, channels).instrument(msg_span));

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
        debug!(
            nous_id = %decision.nous_id,
            command = cmd.name(),
            "dispatching !-command"
        );
        let prepared_record = prepare_command_record(&msg, &cmd, &decision, &nous_manager).await;
        if let Some(CommandPrepareOutcome::Duplicate { reply_text }) = prepared_record.as_ref() {
            debug!(
                nous_id = %decision.nous_id,
                session_key = %decision.session_key,
                command = cmd.name(),
                "replaying duplicate !-command response"
            );
            send_reply(&msg, reply_text, &channel_registry).await;
            return;
        }

        let started_at = Instant::now();
        let reply_text = execute_command(
            &cmd,
            decision.nous_id,
            &decision.session_key,
            &nous_manager,
            &channel_registry,
        )
        .await;
        if let Some(CommandPrepareOutcome::Fresh(record)) = prepared_record
            && let Err(error) =
                persist_command_record(&record, &msg, &cmd, &decision, &reply_text, started_at)
                    .await
        {
            warn!(
                error = %error,
                nous_id = %decision.nous_id,
                session_key = %decision.session_key,
                command = cmd.name(),
                "failed to persist !-command record"
            );
        }
        send_reply(&msg, &reply_text, &channel_registry).await;
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

async fn prepare_command_record(
    msg: &InboundMessage,
    cmd: &command::Command,
    decision: &RouteDecision<'_>,
    nous_manager: &NousManager,
) -> Option<CommandPrepareOutcome> {
    let Some(store) = nous_manager.session_store() else {
        warn!(
            nous_id = %decision.nous_id,
            session_key = %decision.session_key,
            command = cmd.name(),
            "no session store configured; !-command will not be durable"
        );
        return None;
    };

    let model = nous_manager
        .get_config(decision.nous_id)
        .map(|config| config.generation.model.clone());
    let candidate_session_id = koina::id::SessionId::new().to_string();
    let idempotency_key = command_idempotency_key(msg, cmd, decision);

    let session_id = {
        let guard = store.lock().await;
        let session = match guard.find_or_create_session(
            &candidate_session_id,
            decision.nous_id,
            &decision.session_key,
            model.as_deref(),
            None,
        ) {
            Ok(session) => session,
            Err(error) => {
                warn!(
                    error = %error,
                    nous_id = %decision.nous_id,
                    session_key = %decision.session_key,
                    command = cmd.name(),
                    "failed to resolve session for !-command record"
                );
                return None;
            }
        };

        match find_command_record_by_idempotency_key(&guard, &session.id, &idempotency_key) {
            Ok(Some(record)) => {
                return Some(CommandPrepareOutcome::Duplicate {
                    reply_text: record.result.reply_text,
                });
            }
            Ok(None) => session.id,
            Err(error) => {
                warn!(
                    error = %error,
                    session_id = %session.id,
                    command = cmd.name(),
                    "failed to scan prior !-command records; proceeding without duplicate replay"
                );
                session.id
            }
        }
    };

    Some(CommandPrepareOutcome::Fresh(PreparedCommandRecord {
        store,
        session_id,
        idempotency_key,
    }))
}

async fn persist_command_record(
    prepared: &PreparedCommandRecord,
    msg: &InboundMessage,
    cmd: &command::Command,
    decision: &RouteDecision<'_>,
    reply_text: &str,
    started_at: Instant,
) -> AletheiaResult<()> {
    let duration = started_at.elapsed();
    let record = build_command_lifecycle_record(
        prepared,
        msg,
        cmd,
        decision,
        reply_text,
        duration,
        jiff::Timestamp::now().to_string(),
    );
    let record_json = serde_json::to_string(&record).map_err(|error| {
        AletheiaError::msg(format!("serialize command lifecycle record: {error}"))
    })?;
    let input_tokens = estimate_tokens(&msg.text);
    let output_tokens = estimate_tokens(reply_text);

    let guard = prepared.store.lock().await;
    if find_command_record_by_idempotency_key(
        &guard,
        &prepared.session_id,
        &prepared.idempotency_key,
    )
    .map_err(|error| AletheiaError::msg(format!("scan existing command record: {error}")))?
    .is_some()
    {
        return Ok(());
    }

    guard
        .append_message(
            &prepared.session_id,
            Role::User,
            &msg.text,
            None,
            None,
            input_tokens,
        )
        .map_err(|error| {
            AletheiaError::msg(format!("append command invocation message: {error}"))
        })?;
    guard
        .append_message(
            &prepared.session_id,
            Role::Assistant,
            reply_text,
            None,
            None,
            output_tokens,
        )
        .map_err(|error| AletheiaError::msg(format!("append command response message: {error}")))?;
    guard
        .add_note(
            &prepared.session_id,
            decision.nous_id,
            COMMAND_RECORD_CATEGORY,
            &record_json,
        )
        .map_err(|error| AletheiaError::msg(format!("append command lifecycle note: {error}")))?;
    guard
        .ensure_durable()
        .map_err(|error| AletheiaError::msg(format!("sync command lifecycle note: {error}")))?;

    Ok(())
}

fn find_command_record_by_idempotency_key(
    store: &mneme::store::SessionStore,
    session_id: &str,
    idempotency_key: &str,
) -> AletheiaResult<Option<CommandLifecycleRecord>> {
    let notes = store
        .get_notes(session_id)
        .map_err(|error| AletheiaError::msg(format!("read session notes: {error}")))?;
    Ok(notes
        .into_iter()
        .filter(|note| note.category == COMMAND_RECORD_CATEGORY)
        .filter_map(|note| serde_json::from_str::<CommandLifecycleRecord>(&note.content).ok())
        .find(|record| {
            record.record_type == "agora_command" && record.idempotency_key == idempotency_key
        }))
}

fn build_command_lifecycle_record(
    prepared: &PreparedCommandRecord,
    msg: &InboundMessage,
    cmd: &command::Command,
    decision: &RouteDecision<'_>,
    reply_text: &str,
    duration: Duration,
    created_at: String,
) -> CommandLifecycleRecord {
    CommandLifecycleRecord {
        version: COMMAND_RECORD_VERSION,
        record_type: "agora_command".to_owned(),
        idempotency_key: prepared.idempotency_key.clone(),
        session_id: prepared.session_id.clone(),
        nous_id: decision.nous_id.to_owned(),
        session_key: decision.session_key.clone(),
        origin: CommandOriginRecord {
            channel: msg.channel.clone(),
            sender: msg.sender.clone(),
            sender_name: msg.sender_name.clone(),
            group_id: msg.group_id.clone(),
            thread_id: extract_raw_thread_id(msg.raw.as_ref()),
            transport_message_id: extract_raw_message_id(msg.raw.as_ref()),
            inbound_timestamp_ms: msg.timestamp,
            reply_target: reply_target(msg),
        },
        command: CommandInvocationRecord {
            name: cmd.name().to_owned(),
            arguments_redacted: command_arguments_redacted(&msg.text),
        },
        result: CommandResultRecord {
            status: command_response_status(cmd),
            duration_ms: duration.as_millis(),
            failure_class: command_failure_class(cmd).map(str::to_owned),
            reply_text: reply_text.to_owned(),
        },
        created_at,
    }
}

fn command_response_status(cmd: &command::Command) -> CommandResponseStatus {
    match cmd {
        command::Command::Unknown { .. } => CommandResponseStatus::Failed,
        _ => CommandResponseStatus::Success,
    }
}

fn command_failure_class(cmd: &command::Command) -> Option<&'static str> {
    match cmd {
        command::Command::Unknown { .. } => Some("unknown_command"),
        _ => None,
    }
}

fn estimate_tokens(content: &str) -> i64 {
    let len = i64::try_from(content.len()).unwrap_or(i64::MAX - 3);
    len.saturating_add(3) / 4
}

fn command_idempotency_key(
    msg: &InboundMessage,
    cmd: &command::Command,
    decision: &RouteDecision<'_>,
) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "agora-command-v1");
    hash_field(&mut hasher, &msg.channel);
    hash_field(&mut hasher, &msg.sender);
    hash_field(&mut hasher, msg.group_id.as_deref().unwrap_or(""));
    hash_field(&mut hasher, decision.nous_id);
    hash_field(&mut hasher, &decision.session_key);
    hash_field(&mut hasher, cmd.name());
    hash_field(&mut hasher, &msg.text);
    hash_field(&mut hasher, &msg.timestamp.to_string());
    hash_field(
        &mut hasher,
        extract_raw_message_id(msg.raw.as_ref())
            .as_deref()
            .unwrap_or(""),
    );
    hex_encode_digest(hasher.finalize())
}

fn hex_encode_digest(digest: impl IntoIterator<Item = u8>) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(hex_digit(byte >> 4));
        encoded.push(hex_digit(byte & 0x0f));
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => '0',
    }
}

fn hash_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
    hasher.update(b";");
}

fn command_arguments_redacted(text: &str) -> String {
    let without_bang = text.trim().strip_prefix('!').unwrap_or(text).trim();
    let args = without_bang
        .split_once(char::is_whitespace)
        .map_or("", |(_name, rest)| rest.trim());
    redact_command_arguments(args)
}

fn redact_command_arguments(args: &str) -> String {
    let mut redacted = Vec::new();
    let mut redact_next = false;

    for token in args.split_whitespace() {
        if redact_next {
            redacted.push("[REDACTED]".to_owned());
            redact_next = false;
            continue;
        }

        if let Some((key, _value)) = token.split_once('=')
            && is_sensitive_argument_name(key)
        {
            redacted.push(format!("{key}=[REDACTED]"));
            continue;
        }

        if is_sensitive_argument_name(token) {
            redacted.push(token.to_owned());
            redact_next = true;
            continue;
        }

        if looks_like_secret_token(token) {
            redacted.push("[REDACTED]".to_owned());
            continue;
        }

        redacted.push(token.to_owned());
    }

    redacted.join(" ")
}

fn is_sensitive_argument_name(name: &str) -> bool {
    let normalized = name
        .trim_start_matches('-')
        .trim_end_matches(':')
        .to_ascii_lowercase();
    normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("api_key")
        || normalized.contains("apikey")
        || normalized == "key"
}

fn looks_like_secret_token(token: &str) -> bool {
    token.len() >= 48
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn extract_raw_message_id(raw: Option<&serde_json::Value>) -> Option<String> {
    let raw = raw?;
    raw_path_to_string(raw, &["event_id"])
        .or_else(|| raw_path_to_string(raw, &["eventId"]))
        .or_else(|| raw_path_to_string(raw, &["message_id"]))
        .or_else(|| raw_path_to_string(raw, &["messageId"]))
        .or_else(|| raw_path_to_string(raw, &["id"]))
        .or_else(|| raw_path_to_string(raw, &["dataMessage", "timestamp"]))
        .or_else(|| raw_path_to_string(raw, &["timestamp"]))
}

fn extract_raw_thread_id(raw: Option<&serde_json::Value>) -> Option<String> {
    let raw = raw?;
    raw_path_to_string(raw, &["thread_id"])
        .or_else(|| raw_path_to_string(raw, &["threadId"]))
        .or_else(|| raw_path_to_string(raw, &["content", "m.relates_to", "event_id"]))
}

fn raw_path_to_string(raw: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = raw;
    for segment in path {
        current = current.get(*segment)?;
    }
    match current {
        serde_json::Value::String(value) if !value.is_empty() => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
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
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex as StdMutex};

    #[cfg(feature = "recall")]
    use std::collections::HashMap;

    use agora::types::{ChannelCapabilities, ChannelProvider, ProbeResult, SendResult};
    use hermeneus::provider::ProviderRegistry;
    use hermeneus::test_utils::MockProvider;
    use mneme::store::SessionStore;
    use nous::adapters::SessionBlackboardAdapter;
    use nous::config::{NousConfig, NousGenerationConfig, PipelineConfig};
    use nous::manager::NousManager;
    use organon::registry::ToolRegistry;
    use organon::types::{BlackboardStore, ToolHttpClients, ToolServices};
    use taxis::config::ChannelBinding;
    use taxis::oikos::Oikos;
    use tokio::sync::Mutex;
    use tokio::task::JoinSet;
    use tokio_util::sync::CancellationToken;

    use super::*;

    static TEST_CAPABILITIES: ChannelCapabilities = ChannelCapabilities {
        threads: false,
        reactions: false,
        typing: false,
        media: false,
        streaming: false,
        rich_formatting: false,
        max_text_length: 4000,
    };

    struct RecordingProvider {
        sent: Arc<StdMutex<Vec<SendParams>>>,
    }

    impl ChannelProvider for RecordingProvider {
        fn id(&self) -> &'static str {
            "signal"
        }

        fn name(&self) -> &'static str {
            "Signal Test"
        }

        fn capabilities(&self) -> &ChannelCapabilities {
            &TEST_CAPABILITIES
        }

        fn send<'a>(
            &'a self,
            params: &'a SendParams,
        ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
            Box::pin(async move {
                self.sent
                    .lock()
                    .expect("sent messages lock")
                    .push(params.clone());
                SendResult::ok()
            })
        }

        fn listen(
            &self,
            _poll_interval: Option<std::time::Duration>,
            _cancel: CancellationToken,
        ) -> (tokio::sync::mpsc::Receiver<InboundMessage>, JoinSet<()>) {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            (rx, JoinSet::new())
        }

        fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
            Box::pin(async {
                ProbeResult {
                    ok: true,
                    latency_ms: None,
                    error: None,
                    details: None,
                }
            })
        }
    }

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

    fn make_command_router() -> Arc<MessageRouter> {
        Arc::new(MessageRouter::new(
            vec![ChannelBinding {
                channel: "signal".to_owned(),
                source: "*".to_owned(),
                nous_id: "alice".to_owned(),
                session_key: "signal:{source}".to_owned(),
            }],
            None,
        ))
    }

    fn make_recording_channel_registry(
        sent: Arc<StdMutex<Vec<SendParams>>>,
    ) -> Arc<ChannelRegistry> {
        let mut registry = ChannelRegistry::new();
        registry
            .register(Arc::new(RecordingProvider { sent }))
            .expect("register recording provider");
        Arc::new(registry)
    }

    fn command_message(text: &str, timestamp: u64, event_id: &str) -> InboundMessage {
        InboundMessage {
            channel: "signal".to_owned(),
            sender: "+15550100".to_owned(),
            sender_name: Some("Alice".to_owned()),
            group_id: Some("group-alpha".to_owned()),
            text: text.to_owned(),
            timestamp,
            attachments: Vec::new(),
            raw: Some(serde_json::json!({
                "event_id": event_id,
                "thread_id": "thread-1",
            })),
        }
    }

    fn only_session_id(store: &SessionStore) -> String {
        let sessions = store
            .list_sessions(Some("alice"))
            .expect("list alice sessions");
        assert_eq!(sessions.len(), 1);
        sessions.into_iter().next().expect("one session").id
    }

    fn command_records(store: &SessionStore, session_id: &str) -> Vec<CommandLifecycleRecord> {
        store
            .get_notes(session_id)
            .expect("get notes")
            .into_iter()
            .filter_map(|note| serde_json::from_str::<CommandLifecycleRecord>(&note.content).ok())
            .collect()
    }

    #[cfg(feature = "recall")]
    fn make_dispatch_manager(
        oikos: Arc<Oikos>,
        session_store: Option<Arc<Mutex<SessionStore>>>,
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
            session_store,
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
        session_store: Option<Arc<Mutex<SessionStore>>>,
        tool_services: Option<Arc<ToolServices>>,
    ) -> NousManager {
        NousManager::new(
            make_providers(),
            Arc::new(ToolRegistry::new()),
            oikos,
            None,
            None,
            session_store,
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
    async fn command_dispatch_persists_successful_command_turn() {
        let (_dir, oikos) = make_oikos();
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory session store"),
        ));
        let mgr = Arc::new(make_dispatch_manager(
            oikos,
            Some(Arc::clone(&session_store)),
            None,
        ));
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let channels = make_recording_channel_registry(Arc::clone(&sent));
        let msg = command_message("!ping", 1_000, "evt-success");

        dispatch_one(msg, make_command_router(), mgr, channels).await;

        let store = session_store.lock().await;
        let session_id = only_session_id(&store);
        let history = store.get_history(&session_id, None).expect("history");
        assert_eq!(history.len(), 2);
        let mut history_iter = history.iter();
        let user = history_iter.next().expect("user command message");
        let assistant = history_iter.next().expect("assistant command response");
        assert_eq!(user.role, Role::User);
        assert_eq!(user.content, "!ping");
        assert_eq!(assistant.role, Role::Assistant);
        assert!(
            assistant
                .content
                .contains("Agent 'alice' is not responding.")
        );

        let records = command_records(&store, &session_id);
        assert_eq!(records.len(), 1);
        let record = records.into_iter().next().expect("command record");
        assert_eq!(record.record_type, "agora_command");
        assert_eq!(record.nous_id, "alice");
        assert_eq!(record.session_key, "signal:+15550100");
        assert_eq!(record.origin.channel, "signal");
        assert_eq!(record.origin.sender, "+15550100");
        assert_eq!(record.origin.group_id.as_deref(), Some("group-alpha"));
        assert_eq!(record.origin.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(
            record.origin.transport_message_id.as_deref(),
            Some("evt-success")
        );
        assert_eq!(record.command.name, "ping");
        assert_eq!(record.command.arguments_redacted, "");
        assert_eq!(record.result.status, CommandResponseStatus::Success);
        assert!(record.result.failure_class.is_none());
        assert!(record.result.reply_text.contains("not responding"));
        drop(store);

        let sent = sent.lock().expect("sent messages lock");
        assert_eq!(sent.len(), 1);
        let sent_reply = sent.iter().next().expect("sent reply");
        assert_eq!(sent_reply.to, "group:group-alpha");
        assert_eq!(sent_reply.text, record.result.reply_text);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn command_dispatch_persists_failed_unknown_command() {
        let (_dir, oikos) = make_oikos();
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory session store"),
        ));
        let mgr = Arc::new(make_dispatch_manager(
            oikos,
            Some(Arc::clone(&session_store)),
            None,
        ));
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let channels = make_recording_channel_registry(Arc::clone(&sent));
        let msg = command_message("!launch --token secret-value keep", 2_000, "evt-failed");

        dispatch_one(msg, make_command_router(), mgr, channels).await;

        let store = session_store.lock().await;
        let session_id = only_session_id(&store);
        let history = store.get_history(&session_id, None).expect("history");
        assert_eq!(history.len(), 2);
        let records = command_records(&store, &session_id);
        assert_eq!(records.len(), 1);
        let record = records.into_iter().next().expect("command record");
        assert_eq!(record.command.name, "launch");
        assert_eq!(record.command.arguments_redacted, "--token [REDACTED] keep");
        assert_eq!(record.result.status, CommandResponseStatus::Failed);
        assert_eq!(
            record.result.failure_class.as_deref(),
            Some("unknown_command")
        );
        assert!(
            record
                .result
                .reply_text
                .contains("Unknown command '!launch'")
        );
        drop(store);

        let sent = sent.lock().expect("sent messages lock");
        assert_eq!(sent.len(), 1);
        let sent_reply = sent.iter().next().expect("sent reply");
        assert_eq!(sent_reply.text, record.result.reply_text);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn command_dispatch_replays_duplicate_without_new_history() {
        let (_dir, oikos) = make_oikos();
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory session store"),
        ));
        let mgr = Arc::new(make_dispatch_manager(
            oikos,
            Some(Arc::clone(&session_store)),
            None,
        ));
        let sent = Arc::new(StdMutex::new(Vec::new()));
        let channels = make_recording_channel_registry(Arc::clone(&sent));
        let router = make_command_router();
        let msg = command_message("!ping", 3_000, "evt-duplicate");

        dispatch_one(
            msg.clone(),
            Arc::clone(&router),
            Arc::clone(&mgr),
            Arc::clone(&channels),
        )
        .await;
        dispatch_one(msg, router, mgr, channels).await;

        let store = session_store.lock().await;
        let session_id = only_session_id(&store);
        let history = store.get_history(&session_id, None).expect("history");
        assert_eq!(
            history.len(),
            2,
            "duplicate delivery must not append a second command turn"
        );
        let records = command_records(&store, &session_id);
        assert_eq!(
            records.len(),
            1,
            "duplicate delivery must reuse the first lifecycle record"
        );
        drop(store);

        let sent = sent.lock().expect("sent messages lock");
        assert_eq!(sent.len(), 2);
        let mut sent_iter = sent.iter();
        let first = sent_iter.next().expect("first send");
        let second = sent_iter.next().expect("second send");
        assert_eq!(first.text, second.text);
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
        let mgr =
            make_dispatch_manager(oikos, Some(Arc::clone(&session_store)), Some(tool_services));

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
        let mgr = make_dispatch_manager(oikos, None, None);

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
}
