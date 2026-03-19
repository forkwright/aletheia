#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/slice indices are valid after asserting sufficient length"
)]
use super::*;
use crate::types::{
    CompletionRequest, Content, ContentBlock, Message, Role, StopReason, ThinkingConfig,
    ToolDefinition,
};

#[test]
fn wire_response_deserializes() {
    let json = r#"{
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello"}],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    }"#;
    let resp: WireResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.id, "msg_123");
    assert_eq!(resp.stop_reason, "end_turn");
    assert_eq!(resp.usage.input_tokens, 10);
    assert_eq!(resp.usage.cache_creation_input_tokens, 0);
}

#[test]
fn wire_response_with_cache_tokens() {
    let json = r#"{
        "id": "msg_456",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hi"}],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_creation_input_tokens": 200,
            "cache_read_input_tokens": 80
        }
    }"#;
    let resp: WireResponse = serde_json::from_str(json).unwrap();
    let converted = resp.into_response().unwrap();
    assert_eq!(converted.usage.cache_write_tokens, 200);
    assert_eq!(converted.usage.cache_read_tokens, 80);
}

#[test]
fn wire_content_block_tool_use() {
    let json = r#"{"type":"tool_use","id":"toolu_1","name":"exec","input":{"cmd":"ls"}}"#;
    let block: WireContentBlock = serde_json::from_str(json).unwrap();
    let converted = block.into_content_block();
    match converted {
        ContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "toolu_1");
            assert_eq!(name, "exec");
        }
        _ => panic!("expected ToolUse"),
    }
}

#[test]
fn wire_content_block_thinking() {
    let json = r#"{"type":"thinking","thinking":"let me think","signature":"sig_abc"}"#;
    let block: WireContentBlock = serde_json::from_str(json).unwrap();
    let converted = block.into_content_block();
    match converted {
        ContentBlock::Thinking { thinking, .. } => {
            assert_eq!(thinking, "let me think");
        }
        _ => panic!("expected Thinking"),
    }
}

#[test]
fn wire_error_response_deserializes() {
    let json = r#"{
        "type": "error",
        "error": {"type": "invalid_request_error", "message": "bad input"}
    }"#;
    let err: WireErrorResponse = serde_json::from_str(json).unwrap();
    assert_eq!(err.error.message, "bad input");
}

#[test]
fn wire_request_extracts_system_prompt() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: Some("You are helpful.".to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hello".to_owned()),
        }],
        max_tokens: 1024,
        tools: vec![],
        temperature: None,
        thinking: None,
        stop_sequences: vec![],
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    assert_eq!(
        wire.system,
        Some(serde_json::Value::String("You are helpful.".to_owned()))
    );
    assert_eq!(wire.messages.len(), 1);
    assert_eq!(wire.messages[0].role, "user");
}

#[test]
fn wire_request_extracts_system_from_messages() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: None,
        messages: vec![
            Message {
                role: Role::System,
                content: Content::Text("Be concise.".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
            },
        ],
        max_tokens: 1024,
        tools: vec![],
        temperature: None,
        thinking: None,
        stop_sequences: vec![],
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    assert_eq!(
        wire.system,
        Some(serde_json::Value::String("Be concise.".to_owned()))
    );
    // WHY: System messages must not appear in the messages array
    assert_eq!(wire.messages.len(), 1);
}

#[test]
fn wire_request_serializes_thinking_config() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("think hard".to_owned()),
        }],
        max_tokens: 16384,
        tools: vec![],
        temperature: None,
        thinking: Some(ThinkingConfig {
            enabled: true,
            budget_tokens: 8192,
        }),
        stop_sequences: vec![],
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let thinking = json.get("thinking").unwrap();
    assert_eq!(thinking["type"], "enabled");
    assert_eq!(thinking["budget_tokens"], 8192);
}

#[test]
fn wire_request_serializes_tools() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("run ls".to_owned()),
        }],
        max_tokens: 1024,
        tools: vec![ToolDefinition {
            name: "exec".to_owned(),
            description: "Execute a command".to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"}
                },
                "required": ["command"]
            }),
            disable_passthrough: None,
        }],
        temperature: None,
        thinking: None,
        stop_sequences: vec![],
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let tools = json["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "exec");
}

#[test]
fn wire_stream_event_deserializes() {
    let json = r#"{"type":"message_start","message":{"id":"msg_1","model":"claude-opus-4-20250514","usage":{"input_tokens":10,"output_tokens":0}}}"#;
    let event: WireStreamEvent = serde_json::from_str(json).unwrap();
    match event {
        WireStreamEvent::MessageStart { message } => {
            assert_eq!(message.id, "msg_1");
        }
        _ => panic!("expected MessageStart"),
    }
}

