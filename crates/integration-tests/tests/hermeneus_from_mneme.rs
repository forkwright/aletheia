//! Cross-crate tests converting mneme types to hermeneus types.
#![cfg(feature = "sqlite-tests")]

use aletheia_hermeneus::types as h;
use aletheia_mneme::store::SessionStore;
use aletheia_mneme::types as m;

fn convert_role(role: m::Role) -> h::Role {
    match role {
        m::Role::System => h::Role::System,
        m::Role::User | m::Role::ToolResult => h::Role::User,
        m::Role::Assistant => h::Role::Assistant,
    }
}

fn convert_message(msg: &m::Message) -> h::Message {
    let content = match msg.role {
        m::Role::ToolResult => {
            let tool_use_id = msg.tool_call_id.clone().unwrap_or_default();
            h::Content::Blocks(vec![h::ContentBlock::ToolResult {
                tool_use_id,
                content: msg.content.clone(),
                is_error: None,
            }])
        }
        _ => h::Content::Text(msg.content.clone()),
    };

    h::Message {
        role: convert_role(msg.role),
        content,
    }
}

#[test]
fn build_completion_request_from_mneme_history() {
    let store = SessionStore::open_in_memory().unwrap();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .unwrap();

    store
        .append_message("ses-1", m::Role::User, "what is 2+2?", None, None, 20)
        .unwrap();
    store
        .append_message("ses-1", m::Role::Assistant, "4", None, None, 10)
        .unwrap();

    let history = store.get_history("ses-1", None).unwrap();
    let messages: Vec<h::Message> = history.iter().map(convert_message).collect();

    let request = h::CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: Some("You are a calculator.".to_owned()),
        messages,
        max_tokens: 1024,
        tools: vec![],
        temperature: None,
        thinking: None,
        stop_sequences: vec![],
    };

    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.messages[0].role, h::Role::User);
    assert_eq!(request.messages[1].role, h::Role::Assistant);
}

#[test]
fn tool_result_converts_to_content_block() {
    let store = SessionStore::open_in_memory().unwrap();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .unwrap();

    store
        .append_message(
            "ses-1",
            m::Role::ToolResult,
            r#"{"output": "ok"}"#,
            Some("tool_abc"),
            Some("exec"),
            30,
        )
        .unwrap();

    let history = store.get_history("ses-1", None).unwrap();
    let converted = convert_message(&history[0]);

    match &converted.content {
        h::Content::Blocks(blocks) => {
            assert_eq!(blocks.len(), 1);
            match &blocks[0] {
                h::ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => {
                    assert_eq!(tool_use_id, "tool_abc");
                    assert_eq!(content, r#"{"output": "ok"}"#);
                }
                other => panic!("expected ToolResult block, got {other:?}"),
            }
        }
        h::Content::Text(t) => panic!("expected Blocks content, got Text({t:?})"),
    }
}

#[test]
fn text_content_extraction() {
    let content = h::Content::Text("hello world".to_owned());
    assert_eq!(content.text(), "hello world");
}
