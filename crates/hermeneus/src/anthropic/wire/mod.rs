//! Wire types matching the Anthropic Messages API format.

use crate::types::{Content, Role, StopReason};

mod request;
mod response;
mod stream;

pub(crate) use request::*;
pub(crate) use response::*;
pub(crate) use stream::*;

pub(super) const MAX_TURN_CACHE_BREAKPOINTS: usize = 2;

/// Determine which message indices should receive `cache_control` breakpoints.
///
/// Strategy: mark the last user message before the current (final) message,
/// plus one earlier user message if available. This creates up to
/// `MAX_TURN_CACHE_BREAKPOINTS` breakpoints so the API can cache the
/// conversation prefix on subsequent turns.
pub(super) fn compute_turn_cache_indices(messages: &[&crate::types::Message]) -> Vec<usize> {
    if messages.len() < 2 {
        return Vec::new();
    }

    let last_idx = messages.len() - 1;
    let mut breakpoints = Vec::new();

    for i in (0..last_idx).rev() {
        if messages[i].role == Role::User {
            breakpoints.push(i);
            if breakpoints.len() >= MAX_TURN_CACHE_BREAKPOINTS {
                break;
            }
        }
    }

    breakpoints
}

/// Transform content to include `cache_control: ephemeral` on the last block.
///
/// For `Content::Text`, wraps as a single-element block array.
/// For `Content::Blocks`, clones and injects `cache_control` on the final block.
pub(super) fn content_with_cache_control(content: &Content) -> serde_json::Value {
    let cc = serde_json::json!({"type": "ephemeral"});

    match content {
        Content::Text(text) => {
            serde_json::json!([{
                "type": "text",
                "text": text,
                "cache_control": cc
            }])
        }
        Content::Blocks(blocks) => {
            let mut arr: Vec<serde_json::Value> = blocks
                .iter()
                .map(|b| serde_json::to_value(b).unwrap_or_default())
                .collect();
            if let Some(last) = arr.last_mut()
                && let Some(obj) = last.as_object_mut()
            {
                obj.insert(String::from("cache_control"), cc);
            }
            serde_json::Value::Array(arr)
        }
    }
}

pub(super) fn parse_stop_reason(s: &str) -> Result<StopReason, String> {
    s.parse()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions use .expect() for descriptive panic messages")]
mod tests;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions use .expect() for descriptive panic messages")]
mod tests_extended;
