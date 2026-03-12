use super::*;

#[test]
fn role_serde_roundtrip() {
    for role in [Role::System, Role::User, Role::Assistant] {
        let json = serde_json::to_string(&role).unwrap();
        let back: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn stop_reason_serde_roundtrip() {
    for reason in [
        StopReason::EndTurn,
        StopReason::ToolUse,
        StopReason::MaxTokens,
        StopReason::StopSequence,
    ] {
        let json = serde_json::to_string(&reason).unwrap();
        let back: StopReason = serde_json::from_str(&json).unwrap();
        assert_eq!(reason, back);
    }
}

#[test]
fn content_text_extraction() {
    let text = Content::Text("hello world".to_owned());
    assert_eq!(text.text(), "hello world");

    let blocks = Content::Blocks(vec![
        ContentBlock::Thinking {
            thinking: "let me think".to_owned(),
            signature: None,
        },
        ContentBlock::Text {
            text: "the answer is 42".to_owned(),
            citations: None,
        },
    ]);
    assert!(blocks.text().contains("let me think"));
    assert!(blocks.text().contains("the answer is 42"));
}

#[test]
fn tool_use_block_serde() {
    let block = ContentBlock::ToolUse {
        id: "tool_123".to_owned(),
        name: "exec".to_owned(),
        input: serde_json::json!({"command": "ls"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("tool_use"));
    assert!(json.contains("exec"));

    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "tool_123");
            assert_eq!(name, "exec");
        }
        _ => panic!("expected ToolUse"),
    }
}

#[test]
fn tool_result_block_serde() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tool_123".to_owned(),
        content: ToolResultContent::text("file.txt\ndir/"),
        is_error: Some(false),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tool_123");
            assert_eq!(content.text_summary(), "file.txt\ndir/");
            assert_eq!(is_error, Some(false));
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn tool_result_text_serializes_as_string() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "t1".to_owned(),
        content: ToolResultContent::text("hello"),
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        v["content"].is_string(),
        "Text should serialize as bare string"
    );
    assert_eq!(v["content"], "hello");
}

#[test]
fn tool_result_blocks_serializes_as_array() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "t1".to_owned(),
        content: ToolResultContent::blocks(vec![
            ToolResultBlock::Text {
                text: "description".to_owned(),
            },
            ToolResultBlock::Image {
                source: ImageSource {
                    source_type: "base64".to_owned(),
                    media_type: "image/png".to_owned(),
                    data: "iVBOR".to_owned(),
                },
            },
        ]),
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["content"].is_array(), "Blocks should serialize as array");
    assert_eq!(v["content"].as_array().unwrap().len(), 2);
    assert_eq!(v["content"][0]["type"], "text");
    assert_eq!(v["content"][1]["type"], "image");
}

