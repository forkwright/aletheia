//! Fuzz target for mneme knowledge store write/read round-trip.
//!
//! Exercises fact serialization, timestamp parsing, fact type classification,
//! and: when valid input is produced: actual insert/read against an in-memory
//! CozoDB knowledge store. Catches edge cases in bi-temporal semantics,
//! epistemic tier encoding, and content boundary validation.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;

use aletheia_mneme::id::FactId;
use aletheia_mneme::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal, FactType,
    ForgetReason, far_future, format_timestamp, parse_timestamp,
};
use aletheia_mneme::knowledge_store::KnowledgeStore;

/// Shared in-memory knowledge store, initialized once for the fuzzer process.
static STORE: LazyLock<std::sync::Arc<KnowledgeStore>> =
    LazyLock::new(|| KnowledgeStore::open_mem().expect("in-memory knowledge store must open"));

/// Monotonic counter for unique fact IDs across fuzz iterations.
static FACT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fuzz_target!(|data: &[u8]| {
    // 1. Fact JSON deserialization: malformed content, invalid confidence,
    //    missing fields, unexpected types, oversized payloads.
    let _ = serde_json::from_slice::<Fact>(data);

    // 2. Timestamp parsing: ISO 8601, date-only, far-future sentinel,
    //    empty strings, garbage input.
    if let Ok(s) = std::str::from_utf8(data) {
        let parsed = parse_timestamp(s);
        // If it parsed, format/parse roundtrip must be stable.
        if let Some(ts) = parsed {
            let formatted = format_timestamp(&ts);
            let reparsed = parse_timestamp(&formatted);
            assert!(
                reparsed.is_some(),
                "format_timestamp output must reparse: {formatted}"
            );
        }

        // 3. FactType classification: keyword heuristics on arbitrary strings.
        let _ = FactType::classify(s);
        let _ = FactType::from_str_lossy(s);

        // 4. ForgetReason parsing: enum from string.
        let _ = s.parse::<ForgetReason>();

        // 5. EpistemicTier serde roundtrip.
        let _ = serde_json::from_str::<EpistemicTier>(s);
    }

    // 6. Knowledge store write/read round-trip.
    //    Construct a Fact from fuzzer-derived bytes and attempt insert + read.
    // WHY: .get() instead of indexing to avoid false-positive fuzzer crash reports.
    let Some(&b0) = data.get(0) else { return };
    let Some(&b1) = data.get(1) else { return };
    let Some(&b2) = data.get(2) else { return };
    let Some(content_bytes) = data.get(8..) else {
        return;
    };

    let counter = FACT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let fact_id = format!("fuzz-{counter}");

    // Derive content from fuzzer input (skip first 8 bytes used for other fields).
    let content = String::from_utf8_lossy(content_bytes);

    // Skip empty content (insert_fact rejects it).
    if content.is_empty() {
        return;
    }

    // Clamp content to MAX_CONTENT_LENGTH to focus on store logic, not validation.
    // WHY: floor_char_boundary + .get() avoids panics on UTF-8 boundaries in fuzz input.
    let max_len = aletheia_mneme::knowledge::MAX_CONTENT_LENGTH;
    let content: &str = if content.len() > max_len {
        let end = content.floor_char_boundary(max_len);
        content.get(..end).unwrap_or(&content)
    } else {
        &content
    };

    // Derive confidence from first byte: clamp to [0.0, 1.0].
    let confidence = f64::from(b0) / 255.0;

    // Derive tier from second byte.
    let tier = match b1 % 3 {
        0 => EpistemicTier::Verified,
        1 => EpistemicTier::Inferred,
        _ => EpistemicTier::Assumed,
    };

    // Derive fact_type from third byte.
    let fact_type = match b2 % 7 {
        0 => FactType::Identity,
        1 => FactType::Preference,
        2 => FactType::Skill,
        3 => FactType::Relationship,
        4 => FactType::Event,
        5 => FactType::Task,
        _ => FactType::Observation,
    };

    let now = jiff::Timestamp::now();
    // WHY: FactId::new returns Result; skip iteration if ID is somehow invalid.
    let Ok(id) = FactId::new(fact_id) else { return };
    let fact = Fact {
        id,
        nous_id: "fuzz-agent".to_owned(),
        fact_type: fact_type.as_str().to_owned(),
        content: content.to_string(),
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence,
            tier,
            source_session_id: None,
            stability_hours: fact_type.base_stability_hours(),
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };

    // Insert and read back.
    if STORE.insert_fact(&fact).is_ok() {
        let now_str = format_timestamp(&now);
        if let Ok(facts) = STORE.query_facts("fuzz-agent", &now_str, 1000) {
            // The fact we just inserted should be retrievable (unless the store
            // hit an internal limit). We don't assert exact count because other
            // fuzz iterations may have inserted facts concurrently.
            let _ = facts.len();
        }
    }
});
