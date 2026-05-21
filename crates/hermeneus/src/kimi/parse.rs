//! Parse Kimi CLI `--print` output.
//!
//! Kimi emits Python-repr-like records on stdout, including:
//! - `TextPart(type='text', text='...')`
//! - `StatusUpdate(... token_usage=TokenUsage(...), message_id='...')`
//!
//! The text parts carry response text. Status updates provide token usage and
//! a provider message ID when present.

use crate::error::{self, Result};
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

/// Usage stats parsed from Kimi `StatusUpdate` output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct KimiUsage {
    pub(crate) input_other: u64,
    pub(crate) output: u64,
    pub(crate) input_cache_read: u64,
    pub(crate) input_cache_creation: u64,
}

impl KimiUsage {
    pub(crate) fn to_usage(self) -> Usage {
        Usage {
            input_tokens: self.input_other,
            output_tokens: self.output,
            cache_read_tokens: self.input_cache_read,
            cache_write_tokens: self.input_cache_creation,
        }
    }
}

/// Parse a `TextPart(...)` line and return its `text` field.
pub(crate) fn parse_text_part_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with("TextPart(") {
        return None;
    }
    parse_quoted_assignment(trimmed, "text")
}

/// Parse a standalone `text='...'` assignment from a multi-line `TextPart`.
pub(crate) fn parse_text_assignment_line(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_end_matches(',');
    if !trimmed.starts_with("text=") {
        return None;
    }
    parse_quoted_assignment(trimmed, "text")
}

/// Parse a `message_id='...'` assignment line.
pub(crate) fn parse_message_id_line(line: &str) -> Option<String> {
    parse_quoted_assignment(line.trim(), "message_id")
}

/// Parse one numeric `TokenUsage(...)` assignment line into `usage`.
pub(crate) fn parse_usage_assignment(line: &str, usage: &mut KimiUsage) {
    let trimmed = line.trim().trim_end_matches(',');
    if let Some(value) = parse_u64_assignment(trimmed, "input_other") {
        usage.input_other = value;
    } else if let Some(value) = parse_u64_assignment(trimmed, "output") {
        usage.output = value;
    } else if let Some(value) = parse_u64_assignment(trimmed, "input_cache_read") {
        usage.input_cache_read = value;
    } else if let Some(value) = parse_u64_assignment(trimmed, "input_cache_creation") {
        usage.input_cache_creation = value;
    }
}

fn parse_u64_assignment(line: &str, key: &str) -> Option<u64> {
    let rest = line.strip_prefix(key)?.strip_prefix('=')?;
    rest.trim_end_matches(',').parse::<u64>().ok()
}

fn parse_quoted_assignment(line: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=");
    let start = line.find(&marker)? + marker.len();
    let rest = line.get(start..)?;
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }

    let mut escaped = false;
    let mut raw = String::new();
    let mut chars = rest.chars();
    let first = chars.next()?;
    debug_assert_eq!(first, quote);
    for ch in chars {
        if escaped {
            raw.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return Some(raw);
        } else {
            raw.push(ch);
        }
    }

    None
}

/// Convert Kimi subprocess output into a `CompletionResponse`.
pub(crate) fn result_to_response(
    result_text: &str,
    usage: Option<KimiUsage>,
    model: &str,
    message_id: Option<&str>,
) -> Result<CompletionResponse> {
    if result_text.is_empty() {
        return Err(error::ApiRequestSnafu {
            message: "Kimi subprocess produced an empty response".to_owned(),
        }
        .build());
    }

    let content = vec![ContentBlock::Text {
        text: result_text.to_owned(),
        citations: None,
    }];

    let usage = usage.map_or_else(Usage::default, KimiUsage::to_usage);
    let id = match message_id {
        Some(id) => id.to_owned(),
        None => format!("kimi_{}", koina::uuid::uuid_v4()),
    };

    Ok(CompletionResponse {
        id,
        model: model.to_owned(),
        stop_reason: StopReason::EndTurn,
        content,
        usage,
        cost_usd: None,
        duration_ms: None,
    })
}
