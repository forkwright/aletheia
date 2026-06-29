use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use agora::command;
use agora::router::reply_target;
use agora::types::InboundMessage;
use koina::id::SessionId;
use koina::redact::redact_sensitive;
use mneme::store::{FinalizeMessage, FinalizeNote, FinalizeTurnRequest, SessionStore};
use mneme::types::Role;

const COMMAND_NOTE_CATEGORY: &str = "context";
const COMMAND_RECORD_KIND: &str = "dispatch_command";

#[derive(Debug, Clone)]
pub(super) struct CommandRecordInput {
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
    pub(super) fn from_message(
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
pub(super) enum CommandRecordStart {
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
enum CommandLifecycleStatus {
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "duplicate_replayed")]
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
pub(super) struct CommandOutcome {
    status: CommandLifecycleStatus,
    response_status: &'static str,
    failure_class: Option<&'static str>,
    duration: Duration,
}

impl CommandOutcome {
    pub(super) fn from_command(cmd: &command::Command, duration: Duration) -> Self {
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

pub(super) async fn begin_command_record(
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

pub(super) async fn finish_command_record(
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

#[cfg(test)]
mod tests;
