use super::*;
use crate::types::{
    CacheControl, CompletionRequest, Content, ContentBlock, Message, Role, ToolDefinition,
};

#[test]
fn wire_content_block_web_search_result_with_multiple_results() {
    let json = r#"{"type":"web_search_tool_result","tool_use_id":"srvtoolu_456","content":[
        {"type":"web_search_result","url":"https://a.com","title":"A","encrypted_content":"enc1"},
        {"type":"web_search_result","url":"https://b.com","title":"B","encrypted_content":"enc2"},
        {"type":"web_search_result","url":"https://c.com","title":"C","encrypted_content":"enc3"}
    ]}"#;
    let block: WireContentBlock = serde_json::from_str(json).unwrap();
    let converted = block.into_content_block();
    match converted {
        ContentBlock::WebSearchToolResult { content, .. } => {
            assert_eq!(content.as_array().unwrap().len(), 3);
        }
        _ => panic!("expected WebSearchToolResult"),
    }
}

#[test]
fn wire_response_with_citation_web_search_result_location() {
    let json = r#"{
        "id": "msg_ws_cit",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": "According to...",
            "citations": [{
                "type": "web_search_result_location",
                "url": "https://example.com/article",
                "title": "Example Article",
                "cited_text": "relevant passage"
            }]
        }],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 100, "output_tokens": 50}
    }"#;
    let resp: WireResponse = serde_json::from_str(json).unwrap();
    let converted = resp.into_response().unwrap();
    match &converted.content[0] {
        ContentBlock::Text { citations, .. } => {
            let cits = citations.as_ref().unwrap();
            assert_eq!(cits.len(), 1);
            match &cits[0] {
                crate::types::Citation::WebSearchResultLocation {
                    url,
                    title,
                    cited_text,
                } => {
                    assert_eq!(url, "https://example.com/article");
                    assert_eq!(title.as_deref(), Some("Example Article"));
                    assert_eq!(cited_text, "relevant passage");
                }
                _ => panic!("expected WebSearchResultLocation"),
            }
        }
        _ => panic!("expected Text"),
    }
}

#[test]
fn wire_request_code_execution_server_tool() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("run python".to_owned()),
        }],
        max_tokens: 1024,
        server_tools: vec![crate::types::ServerToolDefinition {
            tool_type: "code_execution_20250522".to_owned(),
            name: "code_execution".to_owned(),
            max_uses: None,
            allowed_domains: None,
            blocked_domains: None,
            user_location: None,
        }],
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let tools = json["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "code_execution_20250522");
    assert_eq!(tools[0]["name"], "code_execution");
}

#[test]
fn wire_request_no_cache_system_is_string() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: Some("test".to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    assert!(json["system"].is_string());
    assert_eq!(json["system"], "test");
}

#[test]
fn cache_turns_marks_text_content_as_blocks() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Text("first".to_owned()),
            },
            Message {
                role: Role::Assistant,
                content: Content::Text("response".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("second".to_owned()),
            },
        ],
        max_tokens: 1024,
        cache_turns: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let msgs = json["messages"].as_array().unwrap();
    // First user message (index 0) should have cache_control
    let first_content = &msgs[0]["content"];
    assert!(first_content.is_array(), "text should be wrapped as blocks");
    assert_eq!(first_content[0]["cache_control"]["type"], "ephemeral");
    // Last message (current turn) should NOT have cache_control
    let last_content = &msgs[2]["content"];
    if last_content.is_string() {
        // plain text, no cache_control: correct
    } else {
        assert!(
            last_content[0].get("cache_control").is_none(),
            "current turn should not be cached"
        );
    }
}

#[test]
fn cache_turns_disabled_leaves_content_unchanged() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Text("first".to_owned()),
            },
            Message {
                role: Role::Assistant,
                content: Content::Text("response".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("second".to_owned()),
            },
        ],
        max_tokens: 1024,
        cache_turns: false,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let msgs = json["messages"].as_array().unwrap();
    // All content should be plain text strings (no cache_control injection)
    for msg in msgs {
        assert!(
            msg["content"].is_string(),
            "content should be plain text when cache_turns is disabled"
        );
    }
}

#[test]
fn cache_turns_single_message_no_breakpoints() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("only one".to_owned()),
        }],
        max_tokens: 1024,
        cache_turns: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let msgs = json["messages"].as_array().unwrap();
    assert!(
        msgs[0]["content"].is_string(),
        "single message should not get cache_control"
    );
}

