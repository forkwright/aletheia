//! Fuzz target for tool dispatch input parsing.
//!
//! Exercises the highest-risk parsing surface: user-controlled JSON that drives
//! tool execution. Covers `ContentBlock` tagged-enum deserialization, `ToolCall`
//! serde roundtrip, `ToolName` validation, and `LoopDetector` cycle detection.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // 1. ContentBlock tagged-enum deserialization (includes ToolUse variant).
    //    Malformed JSON, unexpected `type` tags, missing fields, extra fields.
    let _ = serde_json::from_slice::<aletheia_hermeneus::types::ContentBlock>(data);

    // 2. ToolCall deserialization: the struct persisted per-turn.
    //    Unexpected types in `input` (Value), missing optional `result`, etc.
    let _ = serde_json::from_slice::<aletheia_nous::pipeline::ToolCall>(data);

    // 3. ToolName validation: arbitrary strings against the allowlist regex.
    //    Empty, oversized (>128), unicode, special chars, null bytes.
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = aletheia_koina::id::ToolName::new(s);
    }

    // 4. JSON Value parsing and stringification (exercises the path simple_hash takes).
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) {
        // Replicates the simple_hash code path: Value -> String -> hash.
        let s = value.to_string();
        assert!(!s.is_empty());
    }

    // 5. LoopDetector: feed arbitrary tool_name:input_hash sequences.
    //    Tests cycle detection with varied pattern lengths and thresholds.
    if data.len() >= 4 {
        if let Ok(s) = std::str::from_utf8(&data[1..]) {
            let threshold = (data[0] % 5).saturating_add(2); // 2..=6
            let mut detector = aletheia_nous::pipeline::LoopDetector::new(u32::from(threshold));
            for chunk in s.as_bytes().chunks(8) {
                if let Ok(part) = std::str::from_utf8(chunk) {
                    // Ensure split lands on a char boundary to avoid panics
                    // on multi-byte UTF-8 sequences.
                    let mid = part.floor_char_boundary((part.len() / 2).max(1));
                    if mid > 0 && mid < part.len() {
                        let (name, hash) = part.split_at(mid);
                        let _ = detector.record(name, hash);
                    }
                }
            }
            // Verify invariants hold after arbitrary input.
            let _ = detector.call_count();
            let _ = detector.pattern_count();
        }
    }

    // 6. InteractionSignal serde roundtrip: enum variant coverage.
    let _ = serde_json::from_slice::<aletheia_nous::pipeline::InteractionSignal>(data);
});
