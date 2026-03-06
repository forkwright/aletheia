//! Web research tools: web_fetch (HTTP GET to text).
//!
//! Web search is now handled by Anthropic's server-side `web_search` tool,
//! configured via `NousConfig.server_tools`. This module only provides
//! `web_fetch` for direct URL retrieval.

use std::future::Future;
use std::pin::Pin;

use aletheia_koina::id::ToolName;
use indexmap::IndexMap;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

use super::workspace::{extract_opt_u64, extract_str};

fn require_services(
    ctx: &ToolContext,
) -> std::result::Result<&crate::types::ToolServices, ToolResult> {
    ctx.services
        .as_deref()
        .ok_or_else(|| ToolResult::error("tool services not configured"))
}

// --- web_fetch ---

struct WebFetchExecutor;

impl ToolExecutor for WebFetchExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let services = match require_services(ctx) {
                Ok(s) => s,
                Err(r) => return Ok(r),
            };

            let url = extract_str(&input.arguments, "url", &input.name)?;
            let max_length = extract_opt_u64(&input.arguments, "maxLength").unwrap_or(50_000);

            // Basic URL validation
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Ok(ToolResult::error("URL must start with http:// or https://"));
            }

            let response = services
                .http_client
                .get(url)
                .header(
                    "User-Agent",
                    "Aletheia/1.0 (Research Agent; +https://github.com/CKickertz/aletheia)",
                )
                .send()
                .await;

            let response = match response {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::error(format!("fetch failed: {e}"))),
            };

            if !response.status().is_success() {
                return Ok(ToolResult::error(format!("HTTP {}", response.status())));
            }

            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();

            let body = match response.text().await {
                Ok(t) => t,
                Err(e) => return Ok(ToolResult::error(format!("failed to read body: {e}"))),
            };

            let text = if content_type.contains("text/html") {
                strip_html_tags(&body)
            } else {
                body
            };

            #[expect(clippy::cast_possible_truncation, reason = "max_length fits in usize")]
            let max_len = max_length as usize;
            let truncated = if text.len() > max_len {
                let mut end = max_len;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!(
                    "{}...\n\n[Truncated at {max_length} characters]",
                    &text[..end]
                )
            } else {
                text
            };

            Ok(ToolResult::text(truncated))
        })
    }
}

/// Strip HTML tags and collapse whitespace for readable text extraction.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut last_was_whitespace = false;

    let lower = html.to_lowercase();
    let bytes = html.as_bytes();
    let lower_bytes = lower.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if i + 7 < lower_bytes.len() && &lower_bytes[i..i + 7] == b"<script" {
                in_script = true;
            }
            if i + 9 < lower_bytes.len() && &lower_bytes[i..i + 9] == b"</script>" {
                in_script = false;
                in_tag = false;
                i += 9;
                continue;
            }
            if i + 6 < lower_bytes.len() && &lower_bytes[i..i + 6] == b"<style" {
                in_style = true;
            }
            if i + 8 < lower_bytes.len() && &lower_bytes[i..i + 8] == b"</style>" {
                in_style = false;
                in_tag = false;
                i += 8;
                continue;
            }
            in_tag = true;
            i += 1;
            continue;
        }

        if bytes[i] == b'>' {
            in_tag = false;
            if !last_was_whitespace && !result.is_empty() {
                result.push(' ');
                last_was_whitespace = true;
            }
            i += 1;
            continue;
        }

        if in_tag || in_script || in_style {
            i += 1;
            continue;
        }

        // Decode common HTML entities
        if bytes[i] == b'&' {
            if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"&lt;" {
                result.push('<');
                last_was_whitespace = false;
                i += 4;
                continue;
            }
            if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"&gt;" {
                result.push('>');
                last_was_whitespace = false;
                i += 4;
                continue;
            }
            if i + 5 <= bytes.len() && &bytes[i..i + 5] == b"&amp;" {
                result.push('&');
                last_was_whitespace = false;
                i += 5;
                continue;
            }
            if i + 6 <= bytes.len() && &bytes[i..i + 6] == b"&nbsp;" {
                result.push(' ');
                last_was_whitespace = true;
                i += 6;
                continue;
            }
            if i + 6 <= bytes.len() && &bytes[i..i + 6] == b"&quot;" {
                result.push('"');
                last_was_whitespace = false;
                i += 6;
                continue;
            }
        }

        let ch = bytes[i] as char;
        if ch.is_ascii_whitespace() {
            if !last_was_whitespace && !result.is_empty() {
                result.push(' ');
                last_was_whitespace = true;
            }
        } else {
            result.push(ch);
            last_was_whitespace = false;
        }
        i += 1;
    }

    result.trim().to_owned()
}

// --- Definition ---

fn web_fetch_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("web_fetch").expect("valid tool name"),
        description:
            "Fetch a URL and return its content as text. HTML pages are converted to readable text."
                .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "url".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "URL to fetch".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "maxLength".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum response length in characters (default: 50000)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(50000)),
                    },
                ),
            ]),
            required: vec!["url".to_owned()],
        },
        category: ToolCategory::Research,
        auto_activate: false,
    }
}

pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(web_fetch_def(), Box::new(WebFetchExecutor))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::types::{ToolContext, ToolInput, ToolServices};

    use super::*;

    fn test_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                http_client: reqwest::Client::new(),
                lazy_tool_catalog: vec![],
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    #[test]
    fn strip_html_basic() {
        let html = "<html><body><h1>Title</h1><p>Hello world</p></body></html>";
        let text = strip_html_tags(html);
        assert_eq!(text, "Title Hello world");
    }

    #[test]
    fn strip_html_script_and_style() {
        let html = "<p>Before</p><script>var x = 1;</script><style>.a{}</style><p>After</p>";
        let text = strip_html_tags(html);
        assert_eq!(text, "Before After");
    }

    #[test]
    fn strip_html_entities() {
        let html = "&amp; &lt;tag&gt; &quot;quoted&quot;";
        let text = strip_html_tags(html);
        assert_eq!(text, "& <tag> \"quoted\"");
    }

    #[test]
    fn strip_html_whitespace_collapse() {
        let html = "<p>  lots   of    spaces  </p>";
        let text = strip_html_tags(html);
        assert_eq!(text, "lots of spaces");
    }

    #[test]
    fn web_fetch_def_is_lazy() {
        let def = web_fetch_def();
        assert!(!def.auto_activate);
        assert_eq!(def.category, ToolCategory::Research);
    }

    #[tokio::test]
    async fn web_fetch_invalid_url() {
        let ctx = test_ctx();
        let executor = WebFetchExecutor;
        let input = ToolInput {
            name: ToolName::new("web_fetch").expect("valid"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({"url": "not-a-url"}),
        };

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("http"));
    }
}