#[test]
fn cache_turns_multi_turn_marks_recent_user_messages() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Text("turn 1".to_owned()),
            },
            Message {
                role: Role::Assistant,
                content: Content::Text("reply 1".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("turn 2".to_owned()),
            },
            Message {
                role: Role::Assistant,
                content: Content::Text("reply 2".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("turn 3".to_owned()),
            },
            Message {
                role: Role::Assistant,
                content: Content::Text("reply 3".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("turn 4 (current)".to_owned()),
            },
        ],
        max_tokens: 1024,
        cache_turns: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let msgs = json["messages"].as_array().unwrap();

    // Should have exactly MAX_TURN_CACHE_BREAKPOINTS cached user messages
    let cached_count = msgs
        .iter()
        .filter(|m| {
            let c = &m["content"];
            c.is_array()
                && c.as_array()
                    .is_some_and(|arr| arr.last().is_some_and(|b| b.get("cache_control").is_some()))
        })
        .count();
    assert_eq!(cached_count, MAX_TURN_CACHE_BREAKPOINTS);

    // Current turn (last message) should NOT be cached
    let last = msgs.last().unwrap();
    assert!(
        last["content"].is_string(),
        "current turn should not have cache_control"
    );
}

#[test]
fn cache_turns_with_block_content() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Blocks(vec![
                    ContentBlock::Text {
                        text: "block one".to_owned(),
                        citations: None,
                    },
                    ContentBlock::Text {
                        text: "block two".to_owned(),
                        citations: None,
                    },
                ]),
            },
            Message {
                role: Role::Assistant,
                content: Content::Text("ok".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("current".to_owned()),
            },
        ],
        max_tokens: 1024,
        cache_turns: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let msgs = json["messages"].as_array().unwrap();

    // First message (blocks) should have cache_control on last block
    let first_content = msgs[0]["content"].as_array().unwrap();
    assert_eq!(first_content.len(), 2);
    assert!(
        first_content[0].get("cache_control").is_none(),
        "only last block gets cache_control"
    );
    assert_eq!(first_content[1]["cache_control"]["type"], "ephemeral");
}

#[test]
fn cache_turns_never_marks_current_message() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Text("previous".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("current".to_owned()),
            },
        ],
        max_tokens: 1024,
        cache_turns: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let msgs = json["messages"].as_array().unwrap();
    // First user message should be cached
    assert!(msgs[0]["content"].is_array());
    assert_eq!(msgs[0]["content"][0]["cache_control"]["type"], "ephemeral");
    // Second (last/current) should NOT be cached
    assert!(msgs[1]["content"].is_string());
}

#[test]
fn cache_control_ephemeral_serialization() {
    let cc = CacheControl::ephemeral();
    let json = serde_json::to_value(&cc).unwrap();
    assert_eq!(json["type"], "ephemeral");
}

#[test]
fn cache_control_roundtrip() {
    let cc = CacheControl::ephemeral();
    let json = serde_json::to_string(&cc).unwrap();
    let back: CacheControl = serde_json::from_str(&json).unwrap();
    assert_eq!(cc, back);
}

#[test]
fn compute_turn_cache_indices_empty() {
    let messages: Vec<&crate::types::Message> = vec![];
    let indices = compute_turn_cache_indices(&messages);
    assert!(indices.is_empty());
}

#[test]
fn compute_turn_cache_indices_two_messages() {
    let msgs = [
        Message {
            role: Role::User,
            content: Content::Text("a".to_owned()),
        },
        Message {
            role: Role::User,
            content: Content::Text("b".to_owned()),
        },
    ];
    let refs: Vec<&Message> = msgs.iter().collect();
    let indices = compute_turn_cache_indices(&refs);
    assert_eq!(indices, vec![0]);
}

#[test]
fn compute_turn_cache_indices_respects_max_breakpoints() {
    let msgs = [
        Message {
            role: Role::User,
            content: Content::Text("a".to_owned()),
        },
        Message {
            role: Role::Assistant,
            content: Content::Text("b".to_owned()),
        },
        Message {
            role: Role::User,
            content: Content::Text("c".to_owned()),
        },
        Message {
            role: Role::Assistant,
            content: Content::Text("d".to_owned()),
        },
        Message {
            role: Role::User,
            content: Content::Text("e".to_owned()),
        },
        Message {
            role: Role::Assistant,
            content: Content::Text("f".to_owned()),
        },
        Message {
            role: Role::User,
            content: Content::Text("g".to_owned()),
        },
    ];
    let refs: Vec<&Message> = msgs.iter().collect();
    let indices = compute_turn_cache_indices(&refs);
    assert!(indices.len() <= MAX_TURN_CACHE_BREAKPOINTS);
    // Should not include the last index (current message)
    assert!(!indices.contains(&6));
}

#[test]
fn compute_turn_cache_indices_only_picks_user_messages() {
    let msgs = [
        Message {
            role: Role::User,
            content: Content::Text("a".to_owned()),
        },
        Message {
            role: Role::Assistant,
            content: Content::Text("b".to_owned()),
        },
        Message {
            role: Role::Assistant,
            content: Content::Text("c".to_owned()),
        },
        Message {
            role: Role::User,
            content: Content::Text("d".to_owned()),
        },
    ];
    let refs: Vec<&Message> = msgs.iter().collect();
    let indices = compute_turn_cache_indices(&refs);
    // Should pick user message at index 0 (the only user message before the last)
    assert_eq!(indices, vec![0]);
}

