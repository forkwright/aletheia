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

/// Current schema version for [`TrainingRecord`].
///
/// Bump this constant whenever fields are added, removed, or change
/// semantics so that records from different epochs can be distinguished
/// at read time.
pub const TRAINING_RECORD_SCHEMA_VERSION: u32 = 1;

/// A single training record representing one conversation turn.
///
/// Serialized as one JSON line in the output JSONL file. Fields match
/// the kanon training corpus schema for downstream compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRecord {
    /// Schema version that produced this record.
    ///
    /// Defaults to `0` when deserializing records written before the
    /// field existed, distinguishing them from version-1+ records.
    #[serde(default)]
    pub schema_version: u32,
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
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
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
        assert_eq!(back.schema_version, TRAINING_RECORD_SCHEMA_VERSION);
        assert_eq!(back.session_id, record.session_id);
        assert_eq!(back.tokens, record.tokens);
    }

    #[test]
    fn training_record_deserialize_missing_schema_version() {
        // Records written before schema_version existed should deserialize
        // with schema_version defaulting to 0.
        let json = r#"{"session_id":"ses-old","nous_id":"syn","user_message":"hi","assistant_response":"hello","model":"test","tokens":10,"timestamp":"1970-01-01T00:00:00Z"}"#;
        let record: TrainingRecord = serde_json::from_str(json).expect("deserialize legacy");
        assert_eq!(record.schema_version, 0);
        assert_eq!(record.session_id, "ses-old");
    }
}
