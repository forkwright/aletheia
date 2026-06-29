#![expect(clippy::expect_used, reason = "test assertions")]

use std::time::Duration;

use agora::command;
use agora::types::InboundMessage;
use mneme::store::SessionStore;
use mneme::types::Role;

use super::*;

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
    let msg = command_message("!bogus api_key=test-redaction-token-123456", 101, "msg-2");
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