#[test]
fn wire_usage_cache_tokens_default_zero() {
    let json = r#"{
        "id": "msg_zero",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "hi"}],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    }"#;
    let resp: WireResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.usage.cache_creation_input_tokens, 0);
    assert_eq!(resp.usage.cache_read_input_tokens, 0);
    let converted = resp.into_response().unwrap();
    assert_eq!(converted.usage.cache_write_tokens, 0);
    assert_eq!(converted.usage.cache_read_tokens, 0);
}

#[test]
fn wire_usage_cache_tokens_parsed() {
    let json = r#"{
        "id": "msg_cache",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "cached"}],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_creation_input_tokens": 1500,
            "cache_read_input_tokens": 3000
        }
    }"#;
    let resp: WireResponse = serde_json::from_str(json).unwrap();
    let converted = resp.into_response().unwrap();
    assert_eq!(converted.usage.cache_write_tokens, 1500);
    assert_eq!(converted.usage.cache_read_tokens, 3000);
}

#[test]
fn content_with_cache_control_text() {
    let content = Content::Text("hello".to_owned());
    let value = content_with_cache_control(&content);
    let arr = value.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["type"], "text");
    assert_eq!(arr[0]["text"], "hello");
    assert_eq!(arr[0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn content_with_cache_control_blocks() {
    let content = Content::Blocks(vec![
        ContentBlock::Text {
            text: "a".to_owned(),
            citations: None,
        },
        ContentBlock::Text {
            text: "b".to_owned(),
            citations: None,
        },
    ]);
    let value = content_with_cache_control(&content);
    let arr = value.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // Only the last block should have cache_control
    assert!(arr[0].get("cache_control").is_none());
    assert_eq!(arr[1]["cache_control"]["type"], "ephemeral");
}

#[test]
fn cache_turns_combined_with_system_and_tools() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: Some("system prompt".to_owned()),
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Text("turn 1".to_owned()),
            },
            Message {
                role: Role::Assistant,
                content: Content::Text("reply".to_owned()),
            },
            Message {
                role: Role::User,
                content: Content::Text("turn 2".to_owned()),
            },
        ],
        max_tokens: 1024,
        tools: vec![ToolDefinition {
            name: "exec".to_owned(),
            description: "run".to_owned(),
            input_schema: serde_json::json!({}),
            disable_passthrough: None,
        }],
        cache_system: true,
        cache_tools: true,
        cache_turns: true,
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    // System is cached
    assert_eq!(json["system"][0]["cache_control"]["type"], "ephemeral");
    // Last tool is cached
    assert_eq!(json["tools"][0]["cache_control"]["type"], "ephemeral");
    // First user message is cached
    let msgs = json["messages"].as_array().unwrap();
    assert!(msgs[0]["content"].is_array());
    assert_eq!(msgs[0]["content"][0]["cache_control"]["type"], "ephemeral");
    // Current turn is not cached
    assert!(msgs[2]["content"].is_string());
}

#[test]
fn wire_content_block_code_execution_result() {
    let json = r#"{"type":"code_execution_result","code":"print(42)","stdout":"42\n","stderr":"","return_code":0}"#;
    let block: WireContentBlock = serde_json::from_str(json).unwrap();
    let converted = block.into_content_block();
    match converted {
        ContentBlock::CodeExecutionResult {
            code,
            stdout,
            stderr,
            return_code,
        } => {
            assert_eq!(code, "print(42)");
            assert_eq!(stdout, "42\n");
            assert!(stderr.is_empty());
            assert_eq!(return_code, 0);
        }
        _ => panic!("expected CodeExecutionResult"),
    }
}

#[test]
fn wire_request_disable_passthrough_serialized() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        tools: vec![ToolDefinition {
            name: "exec".to_owned(),
            description: "Execute".to_owned(),
            input_schema: serde_json::json!({"type": "object"}),
            disable_passthrough: Some(true),
        }],
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let tools = json["tools"].as_array().unwrap();
    assert_eq!(tools[0]["disable_passthrough"], true);
}

#[test]
fn wire_request_disable_passthrough_none_omitted() {
    let req = CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
        }],
        max_tokens: 1024,
        tools: vec![ToolDefinition {
            name: "exec".to_owned(),
            description: "Execute".to_owned(),
            input_schema: serde_json::json!({"type": "object"}),
            disable_passthrough: None,
        }],
        ..Default::default()
    };
    let wire = WireRequest::from_request(&req, None);
    let json = serde_json::to_value(&wire).unwrap();
    let tools = json["tools"].as_array().unwrap();
    assert!(
        tools[0].get("disable_passthrough").is_none(),
        "None should be omitted from wire format"
    );
}
