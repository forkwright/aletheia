//! Shared boundary plumbing for specialized (non-LLM) bookkeeping providers.
//!
//! `GLiNER` and `NuExtract` each own a distinct ONNX inference core, output
//! interpretation, and fallback policy — those stay in their own modules.
//! What they share is the plumbing around that core: turning a conversation
//! into model input text, loading a tokenizer, converting shapes/indices to
//! the integer types `ort` expects, and building a provider error with a
//! consistent shape. This module owns exactly that boundary vocabulary, not
//! a generalized "ML provider" abstraction.

use std::path::Path;

use eidos::bookkeeping::{
    BookkeepingError, BookkeepingResult, ConversationMessage, ProviderFailedSnafu,
};
use tokenizers::Tokenizer;

/// Flatten a conversation into a single text block for model input.
///
/// WHY: both `GLiNER` (span NER) and `NuExtract` (schema JSON) run text-only
/// ONNX graphs that take no role information, so roles are deliberately
/// discarded here and message contents are joined with `\n`. A provider that
/// needs role-preserving flattening should add its own projection rather than
/// changing this shared default.
pub(super) fn join_messages(messages: &[ConversationMessage]) -> String {
    messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Load a tokenizer from disk, mapping failure into a provider-labeled error.
pub(super) fn load_tokenizer(provider: &'static str, path: &Path) -> BookkeepingResult<Tokenizer> {
    Tokenizer::from_file(path).map_err(|err| provider_failed(provider, "load_tokenizer", err))
}

/// Convert a `usize` shape/index value into the `i64` `ort` tensors expect.
pub(super) fn usize_to_i64(provider: &'static str, value: usize) -> BookkeepingResult<i64> {
    i64::try_from(value).map_err(|err| provider_failed(provider, "integer_conversion", err))
}

/// Build a [`BookkeepingError`] with a consistent provider/operation/message shape.
pub(super) fn provider_failed(
    provider: &'static str,
    operation: &'static str,
    message: impl std::fmt::Display,
) -> BookkeepingError {
    ProviderFailedSnafu {
        provider,
        operation,
        message: message.to_string(),
    }
    .build()
}
