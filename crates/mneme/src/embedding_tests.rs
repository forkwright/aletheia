#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;

#[test]
fn mock_provider_produces_correct_dimension() {
    let provider = MockEmbeddingProvider::new(384);
    let vec = provider
        .embed("hello world")
        .expect("mock embed should not fail");
    assert_eq!(vec.len(), 384);
}

#[test]
fn mock_provider_is_deterministic() {
    let provider = MockEmbeddingProvider::new(64);
    let v1 = provider
        .embed("test input")
        .expect("mock embed deterministic v1");
    let v2 = provider
        .embed("test input")
        .expect("mock embed deterministic v2");
    assert_eq!(v1, v2);
}

#[test]
fn mock_provider_different_texts_differ() {
    let provider = MockEmbeddingProvider::new(64);
    let v1 = provider.embed("hello").expect("mock embed for 'hello'");
    let v2 = provider.embed("world").expect("mock embed for 'world'");
    assert_ne!(v1, v2);
}

#[test]
fn mock_provider_is_normalized() {
    let provider = MockEmbeddingProvider::new(128);
    let vec = provider
        .embed("normalize me")
        .expect("mock embed for normalization check");
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 0.01, "expected unit norm, got {norm}");
}

#[test]
fn batch_embed_matches_individual() {
    let provider = MockEmbeddingProvider::new(64);
    let texts = ["hello", "world", "test"];
    let batch = provider
        .embed_batch(&texts)
        .expect("batch embed should not fail");
    for (i, text) in texts.iter().enumerate() {
        let individual = provider
            .embed(text)
            .expect("individual embed should not fail");
        assert_eq!(batch[i], individual);
    }
}

#[test]
fn create_mock_provider() {
    let config = EmbeddingConfig::default();
    let provider = create_provider(&config).expect("create mock provider from default config");
    assert_eq!(provider.dimension(), 384);
    assert_eq!(provider.model_name(), "mock-embedding");
}

#[test]
fn create_unknown_provider_fails() {
    let config = EmbeddingConfig {
        provider: "nonexistent".to_owned(),
        ..EmbeddingConfig::default()
    };
    assert!(create_provider(&config).is_err());
}

#[test]
fn mock_provider_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockEmbeddingProvider>();
}

#[test]
fn embedding_empty_input() {
    let provider = MockEmbeddingProvider::new(64);
    let result = provider.embed("");
    assert!(
        result.is_ok(),
        "empty string should produce a valid embedding"
    );
    let vec = result.expect("embedding empty string must succeed");
    assert_eq!(vec.len(), 64);
}

#[test]
fn embedding_long_input() {
    let provider = MockEmbeddingProvider::new(128);
    let long_text = "word ".repeat(10_000);
    let result = provider.embed(&long_text);
    assert!(result.is_ok(), "long input should succeed");
    let vec = result.expect("embedding long input must succeed");
    assert_eq!(vec.len(), 128);
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "long input embedding should be normalized, got {norm}"
    );
}

#[test]
fn embedding_provider_switching() {
    let small = create_provider(&EmbeddingConfig {
        provider: "mock".to_owned(),
        dimension: Some(64),
        ..EmbeddingConfig::default()
    })
    .expect("create small mock provider");

    let large = create_provider(&EmbeddingConfig {
        provider: "mock".to_owned(),
        dimension: Some(256),
        ..EmbeddingConfig::default()
    })
    .expect("create large mock provider");

    assert_eq!(small.dimension(), 64);
    assert_eq!(large.dimension(), 256);

    let v_small = small.embed("test").expect("small provider embed");
    let v_large = large.embed("test").expect("large provider embed");
    assert_eq!(v_small.len(), 64);
    assert_eq!(v_large.len(), 256);
    assert_ne!(v_small.len(), v_large.len());
}

#[test]
fn create_provider_custom_dimension() {
    let config = EmbeddingConfig {
        provider: "mock".to_owned(),
        dimension: Some(512),
        ..EmbeddingConfig::default()
    };
    let provider = create_provider(&config).expect("create custom dimension mock provider");
    assert_eq!(provider.dimension(), 512);

    let vec = provider
        .embed("custom dim")
        .expect("embed with custom dimension provider");
    assert_eq!(vec.len(), 512);
}

#[test]
fn embedding_batch_empty_list() {
    let provider = MockEmbeddingProvider::new(64);
    let result = provider.embed_batch(&[]);
    assert!(result.is_ok());
    assert!(
        result
            .expect("batch embed of empty slice should not fail")
            .is_empty()
    );
}

