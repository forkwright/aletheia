//! Shared pipeline-message fixture for full- and micro-compaction tests.
use crate::pipeline::PipelineMessage;

pub(crate) fn make_text_msg(role: &str, content: &str, tokens: i64) -> PipelineMessage {
    PipelineMessage::text(role, content, tokens)
}