#[test]
fn wire_stream_delta_deserializes() {
    let json =
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
    let event: WireStreamEvent = serde_json::from_str(json).unwrap();
    match event {
        WireStreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(index, 0);
            match delta {
                WireDelta::TextDelta { text } => assert_eq!(text, "Hello"),
                _ => panic!("expected TextDelta"),
            }
        }
        _ => panic!("expected ContentBlockDelta"),
    }
}

#[test]
fn parse_stop_reason_all_variants() {
    assert_eq!(parse_stop_reason("end_turn").unwrap(), StopReason::EndTurn);
    assert_eq!(parse_stop_reason("tool_use").unwrap(), StopReason::ToolUse);
    assert_eq!(
        parse_stop_reason("max_tokens").unwrap(),
        StopReason::MaxTokens
    );
    assert_eq!(
        parse_stop_reason("stop_sequence").unwrap(),
        StopReason::StopSequence
    );
    assert!(parse_stop_reason("unknown").is_err());
}

#[test]
fn wire_request_cache_system_serializes_as_array() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: Some("You are helpful.".to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hello".to_owned()),
        }],
        max_tokens: 1024,
        cache_system: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let system = json["system"].as_array().unwrap();
    assert_eq!(system.len(), 1);
    assert_eq!(system[0]["type"], "text");
    assert_eq!(system[0]["text"], "You are helpful.");
    assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn wire_request_cache_tools_on_last_tool() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("run".to_owned()),
        }],
        max_tokens: 1024,
        tools: vec![
            ToolDefinition {
                name: "a".to_owned(),
                description: "first".to_owned(),
                input_schema: serde_json::json!({}),
                disable_passthrough: None,
            },
            ToolDefinition {
                name: "b".to_owned(),
                description: "second".to_owned(),
                input_schema: serde_json::json!({}),
                disable_passthrough: None,
            },
        ],
        cache_tools: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let tools = json["tools"].as_array().unwrap();
    assert!(tools[0].get("cache_control").is_none());
    assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");
}

#[test]
fn wire_request_tool_choice_auto() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        tool_choice: Some(crate::types::ToolChoice::Auto),
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["tool_choice"]["type"], "auto");
}

#[test]
fn wire_request_tool_choice_specific_tool() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        tool_choice: Some(crate::types::ToolChoice::Tool {
            name: "exec".to_owned(),
        }),
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["tool_choice"]["type"], "tool");
    assert_eq!(json["tool_choice"]["name"], "exec");
}

#[test]
fn wire_request_tool_choice_none_omitted() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    assert!(json.get("tool_choice").is_none());
}

#[test]
fn wire_request_metadata_serialized() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        metadata: Some(crate::types::RequestMetadata {
            user_id: Some("nous:syn:main".to_owned()),
        }),
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["metadata"]["user_id"], "nous:syn:main");
}

#[test]
fn wire_request_citations_serialized() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        citations: Some(crate::types::CitationConfig { enabled: true }),
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["citations"]["enabled"], true);
}

#[test]
fn wire_response_text_with_citations() {
    let json = r#"{
        "id": "msg_cit",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": "According to the doc...",
            "citations": [{
                "type": "char_location",
                "document_index": 0,
                "start_char_index": 0,
                "end_char_index": 150,
                "cited_text": "source text"
            }]
        }],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    }"#;
    let resp: WireResponse = serde_json::from_str(json).unwrap();
    let converted = resp.into_response().unwrap();
    match &converted.content[0] {
        ContentBlock::Text { citations, .. } => {
            let cits = citations.as_ref().unwrap();
            assert_eq!(cits.len(), 1);
        }
        _ => panic!("expected Text block"),
    }
}

#[test]
fn wire_thinking_signature_passes_through() {
    let json = r#"{"type":"thinking","thinking":"let me think","signature":"sig_abc"}"#;
    let block: WireContentBlock = serde_json::from_str(json).unwrap();
    let converted = block.into_content_block();
    match converted {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "let me think");
            assert_eq!(signature.as_deref(), Some("sig_abc"));
        }
        _ => panic!("expected Thinking"),
    }
}

#[test]
fn wire_request_mixed_user_and_server_tools() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("search for rust".to_owned()),
        }],
        max_tokens: 1024,
        tools: vec![ToolDefinition {
            name: "read".to_owned(),
            description: "Read a file".to_owned(),
            input_schema: serde_json::json!({"type": "object"}),
            disable_passthrough: None,
        }],
        server_tools: vec![crate::types::ServerToolDefinition {
            tool_type: "web_search_20250305".to_owned(),
            name: "web_search".to_owned(),
            max_uses: Some(5),
            allowed_domains: None,
            blocked_domains: None,
            user_location: None,
        }],
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let tools = json["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "read");
    assert!(tools[0].get("input_schema").is_some());
    assert!(tools[0].get("type").is_none());
    assert_eq!(tools[1]["type"], "web_search_20250305");
    assert_eq!(tools[1]["name"], "web_search");
    assert_eq!(tools[1]["max_uses"], 5);
    assert!(tools[1].get("input_schema").is_none());
}

