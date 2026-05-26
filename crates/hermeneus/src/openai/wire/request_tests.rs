use super::*;
use crate::types::{
    CompletionRequest, Content, ContentBlock, Message, OutputFormat, Role, ThinkingConfig,
    ToolDefinition, ToolResultContent,
};

#[test]
fn system_prompt_becomes_first_system_message() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        system: Some("You are a helpful assistant.".to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    assert_eq!(wire.messages.len(), 2);
    assert_eq!(wire.messages[0].role, "system");
    assert_eq!(
        wire.messages[0].content.as_deref(),
        Some("You are a helpful assistant.")
    );
    assert_eq!(wire.messages[1].role, "user");
}

#[test]
fn tool_definitions_map_to_function_tools() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("use a tool".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        tools: vec![ToolDefinition {
            name: "get_weather".to_owned(),
            description: "Fetch weather for a city".to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "city": { "type": "string" } }
            }),
            disable_passthrough: None,
        }],
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    assert_eq!(wire.tools.len(), 1);
    let tool = &wire.tools[0];
    assert_eq!(tool.tool_type, "function");
    assert_eq!(tool.function.name, "get_weather");
}

#[test]
fn responses_tool_definitions_preserve_non_strict_schema_behavior() {
    let req = CompletionRequest {
        model: "gpt-5".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("use a tool".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        tools: vec![ToolDefinition {
            name: "get_weather".to_owned(),
            description: "Fetch weather for a city".to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "city": { "type": "string" } }
            }),
            disable_passthrough: None,
        }],
        ..Default::default()
    };

    let wire = ResponsesRequest::from_request(&req, None).unwrap();
    let json = serde_json::to_value(&wire).unwrap();

    assert_eq!(json["tools"][0]["type"], "function");
    assert_eq!(json["tools"][0]["name"], "get_weather");
    assert_eq!(json["tools"][0]["strict"], false);
}

#[test]
fn assistant_tool_use_block_becomes_tool_calls() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Text("call get_weather".to_owned()),
                cache_breakpoint: false,
            },
            Message {
                role: Role::Assistant,
                content: Content::Blocks(vec![
                    ContentBlock::Text {
                        text: "Sure".to_owned(),
                        citations: None,
                    },
                    ContentBlock::ToolUse {
                        id: "call_1".to_owned(),
                        name: "get_weather".to_owned(),
                        input: serde_json::json!({ "city": "Paris" }),
                    },
                ]),
                cache_breakpoint: false,
            },
        ],
        max_tokens: 128,
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    let assistant = wire
        .messages
        .iter()
        .find(|m| m.role == "assistant")
        .unwrap();
    assert_eq!(assistant.tool_calls.len(), 1);
    assert_eq!(assistant.tool_calls[0].id, "call_1");
    assert_eq!(assistant.tool_calls[0].function.name, "get_weather");
    assert!(assistant.tool_calls[0].function.arguments.contains("Paris"));
}

#[test]
fn user_tool_result_block_becomes_role_tool_message() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_owned(),
                content: ToolResultContent::Text("sunny".to_owned()),
                is_error: None,
            }]),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    let tool_msg = wire.messages.iter().find(|m| m.role == "tool").unwrap();
    assert_eq!(tool_msg.tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(tool_msg.content.as_deref(), Some("sunny"));
}

#[test]
fn thinking_block_is_dropped_and_warned() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        thinking: Some(ThinkingConfig {
            enabled: true,
            budget_tokens: 1024,
        }),
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    let json = serde_json::to_string(&wire).unwrap();
    assert!(!json.contains("thinking"));
    assert!(!json.contains("budget_tokens"));
}

#[test]
fn server_tools_rejected_with_clear_error() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        server_tools: vec![crate::types::ServerToolDefinition {
            tool_type: "web_search_20250305".to_owned(),
            name: "web_search".to_owned(),
            max_uses: Some(3),
            allowed_domains: None,
            blocked_domains: None,
            user_location: None,
        }],
        ..Default::default()
    };
    let err = ChatCompletionRequest::from_request(&req, None).unwrap_err();
    assert!(err.to_string().contains("server-side tools"));
}

#[test]
fn cache_flags_dropped_without_error() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        system: Some("sys".to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        cache_system: true,
        cache_tools: true,
        cache_turns: true,
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    let json = serde_json::to_string(&wire).unwrap();
    assert!(!json.contains("cache_control"));
}

#[test]
fn tool_choice_any_maps_to_required() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        tool_choice: Some(ToolChoice::Any),
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    assert_eq!(
        wire.tool_choice,
        Some(serde_json::Value::String("required".to_owned()))
    );
}

#[test]
fn tool_arguments_serialized_as_json_string() {
    let req = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![Message {
            role: Role::Assistant,
            content: Content::Blocks(vec![ContentBlock::ToolUse {
                id: "c1".to_owned(),
                name: "f".to_owned(),
                input: serde_json::json!({ "x": 1 }),
            }]),
            cache_breakpoint: false,
        }],
        max_tokens: 64,
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    let tc = &wire.messages[0].tool_calls[0];
    let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(parsed["x"], 1);
}

#[test]
fn output_format_json_schema_maps_to_response_format() {
    let req = CompletionRequest {
        model: "gpt-4o".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("Give me JSON".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        output_format: Some(OutputFormat::JsonSchema {
            name: "answer".to_owned(),
            schema: serde_json::json!({
                "type": "object",
                "properties": { "answer": { "type": "string" } }
            }),
            strict: Some(true),
        }),
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    assert!(
        wire.output_format.is_some(),
        "output_format should be translated"
    );
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["response_format"]["type"], "json_schema");
    assert_eq!(json["response_format"]["json_schema"]["name"], "answer");
    assert_eq!(json["response_format"]["json_schema"]["strict"], true);
    assert_eq!(
        json["response_format"]["json_schema"]["schema"]["type"],
        "object"
    );
}

#[test]
fn output_format_none_omits_response_format() {
    let req = CompletionRequest {
        model: "gpt-4o".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        output_format: None,
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    assert!(wire.output_format.is_none());
    let json = serde_json::to_string(&wire).unwrap();
    assert!(
        !json.contains("response_format"),
        "serialized request must not contain response_format when output_format is None"
    );
}

#[test]
fn output_format_text_maps_to_response_format_text() {
    let req = CompletionRequest {
        model: "gpt-4o".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        output_format: Some(OutputFormat::Text),
        ..Default::default()
    };
    let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["response_format"]["type"], "text");
}
