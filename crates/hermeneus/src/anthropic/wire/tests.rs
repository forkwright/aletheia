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
    let resp: WireResponse = serde_json::from_str(json).expect("wire response should deserialize");
    assert_eq!(resp.id, "msg_123", "response id should match");
    assert_eq!(
        resp.stop_reason, "end_turn",
        "stop reason should be end_turn"
    );
    assert_eq!(
        resp.usage.input_tokens, 10,
        "input token count should match"
    );
    assert_eq!(
        resp.usage.cache_creation_input_tokens, 0,
        "cache creation tokens should default to zero"
    );
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
    let resp: WireResponse =
        serde_json::from_str(json).expect("wire response with cache tokens should deserialize");
    let converted = resp
        .into_response()
        .expect("wire response should convert to response");
    assert_eq!(
        converted.usage.cache_write_tokens, 200,
        "cache write tokens should match cache_creation_input_tokens"
    );
    assert_eq!(
        converted.usage.cache_read_tokens, 80,
        "cache read tokens should match cache_read_input_tokens"
    );
}

#[test]
fn wire_content_block_tool_use() {
    let json = r#"{"type":"tool_use","id":"toolu_1","name":"exec","input":{"cmd":"ls"}}"#;
    let block: WireContentBlock =
        serde_json::from_str(json).expect("tool_use content block should deserialize");
    let converted = block.into_content_block();
    match converted {
        ContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "toolu_1", "tool use id should match");
            assert_eq!(name, "exec", "tool use name should match");
        }
        _ => panic!("expected ToolUse"),
    }
}

#[test]
fn wire_content_block_thinking() {
    let json = r#"{"type":"thinking","thinking":"let me think","signature":"sig_abc"}"#;
    let block: WireContentBlock =
        serde_json::from_str(json).expect("thinking content block should deserialize");
    let converted = block.into_content_block();
    match converted {
        ContentBlock::Thinking { thinking, .. } => {
            assert_eq!(thinking, "let me think", "thinking text should match");
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
    let err: WireErrorResponse =
        serde_json::from_str(json).expect("wire error response should deserialize");
    assert_eq!(err.error.message, "bad input", "error message should match");
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
        Some(serde_json::Value::String("You are helpful.".to_owned())),
        "system prompt should be extracted from request"
    );
    assert_eq!(
        wire.messages.len(),
        1,
        "only user message should be in messages array"
    );
    assert_eq!(wire.messages[0].role, "user", "message role should be user");
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
        Some(serde_json::Value::String("Be concise.".to_owned())),
        "system prompt should be extracted from system-role message"
    );
    // System messages must not appear in the messages array
    assert_eq!(
        wire.messages.len(),
        1,
        "system message should be removed from messages array"
    );
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
    let json = serde_json::to_value(&wire).expect("wire request should serialize to JSON");
    let thinking = json
        .get("thinking")
        .expect("thinking field should be present in serialized request");
    assert_eq!(
        thinking["type"], "enabled",
        "thinking type should be enabled"
    );
    assert_eq!(
        thinking["budget_tokens"], 8192,
        "thinking budget_tokens should match"
    );
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
    let json = serde_json::to_value(&wire).expect("wire request should serialize to JSON");
    let tools = json["tools"]
        .as_array()
        .expect("tools field should be a JSON array");
    assert_eq!(tools.len(), 1, "tools array should contain one tool");
    assert_eq!(tools[0]["name"], "exec", "tool name should match");
}

#[test]
fn wire_stream_event_deserializes() {
    let json = r#"{"type":"message_start","message":{"id":"msg_1","model":"claude-opus-4-20250514","usage":{"input_tokens":10,"output_tokens":0}}}"#;
    let event: WireStreamEvent =
        serde_json::from_str(json).expect("stream message_start event should deserialize");
    match event {
        WireStreamEvent::MessageStart { message } => {
            assert_eq!(message.id, "msg_1", "message id should match");
        }
        _ => panic!("expected MessageStart"),
    }
}

#[test]
fn wire_stream_delta_deserializes() {
    let json =
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
    let event: WireStreamEvent =
        serde_json::from_str(json).expect("stream content_block_delta event should deserialize");
    match event {
        WireStreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(index, 0, "content block index should match");
            match delta {
                WireDelta::TextDelta { text } => {
                    assert_eq!(text, "Hello", "text delta content should match");
                }
                _ => panic!("expected TextDelta"),
            }
        }
        _ => panic!("expected ContentBlockDelta"),
    }
}

