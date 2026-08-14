//! Tests for distillation provenance: hashes/refs populated on `DistillResult`
//! and their reconstruction via `verify_provenance`.
#![expect(clippy::expect_used, reason = "test assertions")]

use super::{default_engine, sample_conversation, success_provider};
use crate::distill::verify_provenance;
use crate::provenance;

#[tokio::test]
async fn distill_populates_provenance_fields() {
    let engine = default_engine();
    let provider = success_provider(super::MOCK_SUMMARY);
    let messages = sample_conversation();

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .expect("distill must succeed");

    assert!(
        !result.source_message_ids.is_empty(),
        "source_message_ids must be populated"
    );
    assert_eq!(
        result.source_message_ids,
        provenance::message_refs(&messages),
        "source_message_ids must reference every distilled message, in order"
    );
    assert!(result.input_hash.starts_with("sha256:"));
    assert!(result.prompt_hash.starts_with("sha256:"));
    assert_ne!(
        result.input_hash, result.prompt_hash,
        "input_hash (conversation only) and prompt_hash (system + user content) \
         cover different text and must not collide"
    );
    assert!(!result.model.is_empty());
    assert!(result.config_snapshot_hash.starts_with("sha256:"));

    for item in result
        .memory_flush
        .decisions
        .iter()
        .chain(&result.memory_flush.corrections)
        .chain(&result.memory_flush.facts)
    {
        assert_eq!(
            item.source_message_ids, result.source_message_ids,
            "every flush item must carry the same source message pool this run used"
        );
    }
}

#[tokio::test]
async fn verify_provenance_succeeds_on_unmodified_result() {
    let engine = default_engine();
    let provider = success_provider(super::MOCK_SUMMARY);
    let messages = sample_conversation();
    let config = engine.config().clone();

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .expect("distill must succeed");

    verify_provenance(&result, &messages, &config, "test-nous")
        .expect("a genuine result must reconstruct cleanly from its own inputs");
}

#[tokio::test]
async fn verify_provenance_detects_a_tampered_input_hash() {
    let engine = default_engine();
    let provider = success_provider(super::MOCK_SUMMARY);
    let messages = sample_conversation();
    let config = engine.config().clone();

    let mut result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .expect("distill must succeed");
    result.input_hash = "sha256:tampered".to_owned();

    let err = verify_provenance(&result, &messages, &config, "test-nous")
        .expect_err("a tampered input_hash must fail reconstruction");
    assert!(
        err.contains("input_hash"),
        "error must name the mismatched field: {err}"
    );
}

#[tokio::test]
async fn verify_provenance_detects_a_different_message_set() {
    let engine = default_engine();
    let provider = success_provider(super::MOCK_SUMMARY);
    let messages = sample_conversation();
    let config = engine.config().clone();

    let result = engine
        .distill(&messages, "test-nous", &provider, 1)
        .await
        .expect("distill must succeed");

    let mut different_messages = messages.clone();
    different_messages.push(super::text_msg(
        hermeneus::types::Role::User,
        "an extra message that was never part of this run",
    ));

    let err = verify_provenance(&result, &different_messages, &config, "test-nous")
        .expect_err("reconstructing from the wrong message set must fail");
    assert!(
        err.contains("source_message_ids"),
        "error must name the mismatched field: {err}"
    );
}
