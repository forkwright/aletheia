//! Request, response, and server tool serde tests.
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting sufficient length"
)]
use super::*;

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