#[test]
fn parse_stop_reason_all_variants() {
    assert_eq!(
        parse_stop_reason("end_turn").expect("end_turn should parse as valid stop reason"),
        StopReason::EndTurn,
        "end_turn should map to StopReason::EndTurn"
    );
    assert_eq!(
        parse_stop_reason("tool_use").expect("tool_use should parse as valid stop reason"),
        StopReason::ToolUse,
        "tool_use should map to StopReason::ToolUse"
    );
    assert_eq!(
        parse_stop_reason("max_tokens").expect("max_tokens should parse as valid stop reason"),
        StopReason::MaxTokens,
        "max_tokens should map to StopReason::MaxTokens"
    );
    assert_eq!(
        parse_stop_reason("stop_sequence")
            .expect("stop_sequence should parse as valid stop reason"),
        StopReason::StopSequence,
        "stop_sequence should map to StopReason::StopSequence"
    );
    assert!(
        parse_stop_reason("unknown").is_err(),
        "unknown stop reason string should return an error"
    );
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
    let json = serde_json::to_value(&wire)
        .expect("wire request with cache_system should serialize to JSON");
    let system = json["system"]
        .as_array()
        .expect("system field should be a JSON array when cache_system is enabled");
    assert_eq!(system.len(), 1, "system array should have one entry");
    assert_eq!(
        system[0]["type"], "text",
        "system block type should be text"
    );
    assert_eq!(
        system[0]["text"], "You are helpful.",
        "system block text should match"
    );
    assert_eq!(
        system[0]["cache_control"]["type"], "ephemeral",
        "system block should have ephemeral cache_control"
    );
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
    let json = serde_json::to_value(&wire)
        .expect("wire request with cache_tools should serialize to JSON");
    let tools = json["tools"]
        .as_array()
        .expect("tools field should be a JSON array");
    assert!(
        tools[0].get("cache_control").is_none(),
        "first tool should not have cache_control"
    );
    assert_eq!(
        tools[1]["cache_control"]["type"], "ephemeral",
        "last tool should have ephemeral cache_control"
    );
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
    let json = serde_json::to_value(&wire)
        .expect("wire request with tool_choice auto should serialize to JSON");
    assert_eq!(
        json["tool_choice"]["type"], "auto",
        "tool_choice type should be auto"
    );
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
    let json = serde_json::to_value(&wire)
        .expect("wire request with specific tool_choice should serialize to JSON");
    assert_eq!(
        json["tool_choice"]["type"], "tool",
        "tool_choice type should be tool"
    );
    assert_eq!(
        json["tool_choice"]["name"], "exec",
        "tool_choice name should match"
    );
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
    let json = serde_json::to_value(&wire)
        .expect("wire request without tool_choice should serialize to JSON");
    assert!(
        json.get("tool_choice").is_none(),
        "tool_choice should be omitted when not set"
    );
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
    let json =
        serde_json::to_value(&wire).expect("wire request with metadata should serialize to JSON");
    assert_eq!(
        json["metadata"]["user_id"], "nous:syn:main",
        "metadata user_id should match"
    );
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
    let json =
        serde_json::to_value(&wire).expect("wire request with citations should serialize to JSON");
    assert_eq!(
        json["citations"]["enabled"], true,
        "citations enabled flag should be true"
    );
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
    let resp: WireResponse =
        serde_json::from_str(json).expect("wire response with citations should deserialize");
    let converted = resp
        .into_response()
        .expect("wire response should convert to response");
    match &converted.content[0] {
        ContentBlock::Text { citations, .. } => {
            let cits = citations
                .as_ref()
                .expect("citations should be present on text block");
            assert_eq!(cits.len(), 1, "text block should have one citation");
        }
        _ => panic!("expected Text block"),
    }
}

#[test]
fn wire_thinking_signature_passes_through() {
    let json = r#"{"type":"thinking","thinking":"let me think","signature":"sig_abc"}"#;
    let block: WireContentBlock = serde_json::from_str(json)
        .expect("thinking content block with signature should deserialize");
    let converted = block.into_content_block();
    match converted {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "let me think", "thinking text should match");
            assert_eq!(
                signature.as_deref(),
                Some("sig_abc"),
                "thinking signature should pass through"
            );
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
    let json = serde_json::to_value(&wire)
        .expect("wire request with mixed tools should serialize to JSON");
    let tools = json["tools"]
        .as_array()
        .expect("tools field should be a JSON array");
    assert_eq!(
        tools.len(),
        2,
        "tools array should contain both user and server tools"
    );
    // First: user-defined tool (has input_schema)
    assert_eq!(
        tools[0]["name"], "read",
        "first tool name should be the user-defined tool"
    );
    assert!(
        tools[0].get("input_schema").is_some(),
        "user-defined tool should have input_schema"
    );
    assert!(
        tools[0].get("type").is_none(),
        "user-defined tool should not have type field"
    );
    // Second: server-side tool (has type, no input_schema)
    assert_eq!(
        tools[1]["type"], "web_search_20250305",
        "server tool type field should match"
    );
    assert_eq!(
        tools[1]["name"], "web_search",
        "server tool name should match"
    );
    assert_eq!(tools[1]["max_uses"], 5, "server tool max_uses should match");
    assert!(
        tools[1].get("input_schema").is_none(),
        "server tool should not have input_schema"
    );
}