#[test]
fn tool_result_content_text_deserializes_from_string() {
    let json = r#"{"type":"tool_result","tool_use_id":"t1","content":"hello"}"#;
    let block: ContentBlock = serde_json::from_str(json).unwrap();
    match block {
        ContentBlock::ToolResult { content, .. } => {
            assert_eq!(content.text_summary(), "hello");
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn tool_result_content_blocks_deserializes_from_array() {
    let json = r#"{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"hi"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"abc"}}]}"#;
    let block: ContentBlock = serde_json::from_str(json).unwrap();
    match block {
        ContentBlock::ToolResult { content, .. } => {
            assert_eq!(content.text_summary(), "hi\n[image]");
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn image_source_serde_roundtrip() {
    let source = ImageSource {
        source_type: "base64".to_owned(),
        media_type: "image/png".to_owned(),
        data: "iVBOR".to_owned(),
    };
    let json = serde_json::to_string(&source).unwrap();
    let back: ImageSource = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_type, "base64");
    assert_eq!(back.media_type, "image/png");
    assert_eq!(back.data, "iVBOR");
}

#[test]
fn document_source_serde_roundtrip() {
    let source = DocumentSource {
        source_type: "base64".to_owned(),
        media_type: "application/pdf".to_owned(),
        data: "JVBERi0".to_owned(),
    };
    let json = serde_json::to_string(&source).unwrap();
    let back: DocumentSource = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_type, "base64");
    assert_eq!(back.media_type, "application/pdf");
    assert_eq!(back.data, "JVBERi0");
}

#[test]
fn tool_result_content_from_string() {
    let content: ToolResultContent = "hello".to_owned().into();
    assert_eq!(content.text_summary(), "hello");
}

#[test]
fn usage_total() {
    let usage = Usage {
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 800,
        cache_write_tokens: 200,
    };
    assert_eq!(usage.total(), 1500);
}

#[test]
fn citation_char_location_serde() {
    let citation = Citation::CharLocation {
        document_index: 0,
        start_char_index: 10,
        end_char_index: 50,
        cited_text: "some text".to_owned(),
    };
    let json = serde_json::to_string(&citation).unwrap();
    let back: Citation = serde_json::from_str(&json).unwrap();
    match back {
        Citation::CharLocation {
            document_index,
            start_char_index,
            ..
        } => {
            assert_eq!(document_index, 0);
            assert_eq!(start_char_index, 10);
        }
        _ => panic!("expected CharLocation"),
    }
}

#[test]
fn thinking_signature_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "deep thoughts".to_owned(),
        signature: Some("sig_xyz".to_owned()),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "deep thoughts");
            assert_eq!(signature.as_deref(), Some("sig_xyz"));
        }
        _ => panic!("expected Thinking"),
    }
}

#[test]
fn thinking_no_signature_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "brief".to_owned(),
        signature: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::Thinking { signature, .. } => {
            assert!(signature.is_none());
        }
        _ => panic!("expected Thinking"),
    }
}

#[test]
fn server_tool_use_block_serde() {
    let block = ContentBlock::ServerToolUse {
        id: "srvtoolu_123".to_owned(),
        name: "web_search".to_owned(),
        input: serde_json::json!({"query": "rust async"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("server_tool_use"));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::ServerToolUse { id, name, input } => {
            assert_eq!(id, "srvtoolu_123");
            assert_eq!(name, "web_search");
            assert_eq!(input["query"], "rust async");
        }
        _ => panic!("expected ServerToolUse"),
    }
}

#[test]
fn web_search_tool_result_block_serde() {
    let block = ContentBlock::WebSearchToolResult {
        tool_use_id: "srvtoolu_123".to_owned(),
        content: serde_json::json!([
            {"type": "web_search_result", "url": "https://example.com", "title": "Example", "encrypted_content": "abc"}
        ]),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("web_search_tool_result"));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
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
fn server_tool_definition_serde() {
    let def = ServerToolDefinition {
        tool_type: "web_search_20250305".to_owned(),
        name: "web_search".to_owned(),
        max_uses: Some(5),
        allowed_domains: None,
        blocked_domains: None,
        user_location: None,
    };
    let json = serde_json::to_string(&def).unwrap();
    assert!(json.contains("web_search_20250305"));
    let back: ServerToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_type, "web_search_20250305");
    assert_eq!(back.max_uses, Some(5));
}

#[test]
fn completion_request_default() {
    let req = CompletionRequest::default();
    assert!(req.model.is_empty());
    assert!(req.system.is_none());
    assert!(req.messages.is_empty());
    assert_eq!(req.max_tokens, 4096);
    assert!(req.server_tools.is_empty());
    assert!(!req.cache_system);
    assert!(!req.cache_tools);
    assert!(req.tool_choice.is_none());
    assert!(req.metadata.is_none());
    assert!(req.citations.is_none());
}

#[test]
fn tool_choice_serde() {
    let auto = ToolChoice::Auto;
    let json = serde_json::to_string(&auto).unwrap();
    assert!(json.contains("\"type\":\"auto\""));

    let tool = ToolChoice::Tool {
        name: "exec".to_owned(),
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("\"type\":\"tool\""));
    assert!(json.contains("\"name\":\"exec\""));
}

#[test]
fn text_block_with_citations_serde() {
    let block = ContentBlock::Text {
        text: "cited text".to_owned(),
        citations: Some(vec![Citation::CharLocation {
            document_index: 0,
            start_char_index: 0,
            end_char_index: 10,
            cited_text: "source".to_owned(),
        }]),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::Text { citations, .. } => {
            assert_eq!(citations.unwrap().len(), 1);
        }
        _ => panic!("expected Text"),
    }
}

#[test]
fn completion_response_serde() {
    let response = CompletionResponse {
        id: "msg_123".to_owned(),
        model: "claude-opus-4-20250514".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: "Hello!".to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Usage::default()
        },
    };
    let json = serde_json::to_string(&response).unwrap();
    let back: CompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "msg_123");
    assert_eq!(back.stop_reason, StopReason::EndTurn);
}

