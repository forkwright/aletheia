//! Training data capture types.
//!
//! Configuration and record types for training data capture.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// Configuration for training data capture.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrainingConfig {
    /// Whether training data capture is enabled.
    pub enabled: bool,
    /// Directory path for training data output, relative to the instance root.
    ///
    /// The JSONL file `conversations.jsonl` is written inside this directory.
    pub path: String,
}

impl Default for TrainingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "data/training".to_owned(),
        }
    }
}

/// A single training record representing one conversation turn.
///
/// Serialized as one JSON line in the output JSONL file. Fields match
/// the kanon training corpus schema for downstream compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRecord {
    /// Session identifier (groups turns within a conversation).
    pub session_id: String,
    /// Nous agent identifier that handled the turn.
    pub nous_id: String,
    /// The user's input message.
    pub user_message: String,
    /// The assistant's response content.
    pub assistant_response: String,
    /// LLM model used for generation.
    pub model: String,
    /// Total tokens consumed (input + output).
    pub tokens: u64,
    /// When the turn was captured.
    pub timestamp: Timestamp,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn training_config_defaults() {
        let config = TrainingConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.path, "data/training");
    }

    #[test]
    fn training_record_serde_roundtrip() {
        let record = TrainingRecord {
            session_id: "ses-1".to_owned(),
            nous_id: "syn".to_owned(),
            user_message: "test input".to_owned(),
            assistant_response: "test output".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            tokens: 200,
            timestamp: Timestamp::UNIX_EPOCH,
        };

        let json = serde_json::to_string(&record).expect("serialize");
        let back: TrainingRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.session_id, record.session_id);
        assert_eq!(back.tokens, record.tokens);
    }
}