#[test]
fn mock_provider_consistent_dimension() {
    let provider = MockEmbeddingProvider::new(256);
    assert_eq!(provider.dimension(), 256);
    let vec = provider
        .embed("consistency check")
        .expect("mock embed for consistency check");
    assert_eq!(
        vec.len(),
        provider.dimension(),
        "dimension() must match actual vector length"
    );
}

#[test]
fn mock_provider_batch_empty() {
    let provider = MockEmbeddingProvider::new(128);
    let result = provider
        .embed_batch(&[])
        .expect("batch embed of empty should succeed");
    assert!(result.is_empty(), "batch of empty slice returns empty vec");
}

#[test]
fn mock_provider_different_texts_same_dim() {
    let provider = MockEmbeddingProvider::new(96);
    let inputs = ["alpha", "beta", "gamma", "delta", ""];
    for input in &inputs {
        let vec = provider
            .embed(input)
            .expect("mock embed for dimension check");
        assert_eq!(
            vec.len(),
            96,
            "all inputs must produce vectors of configured dimension"
        );
    }
}

#[test]
fn create_provider_mock_config() {
    let config = EmbeddingConfig {
        provider: "mock".to_owned(),
        model: Some("custom-model".to_owned()),
        dimension: Some(768),
        api_key: None,
    };
    let provider = create_provider(&config).expect("create mock provider with full config");
    assert_eq!(provider.dimension(), 768);
    assert_eq!(provider.model_name(), "mock-embedding");
    let vec = provider
        .embed("test")
        .expect("embed with full config mock provider");
    assert_eq!(vec.len(), 768);
}

#[test]
fn embed_empty_string() {
    let provider = MockEmbeddingProvider::new(64);
    let result = provider.embed("");
    assert!(result.is_ok(), "embedding empty string must not panic");
    let vec = result.expect("embedding empty string must succeed");
    assert_eq!(vec.len(), 64);
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        norm < 1.1,
        "empty string embedding should be normalized or zero"
    );
}

#[test]
fn embed_batch_single_item() {
    let provider = MockEmbeddingProvider::new(64);
    let single = provider.embed("solo").expect("single embed for 'solo'");
    let batch = provider
        .embed_batch(&["solo"])
        .expect("batch of single item for 'solo'");
    assert_eq!(batch.len(), 1);
    assert_eq!(
        batch[0], single,
        "batch of one must match single embed result"
    );
}

#[test]
fn mock_embed_normalized() {
    let provider = MockEmbeddingProvider::new(256);
    let inputs = ["alpha", "bravo", "charlie delta echo"];
    for input in &inputs {
        let vec = provider
            .embed(input)
            .expect("mock embed for normalization magnitude check");
        let magnitude: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (magnitude - 1.0).abs() < 0.001,
            "vector for {input:?} should be L2-normalized, got magnitude {magnitude}"
        );
    }
}

#[test]
fn mock_embed_batch_matches_single() {
    let provider = MockEmbeddingProvider::new(128);
    let texts = ["foo bar", "baz qux", "hello world", "rust lang", ""];
    let batch = provider
        .embed_batch(&texts)
        .expect("batch embed must succeed");
    assert_eq!(batch.len(), texts.len());
    for (i, text) in texts.iter().enumerate() {
        let single = provider
            .embed(text)
            .expect("single embed in batch comparison must succeed");
        assert_eq!(
            batch[i], single,
            "batch[{i}] must equal single embed for {text:?}"
        );
    }
}

#[test]
fn mock_model_name() {
    let provider = MockEmbeddingProvider::new(64);
    assert_eq!(provider.model_name(), "mock-embedding");
}

#[tokio::test]
async fn concurrent_embed_no_deadlock() {
    use std::sync::Arc;
    let provider = Arc::new(MockEmbeddingProvider::new(128));
    let mut set = tokio::task::JoinSet::new();
    for i in 0..4u32 {
        let p = Arc::clone(&provider);
        set.spawn(async move {
            let text = format!("concurrent text {i}");
            let vec = p.embed(&text).expect("concurrent embed must succeed");
            assert_eq!(
                vec.len(),
                128,
                "concurrent embed must produce correct dimension"
            );
            vec
        });
    }
    let mut results = Vec::new();
    while let Some(result) = set.join_next().await {
        results.push(result.expect("spawned task must not panic"));
    }
    assert_eq!(results.len(), 4, "all 4 concurrent tasks must complete");
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn embedding_dimensions_constant(input in "[a-zA-Z ]{1,100}") {
            let provider = MockEmbeddingProvider::new(384);
            let vec = provider.embed(&input).expect("mock embed in proptest must succeed");
            prop_assert_eq!(vec.len(), 384);
            let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            prop_assert!((norm - 1.0).abs() < 0.01, "norm was {}", norm);
        }
    }
}