#[test]
fn cache_control_type_serde() {
    let cc = CacheControl::ephemeral();
    let json = serde_json::to_string(&cc).unwrap();
    assert!(json.contains("\"type\":\"ephemeral\""));
    let back: CacheControl = serde_json::from_str(&json).unwrap();
    assert_eq!(back.kind, CacheControlType::Ephemeral);
}

#[test]
fn caching_config_defaults() {
    let config = CachingConfig::default();
    assert!(config.enabled);
    assert_eq!(config.strategy, CachingStrategy::Auto);
}

#[test]
fn caching_strategy_serde_roundtrip() {
    for strategy in [CachingStrategy::Auto, CachingStrategy::Disabled] {
        let json = serde_json::to_string(&strategy).unwrap();
        let back: CachingStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(strategy, back);
    }
}

#[test]
fn completion_request_cache_defaults() {
    let req = CompletionRequest::default();
    assert!(!req.cache_system);
    assert!(!req.cache_tools);
    assert!(!req.cache_turns);
}

#[test]
fn code_execution_result_block_serde() {
    let block = ContentBlock::CodeExecutionResult {
        code: "print('hello')".to_owned(),
        stdout: "hello\n".to_owned(),
        stderr: String::new(),
        return_code: 0,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("code_execution_result"));
    assert!(json.contains("print('hello')"));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        ContentBlock::CodeExecutionResult {
            code,
            stdout,
            stderr,
            return_code,
        } => {
            assert_eq!(code, "print('hello')");
            assert_eq!(stdout, "hello\n");
            assert!(stderr.is_empty());
            assert_eq!(return_code, 0);
        }
        _ => panic!("expected CodeExecutionResult"),
    }
}

#[test]
fn code_execution_result_nonzero_return_code() {
    let json = r#"{"type":"code_execution_result","code":"exit(1)","stdout":"","stderr":"error","return_code":1}"#;
    let block: ContentBlock = serde_json::from_str(json).unwrap();
    match block {
        ContentBlock::CodeExecutionResult {
            return_code,
            stderr,
            ..
        } => {
            assert_eq!(return_code, 1);
            assert_eq!(stderr, "error");
        }
        _ => panic!("expected CodeExecutionResult"),
    }
}

#[test]
fn tool_definition_disable_passthrough_serde() {
    let def = ToolDefinition {
        name: "exec".to_owned(),
        description: "Execute".to_owned(),
        input_schema: serde_json::json!({"type": "object"}),
        disable_passthrough: Some(true),
    };
    let json = serde_json::to_string(&def).unwrap();
    assert!(json.contains("\"disable_passthrough\":true"));
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(back.disable_passthrough, Some(true));
}

#[test]
fn tool_definition_disable_passthrough_none_omitted() {
    let def = ToolDefinition {
        name: "exec".to_owned(),
        description: "Execute".to_owned(),
        input_schema: serde_json::json!({"type": "object"}),
        disable_passthrough: None,
    };
    let json = serde_json::to_string(&def).unwrap();
    assert!(
        !json.contains("disable_passthrough"),
        "None should be omitted"
    );
}

