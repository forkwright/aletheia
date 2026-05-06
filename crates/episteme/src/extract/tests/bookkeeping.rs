//! Tests for the LLM-backed bookkeeping compatibility provider.

use eidos::bookkeeping::BookkeepingProvider;

use super::super::*;
use crate::bookkeeping::LlmBookkeepingProvider;

struct StaticProvider;

impl ExtractionProvider for StaticProvider {
    fn complete<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
    > {
        Box::pin(async {
            Ok(r#"{"entities":[{"name":"Alice","entity_type":"person","description":"operator"}],"relationships":[],"facts":[{"subject":"Alice","predicate":"uses","object":"Aletheia","confidence":0.9}]}"#.to_owned())
        })
    }
}

#[tokio::test]
async fn llm_bookkeeping_provider_extracts_facts_through_existing_path() {
    let config = ExtractionConfig {
        min_message_length: 1,
        ..ExtractionConfig::default()
    };
    let engine = ExtractionEngine::new(config);
    let provider = LlmBookkeepingProvider::new(&engine, &StaticProvider);

    let facts = match provider
        .extract_facts("Alice uses Aletheia.", &engine.config().schema())
        .await
    {
        Ok(facts) => facts,
        Err(err) => panic!("bookkeeping extraction should succeed: {err}"),
    };

    let [fact] = facts.as_slice() else {
        panic!("expected exactly one fact, got {}", facts.len());
    };
    assert_eq!(fact.subject, "Alice");
    assert_eq!(fact.predicate, "uses");
    assert_eq!(fact.object, "Aletheia");
}
