//! Advanced serde tests: caching, code execution, tool definitions.
use super::*;

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
