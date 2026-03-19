//! Batch API types for the Anthropic Messages Batches endpoint.
#![expect(
    dead_code,
    reason = "batch API types reserved for future Anthropic batch support"
)]

use serde::{Deserialize, Serialize};

/// A batch request: multiple messages requests submitted together.
#[derive(Debug, Clone, Serialize)]
pub struct BatchRequest {
    pub requests: Vec<BatchItem>,
}

/// A single item in a batch request.
#[derive(Debug, Clone, Serialize)]
pub struct BatchItem {
    pub custom_id: String,
    /// Pre-serialized request body (`WireRequest` borrows and can't be stored).
    pub params: serde_json::Value,
}

/// Response from batch creation.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResponse {
    pub id: String,
    pub processing_status: String,
    pub request_counts: BatchRequestCounts,
    pub results_url: Option<String>,
}

/// Counts of requests in various states.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchRequestCounts {
    pub processing: u32,
    pub succeeded: u32,
    pub errored: u32,
    pub canceled: u32,
    pub expired: u32,
}

/// A single result from a completed batch.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResult {
    pub custom_id: String,
    pub result: BatchResultType,
}

/// Result type for a batch item.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum BatchResultType {
    #[serde(rename = "succeeded")]
    Succeeded { message: serde_json::Value },
    #[serde(rename = "errored")]
    Errored { error: BatchError },
}

/// Error detail for a failed batch item.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: serde_json indexing is safe (returns Null on missing key)"
)]
mod tests {
    use super::*;

    #[test]
    fn batch_request_serializes() {
        let req = BatchRequest {
            requests: vec![BatchItem {
                custom_id: "req-1".to_owned(),
                params: serde_json::json!({"model": "claude-opus-4-20250514", "max_tokens": 100}),
            }],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["requests"][0]["custom_id"], "req-1");
    }

    #[test]
    fn batch_response_deserializes() {
        let json = r#"{
            "id": "batch_123",
            "processing_status": "in_progress",
            "request_counts": {
                "processing": 5,
                "succeeded": 3,
                "errored": 1,
                "canceled": 0,
                "expired": 0
            },
            "results_url": null
        }"#;
        let resp: BatchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "batch_123");
        assert_eq!(resp.request_counts.succeeded, 3);
    }

    #[test]
    fn batch_result_succeeded_deserializes() {
        let json = r#"{
            "custom_id": "req-1",
            "result": {
                "type": "succeeded",
                "message": {"id": "msg_1", "content": []}
            }
        }"#;
        let result: BatchResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.custom_id, "req-1");
        assert!(matches!(result.result, BatchResultType::Succeeded { .. }));
    }

    #[test]
    fn batch_result_errored_deserializes() {
        let json = r#"{
            "custom_id": "req-2",
            "result": {
                "type": "errored",
                "error": {"type": "invalid_request_error", "message": "bad input"}
            }
        }"#;
        let result: BatchResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.custom_id, "req-2");
        match result.result {
            BatchResultType::Errored { error } => {
                assert_eq!(error.message, "bad input");
            }
            BatchResultType::Succeeded { .. } => panic!("expected Errored"),
        }
    }
}