#[test]
fn wire_content_block_server_tool_use() {
    let json = r#"{"type":"server_tool_use","id":"srvtoolu_123","name":"web_search","input":{"query":"rust async"}}"#;
    let block: WireContentBlock = serde_json::from_str(json).unwrap();
    let converted = block.into_content_block();
    match converted {
        ContentBlock::ServerToolUse { id, name, input } => {
            assert_eq!(id, "srvtoolu_123");
            assert_eq!(name, "web_search");
            assert_eq!(input["query"], "rust async");
        }
        _ => panic!("expected ServerToolUse"),
    }
}

#[test]
fn wire_content_block_web_search_tool_result() {
    let json = r#"{"type":"web_search_tool_result","tool_use_id":"srvtoolu_123","content":[{"type":"web_search_result","url":"https://example.com","title":"Example","encrypted_content":"abc"}]}"#;
    let block: WireContentBlock = serde_json::from_str(json).unwrap();
    let converted = block.into_content_block();
    match converted {
        ContentBlock::WebSearchToolResult {
            tool_use_id,
            content,
        } => {
            assert_eq!(tool_use_id, "srvtoolu_123");
            assert!(content.is_array());
        }
        _ => panic!("expected WebSearchToolResult"),
    }
}

#[test]
fn wire_response_with_server_tool_blocks() {
    let json = r#"{
        "id": "msg_srv",
        "type": "message",
        "role": "assistant",
        "content": [
            {"type": "server_tool_use", "id": "srvtoolu_1", "name": "web_search", "input": {"query": "test"}},
            {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_1", "content": []},
            {"type": "text", "text": "Based on my search..."}
        ],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    }"#;
    let resp: WireResponse = serde_json::from_str(json).unwrap();
    let converted = resp.into_response().unwrap();
    assert_eq!(converted.content.len(), 3);
    assert!(matches!(
        &converted.content[0],
        ContentBlock::ServerToolUse { .. }
    ));
    assert!(matches!(
        &converted.content[1],
        ContentBlock::WebSearchToolResult { .. }
    ));
    assert!(matches!(&converted.content[2], ContentBlock::Text { .. }));
}

#[test]
fn wire_request_cache_tools_only_on_user_tools() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        tools: vec![ToolDefinition {
            name: "read".to_owned(),
            description: "Read".to_owned(),
            input_schema: serde_json::json!({}),
            disable_passthrough: None,
        }],
        server_tools: vec![crate::types::ServerToolDefinition {
            tool_type: "web_search_20250305".to_owned(),
            name: "web_search".to_owned(),
            max_uses: Some(5),
            allowed_domains: None,
            blocked_domains: None,
            user_location: None,
        }],
        cache_tools: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let tools = json["tools"].as_array().unwrap();
    assert_eq!(tools[0]["cache_control"]["type"], "ephemeral");
    assert!(tools[1].get("cache_control").is_none());
}

#[test]
fn wire_server_tool_serializes_type_field() {
    let tool = WireServerTool {
        tool_type: "web_search_20250305",
        name: "web_search",
        max_uses: Some(5),
        allowed_domains: None,
        blocked_domains: None,
        user_location: None,
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["type"], "web_search_20250305");
    assert_eq!(json["name"], "web_search");
    assert_eq!(json["max_uses"], 5);
    assert!(json.get("allowed_domains").is_none());
}

#[test]
fn wire_server_tool_with_domain_filters() {
    let allowed = vec!["example.com".to_owned()];
    let blocked = vec!["evil.com".to_owned()];
    let tool = WireServerTool {
        tool_type: "web_search_20250305",
        name: "web_search",
        max_uses: None,
        allowed_domains: Some(&allowed),
        blocked_domains: Some(&blocked),
        user_location: None,
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["allowed_domains"][0], "example.com");
    assert_eq!(json["blocked_domains"][0], "evil.com");
    assert!(json.get("max_uses").is_none());
}

#[test]
fn wire_tool_entry_untagged_serialization() {
    let user_tool = WireToolEntry::UserDefined(WireTool {
        name: "read",
        description: "Read a file",
        input_schema: &serde_json::json!({"type": "object"}),
        cache_control: None,
        disable_passthrough: None,
    });
    let server_tool = WireToolEntry::ServerSide(WireServerTool {
        tool_type: "web_search_20250305",
        name: "web_search",
        max_uses: Some(3),
        allowed_domains: None,
        blocked_domains: None,
        user_location: None,
    });

    let user_json = serde_json::to_value(&user_tool).unwrap();
    assert!(user_json.get("type").is_none());
    assert!(user_json.get("input_schema").is_some());

    let server_json = serde_json::to_value(&server_tool).unwrap();
    assert_eq!(server_json["type"], "web_search_20250305");
    assert!(server_json.get("input_schema").is_none());
}
