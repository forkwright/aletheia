// WHY: `session_line` and `write_jsonl` were reimplemented independently in
// the affinity, empirical, and persona router test modules — the same
// after-action JSONL record schema three times over. Shared here so the
// schema has one home; `aletheia-routing::store` carries its own copy
// deliberately, since that lives in a separate crate this one depends on.
#![cfg(test)]

use std::io::Write as _;
use std::path::Path;

/// A single after-action JSONL record for `model`/`status`/`category`, with
/// otherwise-fixed dispatch metadata (timestamps, cost, turns, QA verdict).
pub(crate) fn session_line(model: &str, status: &str, category: &str) -> serde_json::Value {
    serde_json::json!({
        "dispatch_id": "test",
        "ts_start": "2026-04-17T00:00:00Z",
        "ts_end": "2026-04-17T00:01:00Z",
        "duration_ms": 60000,
        "session_outcomes": [{"model": model, "status": status, "category": category}],
        "cost_total_cents": 5,
        "turns_total": 10,
        "stage_latencies_ms": {},
        "qa_verdict": "pass",
        "prompt_hash": "sha256:abc"
    })
}

/// Write `lines` as newline-delimited JSON to `dir/filename`.
pub(crate) fn write_jsonl(dir: &Path, filename: &str, lines: &[serde_json::Value]) {
    let path = dir.join(filename);
    let mut file = std::fs::File::create(path).unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
}