#[cfg(not(feature = "embed-candle"))]
#[test]
fn candle_not_enabled_returns_error() {
    let config = EmbeddingConfig {
        provider: "candle".to_owned(),
        ..EmbeddingConfig::default()
    };
    let Err(err) = create_provider(&config) else {
        panic!("expected error for disabled embed-candle feature");
    };
    let msg = err.to_string();
    assert!(
        msg.contains("not enabled"),
        "expected 'not enabled' in error, got: {msg}"
    );
}

#[test]
fn lock_poisoned_error_returns_err_not_panic() {
    use std::sync::RwLock;

    // Poison an RwLock by panicking inside a thread while holding a write lock.
    let m: RwLock<u32> = RwLock::new(0);
    let _ = std::panic::catch_unwind(|| {
        let _guard = m.write().expect("pre-poison write lock must succeed");
        panic!("intentional poison");
    });
    assert!(
        m.is_poisoned(),
        "RwLock must be poisoned after thread panic"
    );

    // Simulate what embed() does: map_err to LockPoisoned on read().
    let result: EmbeddingResult<()> = m
        .read()
        .map_err(|_poison| LockPoisonedSnafu.build())
        .map(|_| ());
    assert!(
        matches!(result, Err(EmbeddingError::LockPoisoned { .. })),
        "poisoned RwLock read must produce EmbeddingError::LockPoisoned"
    );
}

#[test]
fn lock_poisoned_error_formats() {
    let err = LockPoisonedSnafu.build();
    assert_eq!(
        err.to_string(),
        "embedding model lock poisoned",
        "LockPoisoned display must match spec"
    );
}

#[cfg(feature = "embed-candle")]
mod candle_tests {
    use super::*;
    use std::sync::LazyLock;

    static PROVIDER: LazyLock<CandelProvider> =
        LazyLock::new(|| CandelProvider::new(None).expect("candle provider init"));

    #[test]
    fn candle_provider_initializes() {
        assert_eq!(PROVIDER.dimension(), 384);
    }

    #[test]
    fn candle_embed_produces_correct_dimension() {
        let vec = PROVIDER
            .embed("hello world")
            .expect("candle embed for dimension check");
        assert_eq!(vec.len(), 384);
    }

    #[test]
    fn candle_embed_is_normalized() {
        let vec = PROVIDER
            .embed("normalize me")
            .expect("candle embed for normalization check");
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "expected unit norm, got {norm}");
    }

    #[test]
    fn candle_embed_deterministic() {
        let v1 = PROVIDER
            .embed("test input")
            .expect("candle embed deterministic v1");
        let v2 = PROVIDER
            .embed("test input")
            .expect("candle embed deterministic v2");
        assert_eq!(v1, v2);
    }

    #[test]
    fn candle_different_texts_differ() {
        let v1 = PROVIDER.embed("hello").expect("candle embed for 'hello'");
        let v2 = PROVIDER.embed("world").expect("candle embed for 'world'");
        assert_ne!(v1, v2);
    }

    #[test]
    fn candle_batch_matches_individual() {
        let texts = ["hello", "world", "test"];
        let batch = PROVIDER.embed_batch(&texts).expect("candle batch embed");
        for (i, text) in texts.iter().enumerate() {
            let individual = PROVIDER
                .embed(text)
                .expect("candle individual embed in batch comparison");
            assert_eq!(batch[i], individual);
        }
    }

    #[test]
    fn candle_provider_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CandelProvider>();
    }

    /// Spawn 4 tasks that embed concurrently via the shared candle provider.
    /// Verifies no deadlock under concurrent read locks.
    #[tokio::test]
    async fn candle_concurrent_embed_no_deadlock() {
        use std::sync::Arc;
        let provider: Arc<dyn EmbeddingProvider> =
            Arc::new(CandelProvider::new(None).expect("candle provider init for concurrent test"));
        let mut set = tokio::task::JoinSet::new();
        for i in 0..4u32 {
            let p = Arc::clone(&provider);
            set.spawn(async move {
                let text = format!("concurrent candle text {i}");
                let vec = p
                    .embed(&text)
                    .expect("concurrent candle embed must succeed");
                assert_eq!(
                    vec.len(),
                    384,
                    "concurrent candle embed must produce correct dimension"
                );
                vec
            });
        }
        let mut results = Vec::new();
        while let Some(result) = set.join_next().await {
            results.push(result.expect("spawned candle task must not panic"));
        }
        assert_eq!(
            results.len(),
            4,
            "all 4 concurrent candle tasks must complete"
        );
    }
}