#[test]
fn code_execution_server_tool_definition_serde() {
    let def = ServerToolDefinition {
        tool_type: "code_execution_20250522".to_owned(),
        name: "code_execution".to_owned(),
        max_uses: None,
        allowed_domains: None,
        blocked_domains: None,
        user_location: None,
    };
    let json = serde_json::to_string(&def).unwrap();
    assert!(json.contains("code_execution_20250522"));
    let back: ServerToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_type, "code_execution_20250522");
    assert_eq!(back.name, "code_execution");
}

// --- Proptest serde roundtrip tests ---
//
// Per project standards (docs/STANDARDS.md): every type that implements
// Serialize + Deserialize gets a roundtrip property test.

mod proptests {
    use proptest::prelude::*;

    use super::super::*;

    proptest! {
        /// Role serializes to a lowercase JSON string and deserializes back identically.
        #[test]
        fn role_serde_roundtrip_prop(role in prop_oneof![
            Just(Role::System),
            Just(Role::User),
            Just(Role::Assistant),
        ]) {
            let json = serde_json::to_string(&role).unwrap();
            let back: Role = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(role, back);
        }

        /// StopReason serializes and deserializes without loss.
        #[test]
        fn stop_reason_roundtrip_prop(reason in prop_oneof![
            Just(StopReason::EndTurn),
            Just(StopReason::ToolUse),
            Just(StopReason::MaxTokens),
            Just(StopReason::StopSequence),
        ]) {
            let json = serde_json::to_string(&reason).unwrap();
            let back: StopReason = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(reason, back);
        }

        /// Content::Text round-trips through JSON without data loss.
        #[test]
        fn content_text_roundtrip(text in "\\PC{0,500}") {
            let content = Content::Text(text.clone());
            let json = serde_json::to_string(&content).unwrap();
            let back: Content = serde_json::from_str(&json).unwrap();
            match back {
                Content::Text(s) => prop_assert_eq!(s, text),
                Content::Blocks(_) => prop_assert!(false, "expected Text variant"),
            }
        }

        /// Message round-trips through JSON for all role variants.
        #[test]
        fn message_roundtrip(
            role in prop_oneof![
                Just(Role::User),
                Just(Role::Assistant),
            ],
            text in "\\PC{0,200}",
        ) {
            let msg = Message { role, content: Content::Text(text) };
            let json = serde_json::to_string(&msg).unwrap();
            let back: Message = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(back.role, msg.role);
            prop_assert_eq!(back.content.text(), msg.content.text());
        }

        /// Usage.total() equals input_tokens + output_tokens regardless of cache fields.
        #[test]
        fn usage_total_prop(
            input in 0_u64..=100_000,
            output in 0_u64..=100_000,
            cache_read in 0_u64..=50_000,
            cache_write in 0_u64..=50_000,
        ) {
            let usage = Usage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_write_tokens: cache_write,
            };
            prop_assert_eq!(usage.total(), input + output);
        }

        /// ToolChoice::Auto and ::Any round-trip through JSON.
        #[test]
        fn tool_choice_auto_any_roundtrip(auto in proptest::bool::ANY) {
            let choice = if auto { ToolChoice::Auto } else { ToolChoice::Any };
            let json = serde_json::to_string(&choice).unwrap();
            let back: ToolChoice = serde_json::from_str(&json).unwrap();
            // Verify the tag was preserved by checking the JSON contains the right type string.
            if auto {
                prop_assert!(json.contains("\"auto\""), "expected 'auto' in {json}");
                prop_assert!(matches!(back, ToolChoice::Auto));
            } else {
                prop_assert!(json.contains("\"any\""), "expected 'any' in {json}");
                prop_assert!(matches!(back, ToolChoice::Any));
            }
        }

        /// ToolChoice::Tool round-trips preserving the name field.
        #[test]
        fn tool_choice_tool_roundtrip(name in "[a-zA-Z_]{1,50}") {
            let choice = ToolChoice::Tool { name: name.clone() };
            let json = serde_json::to_string(&choice).unwrap();
            let back: ToolChoice = serde_json::from_str(&json).unwrap();
            match back {
                ToolChoice::Tool { name: n } => prop_assert_eq!(n, name),
                other => prop_assert!(false, "expected Tool variant, got {other:?}"),
            }
        }
    }
}
