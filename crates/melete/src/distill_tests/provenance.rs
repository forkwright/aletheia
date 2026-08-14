//! Tests for distillation provenance: hashes/refs populated on `DistillResult`
//! and their reconstruction via `verify_provenance`.
#![expect(clippy::expect_used, reason = "test assertions")]

use hermeneus::types::Message;

use super::{default_engine, sample_conversation, success_provider};
use crate::distill::{DistillConfig, verify_provenance};
use crate::provenance;

/// Reconstruct the pruned, post-similarity-filtering message set
/// [`crate::distill::DistillResult::source_message_ids`] names, by
/// replaying the same tail-split + prune steps `DistillEngine::distill`
/// runs before sending anything to the LLM (see `distill.rs`'s
/// `source_message_ids` field doc and its population-site WHY comment).
/// This is the set [`verify_provenance`] must be called with -- not the
/// original conversation, which similarity pruning is not guaranteed to
/// reduce identically twice (see `provenance.rs`'s module doc).
///
/// WARNING: re-deriving the set here is only sound because
/// `sample_conversation`'s messages are textually distinct enough that
/// pruning is a no-op on them, so the run is stable despite the bucket
/// iteration order that makes pruning non-deterministic in general. That is
/// a property of the fixture, not of this helper. Adding two similar
/// messages to `sample_conversation` could make these tests flake
/// intermittently rather than fail outright -- the worst failure shape to
/// diagnose. If that fixture gains near-duplicate content, capture the
/// pruned set from the distill run instead of recomputing it.
#[expect(
    clippy::indexing_slicing,
    reason = "split_at = messages.len() - tail where tail <= messages.len(), mirrors distill.rs"
)]
fn pruned_for_summarization(messages: &[Message], config: &DistillConfig) -> Vec<Message> {
    let tail = config.verbatim_tail.min(messages.len());
    let split_at = messages.len() - tail;
    let to_summarize = &messages[..split_at];
    let (pruned, _stats) = crate::similarity::prune_similar_messages(
        to_summarize,
        config.similarity_threshold,
        crate::similarity::DEFAULT_MAX_SIMILARITY_MESSAGES,
    );
    pruned
}

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
    let expected = pruned_for_summarization(&messages, engine.config());
    assert_eq!(
        result.source_message_ids,
        provenance::message_refs(&expected),
        "source_message_ids must reference the pruned, post-similarity-filtering set \
         actually sent to the LLM, in order -- not the verbatim tail, and not any \
         near-duplicates similarity pruning removed"
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

    let pruned = pruned_for_summarization(&messages, &config);
    verify_provenance(&result, &pruned, &config, "test-nous")
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

    let pruned = pruned_for_summarization(&messages, &config);
    let err = verify_provenance(&result, &pruned, &config, "test-nous")
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
