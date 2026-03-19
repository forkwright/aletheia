#![expect(
    clippy::expect_used,
    reason = "test assertions use .expect() for descriptive panic messages"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting sufficient length"
)]
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

mod advanced;
mod request_response;
