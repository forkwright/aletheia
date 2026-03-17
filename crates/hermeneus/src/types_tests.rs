use super::*;

#[test]
fn role_serde_roundtrip() {
    for role in [Role::System, Role::User, Role::Assistant] {
        let json = serde_json::to_string(&role).expect("Role should serialize to JSON");
        let back: Role = serde_json::from_str(&json).expect("Role should deserialize from JSON");
        assert_eq!(role, back, "Role should round-trip through JSON unchanged");
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
        let json = serde_json::to_string(&reason).expect("StopReason should serialize to JSON");
        let back: StopReason =
            serde_json::from_str(&json).expect("StopReason should deserialize from JSON");
        assert_eq!(
            reason, back,
            "StopReason should round-trip through JSON unchanged"
        );
    }
}

#[test]
fn content_text_extraction() {
    let text = Content::Text("hello world".to_owned());
    assert_eq!(
        text.text(),
        "hello world",
        "Content::Text should return its string via text()"
    );

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
    assert!(
        blocks.text().contains("let me think"),
        "text() should include thinking block content"
    );
    assert!(
        blocks.text().contains("the answer is 42"),
        "text() should include text block content"
    );
}

#[test]
fn tool_use_block_serde() {
    let block = ContentBlock::ToolUse {
        id: "tool_123".to_owned(),
        name: "exec".to_owned(),
        input: serde_json::json!({"command": "ls"}),
    };
    let json = serde_json::to_string(&block).expect("ToolUse block should serialize to JSON");
    assert!(
        json.contains("tool_use"),
        "serialized ToolUse should contain type tag 'tool_use'"
    );
    assert!(
        json.contains("exec"),
        "serialized ToolUse should contain tool name 'exec'"
    );

    let back: ContentBlock =
        serde_json::from_str(&json).expect("ToolUse block should deserialize from JSON");
    match back {
        ContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "tool_123", "ToolUse id should round-trip unchanged");
            assert_eq!(name, "exec", "ToolUse name should round-trip unchanged");
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
    let json = serde_json::to_string(&block).expect("ToolResult block should serialize to JSON");
    let back: ContentBlock =
        serde_json::from_str(&json).expect("ToolResult block should deserialize from JSON");
    match back {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(
                tool_use_id, "tool_123",
                "ToolResult tool_use_id should round-trip unchanged"
            );
            assert_eq!(
                content.text_summary(),
                "file.txt\ndir/",
                "ToolResult content text should round-trip unchanged"
            );
            assert_eq!(
                is_error,
                Some(false),
                "ToolResult is_error should round-trip unchanged"
            );
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
    let json = serde_json::to_string(&block).expect("ToolResult block should serialize to JSON");
    let v: serde_json::Value =
        serde_json::from_str(&json).expect("ToolResult JSON should parse as serde_json::Value");
    assert!(
        v["content"].is_string(),
        "Text should serialize as bare string"
    );
    assert_eq!(
        v["content"], "hello",
        "ToolResult text content should serialize to 'hello'"
    );
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
    let json = serde_json::to_string(&block).expect("ToolResult blocks should serialize to JSON");
    let v: serde_json::Value = serde_json::from_str(&json)
        .expect("ToolResult blocks JSON should parse as serde_json::Value");
    assert!(v["content"].is_array(), "Blocks should serialize as array");
    assert_eq!(
        v["content"]
            .as_array()
            .expect("content should be a JSON array")
            .len(),
        2,
        "content array should have exactly 2 elements"
    );
    assert_eq!(
        v["content"][0]["type"], "text",
        "first block type should be 'text'"
    );
    assert_eq!(
        v["content"][1]["type"], "image",
        "second block type should be 'image'"
    );
}

#[test]
fn tool_result_content_text_deserializes_from_string() {
    let json = r#"{"type":"tool_result","tool_use_id":"t1","content":"hello"}"#;
    let block: ContentBlock =
        serde_json::from_str(json).expect("ToolResult with string content should deserialize");
    match block {
        ContentBlock::ToolResult { content, .. } => {
            assert_eq!(
                content.text_summary(),
                "hello",
                "ToolResult text content should deserialize from string"
            );
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn tool_result_content_blocks_deserializes_from_array() {
    let json = r#"{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"hi"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"abc"}}]}"#;
    let block: ContentBlock =
        serde_json::from_str(json).expect("ToolResult with array content should deserialize");
    match block {
        ContentBlock::ToolResult { content, .. } => {
            assert_eq!(
                content.text_summary(),
                "hi\n[image]",
                "ToolResult blocks content should produce correct text summary"
            );
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
    let json = serde_json::to_string(&source).expect("ImageSource should serialize to JSON");
    let back: ImageSource =
        serde_json::from_str(&json).expect("ImageSource should deserialize from JSON");
    assert_eq!(
        back.source_type, "base64",
        "ImageSource source_type should round-trip unchanged"
    );
    assert_eq!(
        back.media_type, "image/png",
        "ImageSource media_type should round-trip unchanged"
    );
    assert_eq!(
        back.data, "iVBOR",
        "ImageSource data should round-trip unchanged"
    );
}

#[test]
fn document_source_serde_roundtrip() {
    let source = DocumentSource {
        source_type: "base64".to_owned(),
        media_type: "application/pdf".to_owned(),
        data: "JVBERi0".to_owned(),
    };
    let json = serde_json::to_string(&source).expect("DocumentSource should serialize to JSON");
    let back: DocumentSource =
        serde_json::from_str(&json).expect("DocumentSource should deserialize from JSON");
    assert_eq!(
        back.source_type, "base64",
        "DocumentSource source_type should round-trip unchanged"
    );
    assert_eq!(
        back.media_type, "application/pdf",
        "DocumentSource media_type should round-trip unchanged"
    );
    assert_eq!(
        back.data, "JVBERi0",
        "DocumentSource data should round-trip unchanged"
    );
}

#[test]
fn tool_result_content_from_string() {
    let content: ToolResultContent = "hello".to_owned().into();
    assert_eq!(
        content.text_summary(),
        "hello",
        "ToolResultContent created from String should report correct text summary"
    );
}

#[test]
fn usage_total() {
    let usage = Usage {
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 800,
        cache_write_tokens: 200,
    };
    assert_eq!(
        usage.total(),
        1500,
        "Usage.total() should equal input_tokens + output_tokens"
    );
}

#[test]
fn citation_char_location_serde() {
    let citation = Citation::CharLocation {
        document_index: 0,
        start_char_index: 10,
        end_char_index: 50,
        cited_text: "some text".to_owned(),
    };
    let json = serde_json::to_string(&citation).expect("Citation should serialize to JSON");
    let back: Citation =
        serde_json::from_str(&json).expect("Citation should deserialize from JSON");
    match back {
        Citation::CharLocation {
            document_index,
            start_char_index,
            ..
        } => {
            assert_eq!(
                document_index, 0,
                "Citation document_index should round-trip unchanged"
            );
            assert_eq!(
                start_char_index, 10,
                "Citation start_char_index should round-trip unchanged"
            );
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
    let json = serde_json::to_string(&block).expect("Thinking block should serialize to JSON");
    let back: ContentBlock =
        serde_json::from_str(&json).expect("Thinking block should deserialize from JSON");
    match back {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(
                thinking, "deep thoughts",
                "Thinking content should round-trip unchanged"
            );
            assert_eq!(
                signature.as_deref(),
                Some("sig_xyz"),
                "Thinking signature should round-trip unchanged"
            );
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
    let json = serde_json::to_string(&block)
        .expect("Thinking block without signature should serialize to JSON");
    let back: ContentBlock = serde_json::from_str(&json)
        .expect("Thinking block without signature should deserialize from JSON");
    match back {
        ContentBlock::Thinking { signature, .. } => {
            assert!(
                signature.is_none(),
                "Thinking block with no signature should deserialize with None signature"
            );
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
    let json = serde_json::to_string(&block).expect("ServerToolUse block should serialize to JSON");
    assert!(
        json.contains("server_tool_use"),
        "serialized ServerToolUse should contain type tag 'server_tool_use'"
    );
    let back: ContentBlock =
        serde_json::from_str(&json).expect("ServerToolUse block should deserialize from JSON");
    match back {
        ContentBlock::ServerToolUse { id, name, input } => {
            assert_eq!(
                id, "srvtoolu_123",
                "ServerToolUse id should round-trip unchanged"
            );
            assert_eq!(
                name, "web_search",
                "ServerToolUse name should round-trip unchanged"
            );
            assert_eq!(
                input["query"], "rust async",
                "ServerToolUse input query should round-trip unchanged"
            );
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
    let json =
        serde_json::to_string(&block).expect("WebSearchToolResult block should serialize to JSON");
    assert!(
        json.contains("web_search_tool_result"),
        "serialized WebSearchToolResult should contain type tag 'web_search_tool_result'"
    );
    let back: ContentBlock = serde_json::from_str(&json)
        .expect("WebSearchToolResult block should deserialize from JSON");
    match back {
        ContentBlock::WebSearchToolResult {
            tool_use_id,
            content,
        } => {
            assert_eq!(
                tool_use_id, "srvtoolu_123",
                "WebSearchToolResult tool_use_id should round-trip unchanged"
            );
            assert!(
                content.is_array(),
                "WebSearchToolResult content should deserialize as a JSON array"
            );
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
    let json = serde_json::to_string(&def).expect("ServerToolDefinition should serialize to JSON");
    assert!(
        json.contains("web_search_20250305"),
        "serialized ServerToolDefinition should contain tool_type 'web_search_20250305'"
    );
    let back: ServerToolDefinition =
        serde_json::from_str(&json).expect("ServerToolDefinition should deserialize from JSON");
    assert_eq!(
        back.tool_type, "web_search_20250305",
        "ServerToolDefinition tool_type should round-trip unchanged"
    );
    assert_eq!(
        back.max_uses,
        Some(5),
        "ServerToolDefinition max_uses should round-trip unchanged"
    );
}

#[test]
fn completion_request_default() {
    let req = CompletionRequest::default();
    assert!(
        req.model.is_empty(),
        "default CompletionRequest model should be empty"
    );
    assert!(
        req.system.is_none(),
        "default CompletionRequest system should be None"
    );
    assert!(
        req.messages.is_empty(),
        "default CompletionRequest messages should be empty"
    );
    assert_eq!(
        req.max_tokens, 4096,
        "default CompletionRequest max_tokens should be 4096"
    );
    assert!(
        req.server_tools.is_empty(),
        "default CompletionRequest server_tools should be empty"
    );
    assert!(
        !req.cache_system,
        "default CompletionRequest cache_system should be false"
    );
    assert!(
        !req.cache_tools,
        "default CompletionRequest cache_tools should be false"
    );
    assert!(
        req.tool_choice.is_none(),
        "default CompletionRequest tool_choice should be None"
    );
    assert!(
        req.metadata.is_none(),
        "default CompletionRequest metadata should be None"
    );
    assert!(
        req.citations.is_none(),
        "default CompletionRequest citations should be None"
    );
}

#[test]
fn tool_choice_serde() {
    let auto = ToolChoice::Auto;
    let json = serde_json::to_string(&auto).expect("ToolChoice::Auto should serialize to JSON");
    assert!(
        json.contains("\"type\":\"auto\""),
        "serialized ToolChoice::Auto should contain '\"type\":\"auto\"'"
    );

    let tool = ToolChoice::Tool {
        name: "exec".to_owned(),
    };
    let json = serde_json::to_string(&tool).expect("ToolChoice::Tool should serialize to JSON");
    assert!(
        json.contains("\"type\":\"tool\""),
        "serialized ToolChoice::Tool should contain '\"type\":\"tool\"'"
    );
    assert!(
        json.contains("\"name\":\"exec\""),
        "serialized ToolChoice::Tool should contain '\"name\":\"exec\"'"
    );
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
    let json =
        serde_json::to_string(&block).expect("Text block with citations should serialize to JSON");
    let back: ContentBlock = serde_json::from_str(&json)
        .expect("Text block with citations should deserialize from JSON");
    match back {
        ContentBlock::Text { citations, .. } => {
            assert_eq!(
                citations
                    .expect("citations should be present after round-trip")
                    .len(),
                1,
                "citations vec should contain exactly one entry"
            );
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
    let json =
        serde_json::to_string(&response).expect("CompletionResponse should serialize to JSON");
    let back: CompletionResponse =
        serde_json::from_str(&json).expect("CompletionResponse should deserialize from JSON");
    assert_eq!(
        back.id, "msg_123",
        "CompletionResponse id should round-trip unchanged"
    );
    assert_eq!(
        back.stop_reason,
        StopReason::EndTurn,
        "CompletionResponse stop_reason should round-trip unchanged"
    );
}

#[test]
fn cache_control_type_serde() {
    let cc = CacheControl::ephemeral();
    let json = serde_json::to_string(&cc).expect("CacheControl should serialize to JSON");
    assert!(
        json.contains("\"type\":\"ephemeral\""),
        "serialized CacheControl should contain '\"type\":\"ephemeral\"'"
    );
    let back: CacheControl =
        serde_json::from_str(&json).expect("CacheControl should deserialize from JSON");
    assert_eq!(
        back.kind,
        CacheControlType::Ephemeral,
        "CacheControl kind should round-trip as Ephemeral"
    );
}

#[test]
fn caching_config_defaults() {
    let config = CachingConfig::default();
    assert!(
        config.enabled,
        "default CachingConfig should have enabled=true"
    );
    assert_eq!(
        config.strategy,
        CachingStrategy::Auto,
        "default CachingConfig strategy should be Auto"
    );
}

#[test]
fn caching_strategy_serde_roundtrip() {
    for strategy in [CachingStrategy::Auto, CachingStrategy::Disabled] {
        let json =
            serde_json::to_string(&strategy).expect("CachingStrategy should serialize to JSON");
        let back: CachingStrategy =
            serde_json::from_str(&json).expect("CachingStrategy should deserialize from JSON");
        assert_eq!(
            strategy, back,
            "CachingStrategy should round-trip through JSON unchanged"
        );
    }
}

#[test]
fn completion_request_cache_defaults() {
    let req = CompletionRequest::default();
    assert!(
        !req.cache_system,
        "default CompletionRequest cache_system should be false"
    );
    assert!(
        !req.cache_tools,
        "default CompletionRequest cache_tools should be false"
    );
    assert!(
        !req.cache_turns,
        "default CompletionRequest cache_turns should be false"
    );
}

#[test]
fn code_execution_result_block_serde() {
    let block = ContentBlock::CodeExecutionResult {
        code: "print('hello')".to_owned(),
        stdout: "hello\n".to_owned(),
        stderr: String::new(),
        return_code: 0,
    };
    let json =
        serde_json::to_string(&block).expect("CodeExecutionResult block should serialize to JSON");
    assert!(
        json.contains("code_execution_result"),
        "serialized CodeExecutionResult should contain type tag 'code_execution_result'"
    );
    assert!(
        json.contains("print('hello')"),
        "serialized CodeExecutionResult should contain the code field"
    );
    let back: ContentBlock = serde_json::from_str(&json)
        .expect("CodeExecutionResult block should deserialize from JSON");
    match back {
        ContentBlock::CodeExecutionResult {
            code,
            stdout,
            stderr,
            return_code,
        } => {
            assert_eq!(
                code, "print('hello')",
                "CodeExecutionResult code should round-trip unchanged"
            );
            assert_eq!(
                stdout, "hello\n",
                "CodeExecutionResult stdout should round-trip unchanged"
            );
            assert!(
                stderr.is_empty(),
                "CodeExecutionResult stderr should round-trip as empty"
            );
            assert_eq!(
                return_code, 0,
                "CodeExecutionResult return_code should round-trip as 0"
            );
        }
        _ => panic!("expected CodeExecutionResult"),
    }
}

#[test]
fn code_execution_result_nonzero_return_code() {
    let json = r#"{"type":"code_execution_result","code":"exit(1)","stdout":"","stderr":"error","return_code":1}"#;
    let block: ContentBlock = serde_json::from_str(json)
        .expect("CodeExecutionResult with nonzero return_code should deserialize");
    match block {
        ContentBlock::CodeExecutionResult {
            return_code,
            stderr,
            ..
        } => {
            assert_eq!(
                return_code, 1,
                "CodeExecutionResult return_code should deserialize as 1"
            );
            assert_eq!(
                stderr, "error",
                "CodeExecutionResult stderr should deserialize as 'error'"
            );
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
    let json = serde_json::to_string(&def).expect("ToolDefinition should serialize to JSON");
    assert!(
        json.contains("\"disable_passthrough\":true"),
        "serialized ToolDefinition should contain '\"disable_passthrough\":true'"
    );
    let back: ToolDefinition =
        serde_json::from_str(&json).expect("ToolDefinition should deserialize from JSON");
    assert_eq!(
        back.disable_passthrough,
        Some(true),
        "ToolDefinition disable_passthrough should round-trip as Some(true)"
    );
}

#[test]
fn tool_definition_disable_passthrough_none_omitted() {
    let def = ToolDefinition {
        name: "exec".to_owned(),
        description: "Execute".to_owned(),
        input_schema: serde_json::json!({"type": "object"}),
        disable_passthrough: None,
    };
    let json = serde_json::to_string(&def).expect("ToolDefinition should serialize to JSON");
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
    let json = serde_json::to_string(&def).expect("ServerToolDefinition should serialize to JSON");
    assert!(
        json.contains("code_execution_20250522"),
        "serialized ServerToolDefinition should contain tool_type 'code_execution_20250522'"
    );
    let back: ServerToolDefinition =
        serde_json::from_str(&json).expect("ServerToolDefinition should deserialize from JSON");
    assert_eq!(
        back.tool_type, "code_execution_20250522",
        "ServerToolDefinition tool_type should round-trip unchanged"
    );
    assert_eq!(
        back.name, "code_execution",
        "ServerToolDefinition name should round-trip unchanged"
    );
}

//
// Per project standards (standards/TESTING.md): every type that implements
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
            let json = serde_json::to_string(&role).expect("Role should serialize to JSON");
            let back: Role = serde_json::from_str(&json).expect("Role JSON should deserialize back");
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
            let json = serde_json::to_string(&reason).expect("StopReason should serialize to JSON");
            let back: StopReason = serde_json::from_str(&json).expect("StopReason JSON should deserialize back");
            prop_assert_eq!(reason, back);
        }

        /// Content::Text round-trips through JSON without data loss.
        #[test]
        fn content_text_roundtrip(text in "\\PC{0,500}") {
            let content = Content::Text(text.clone());
            let json = serde_json::to_string(&content).expect("Content should serialize to JSON");
            let back: Content = serde_json::from_str(&json).expect("Content JSON should deserialize back");
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
            let json = serde_json::to_string(&msg).expect("Message should serialize to JSON");
            let back: Message = serde_json::from_str(&json).expect("Message JSON should deserialize back");
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
            let json = serde_json::to_string(&choice).expect("ToolChoice should serialize to JSON");
            let back: ToolChoice = serde_json::from_str(&json).expect("ToolChoice JSON should deserialize back");
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
            let json = serde_json::to_string(&choice).expect("ToolChoice::Tool should serialize to JSON");
            let back: ToolChoice = serde_json::from_str(&json).expect("ToolChoice::Tool JSON should deserialize back");
            match back {
                ToolChoice::Tool { name: n } => prop_assert_eq!(n, name),
                other => prop_assert!(false, "expected Tool variant, got {other:?}"),
            }
        }
    }
}