#[test]
fn wire_content_block_server_tool_use() {
    let json = r#"{"type":"server_tool_use","id":"srvtoolu_123","name":"web_search","input":{"query":"rust async"}}"#;
    let block: WireContentBlock =
        serde_json::from_str(json).expect("server_tool_use content block should deserialize");
    let converted = block.into_content_block();
    match converted {
        ContentBlock::ServerToolUse { id, name, input } => {
            assert_eq!(id, "srvtoolu_123", "server tool use id should match");
            assert_eq!(name, "web_search", "server tool use name should match");
            assert_eq!(
                input["query"], "rust async",
                "server tool use input query should match"
            );
        }
        _ => panic!("expected ServerToolUse"),
    }
}

#[test]
fn wire_content_block_web_search_tool_result() {
    let json = r#"{"type":"web_search_tool_result","tool_use_id":"srvtoolu_123","content":[{"type":"web_search_result","url":"https://example.com","title":"Example","encrypted_content":"abc"}]}"#;
    let block: WireContentBlock = serde_json::from_str(json)
        .expect("web_search_tool_result content block should deserialize");
    let converted = block.into_content_block();
    match converted {
        ContentBlock::WebSearchToolResult {
            tool_use_id,
            content,
        } => {
            assert_eq!(
                tool_use_id, "srvtoolu_123",
                "web search tool result tool_use_id should match"
            );
            assert!(
                content.is_array(),
                "web search tool result content should be a JSON array"
            );
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
    let resp: WireResponse = serde_json::from_str(json)
        .expect("wire response with server tool blocks should deserialize");
    let converted = resp
        .into_response()
        .expect("wire response should convert to response");
    assert_eq!(
        converted.content.len(),
        3,
        "response should have three content blocks"
    );
    assert!(
        matches!(&converted.content[0], ContentBlock::ServerToolUse { .. }),
        "first content block should be ServerToolUse"
    );
    assert!(
        matches!(
            &converted.content[1],
            ContentBlock::WebSearchToolResult { .. }
        ),
        "second content block should be WebSearchToolResult"
    );
    assert!(
        matches!(&converted.content[2], ContentBlock::Text { .. }),
        "third content block should be Text"
    );
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
    let json = serde_json::to_value(&wire)
        .expect("wire request with cache_tools and server tools should serialize to JSON");
    let tools = json["tools"]
        .as_array()
        .expect("tools field should be a JSON array");
    // cache_control on last user-defined tool
    assert_eq!(
        tools[0]["cache_control"]["type"], "ephemeral",
        "last user-defined tool should have ephemeral cache_control"
    );
    // server tool has no cache_control
    assert!(
        tools[1].get("cache_control").is_none(),
        "server tool should not have cache_control"
    );
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
    let json = serde_json::to_value(&tool).expect("WireServerTool should serialize to JSON");
    assert_eq!(
        json["type"], "web_search_20250305",
        "server tool type field should match tool_type"
    );
    assert_eq!(json["name"], "web_search", "server tool name should match");
    assert_eq!(json["max_uses"], 5, "server tool max_uses should match");
    assert!(
        json.get("allowed_domains").is_none(),
        "allowed_domains should be omitted when None"
    );
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
    let json = serde_json::to_value(&tool)
        .expect("WireServerTool with domain filters should serialize to JSON");
    assert_eq!(
        json["allowed_domains"][0], "example.com",
        "first allowed domain should match"
    );
    assert_eq!(
        json["blocked_domains"][0], "evil.com",
        "first blocked domain should match"
    );
    assert!(
        json.get("max_uses").is_none(),
        "max_uses should be omitted when None"
    );
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

    let user_json = serde_json::to_value(&user_tool)
        .expect("user-defined WireToolEntry should serialize to JSON");
    assert!(
        user_json.get("type").is_none(),
        "user-defined tool entry should not have a type field"
    );
    assert!(
        user_json.get("input_schema").is_some(),
        "user-defined tool entry should have input_schema"
    );

    let server_json = serde_json::to_value(&server_tool)
        .expect("server-side WireToolEntry should serialize to JSON");
    assert_eq!(
        server_json["type"], "web_search_20250305",
        "server-side tool entry type should match tool_type"
    );
    assert!(
        server_json.get("input_schema").is_none(),
        "server-side tool entry should not have input_schema"
    );
}
