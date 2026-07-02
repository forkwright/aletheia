#![expect(clippy::expect_used, reason = "test assertions")]

use tokio_util::sync::CancellationToken;

use super::*;

#[tokio::test]
async fn prosoche_returns_items_for_default() {
    let check = ProsocheCheck::new("test-nous");
    let result = check.run(&CancellationToken::new())
        .await
        .expect("should succeed");
    assert!(!result.checked_at.is_empty());
}

#[test]
fn prosoche_check_new() {
    let check = ProsocheCheck::new("alice-nous");
    let debug = format!("{check:?}");
    assert!(
        debug.contains("alice-nous"),
        "ProsocheCheck should store the nous_id"
    );
}

#[test]
fn prosoche_honors_daemon_behavior_sample_size() {
    let behavior = taxis::config::DaemonBehaviorConfig {
        prosoche_anomaly_sample_size: 27,
        ..taxis::config::DaemonBehaviorConfig::default()
    };
    let check = ProsocheCheck::new("test-nous").with_daemon_behavior(&behavior);

    assert_eq!(check.anomaly_sample_size, 27);
}

#[test]
fn attention_item_category_label_calendar() {
    let item = AttentionItem {
        category: AttentionCategory::Calendar,
        summary: "meeting".to_owned(),
        urgency: Urgency::Medium,
    };
    assert_eq!(item.category_label(), "calendar");
}

#[test]
fn attention_item_category_label_task() {
    let item = AttentionItem {
        category: AttentionCategory::Task,
        summary: "review PR".to_owned(),
        urgency: Urgency::Low,
    };
    assert_eq!(item.category_label(), "task");
}

#[test]
fn attention_item_category_label_health() {
    let item = AttentionItem {
        category: AttentionCategory::SystemHealth,
        summary: "disk full".to_owned(),
        urgency: Urgency::Critical,
    };
    assert_eq!(item.category_label(), "health");
}

#[test]
fn attention_item_category_label_custom() {
    let item = AttentionItem {
        category: AttentionCategory::Custom("foo".to_owned()),
        summary: "custom item".to_owned(),
        urgency: Urgency::Low,
    };
    assert_eq!(item.category_label(), "foo");
}

#[test]
fn urgency_ordering() {
    assert!(Urgency::Low < Urgency::Medium);
    assert!(Urgency::Medium < Urgency::High);
    assert!(Urgency::High < Urgency::Critical);
}

#[test]
fn prosoche_result_serialization() {
    let result = ProsocheResult {
        items: vec![AttentionItem {
            category: AttentionCategory::Task,
            summary: "test".to_owned(),
            urgency: Urgency::High,
        }],
        checked_at: "2026-01-01T00:00:00Z".to_owned(),
    };
    let json = serde_json::to_string(&result).expect("serialize");
    let back: ProsocheResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.items.len(), 1);
    assert_eq!(back.checked_at, "2026-01-01T00:00:00Z");
}

#[test]
fn attention_item_serialization() {
    let item = AttentionItem {
        category: AttentionCategory::Calendar,
        summary: "standup".to_owned(),
        urgency: Urgency::Medium,
    };
    let json = serde_json::to_string(&item).expect("serialize");
    assert!(json.contains("Calendar"));
    assert!(json.contains("standup"));
    assert!(json.contains("Medium"));
}

#[test]
fn parse_vmrss_extracts_value() {
    let content = "\
Name:   aletheia
VmPeak:   500000 kB
VmRSS:   123456 kB
Threads:  8
";
    let rss = parse_vmrss(content).expect("should parse");
    assert_eq!(rss, 123_456);
}

#[test]
fn parse_vmrss_missing_returns_error() {
    let content = "Name: aletheia\nThreads: 8\n";
    assert!(parse_vmrss(content).is_err());
}

#[test]
fn check_memory_runs_without_panic() {
    let items = check_memory();
    assert!(
        items.len() <= 1,
        "test process should not exceed memory thresholds"
    );
}

#[test]
fn check_db_sizes_empty_paths() {
    let items = check_db_sizes(&[]);
    assert!(items.is_empty());
}

#[test]
fn check_db_sizes_nonexistent_file() {
    let items = check_db_sizes(&[PathBuf::from("/tmp/nonexistent-db-file-for-test.db")]);
    assert!(
        items.is_empty(),
        "nonexistent file should not produce items"
    );
}

#[test]
fn check_db_sizes_small_file() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let db_path = dir.path().join("test.db");
    #[expect(
        clippy::disallowed_methods,
        reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
    )]
    std::fs::write(&db_path, b"small content").expect("write test file");

    let items = check_db_sizes(&[db_path]);
    assert!(items.is_empty(), "small file should not trigger warning");
}

#[tokio::test]
async fn prosoche_with_data_dir_runs_disk_check() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let check = ProsocheCheck::new("test-nous").with_data_dir(dir.path());
    let result = check.run(&CancellationToken::new())
        .await
        .expect("should succeed");
    assert!(!result.checked_at.is_empty());
}

#[tokio::test]
async fn prosoche_with_db_paths_runs_size_check() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let db_path = dir.path().join("test.db");
    #[expect(
        clippy::disallowed_methods,
        reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
    )]
    std::fs::write(&db_path, b"data").expect("write");
    let check = ProsocheCheck::new("test-nous").with_db_paths(vec![db_path]);
    let result = check.run(&CancellationToken::new())
        .await
        .expect("should succeed");
    assert!(!result.checked_at.is_empty());
}

#[test]
fn parse_df_percent_valid() {
    let output = "Use%\n 42%\n";
    let percent = parse_df_percent(output).expect("should parse");
    assert!((percent - 42.0).abs() < f64::EPSILON);
}

#[test]
fn parse_df_percent_no_data() {
    let output = "Use%\n";
    assert!(parse_df_percent(output).is_err());
}

// WHY: a hung `df` on a stale NFS/automount mount must not block the prosoche
// heartbeat slot. The production timeout is 10 s; here we advance Tokio's
// paused clock past that bound so the test verifies the guard without waiting
// in real time.
#[cfg(unix)]
#[tokio::test]
async fn disk_usage_percent_times_out_on_hanging_df() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let dir = tempfile::tempdir().expect("create tempdir");
    let fake_df = dir.path().join("df");
    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture writes a temporary executable script"
    )]
    std::fs::write(&fake_df, "#!/bin/sh\nexec sleep 120\n").expect("write fake df");
    let mut perms = std::fs::metadata(&fake_df).expect("metadata").permissions();
    perms.set_mode(0o755);
    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture sets executable permissions"
    )]
    std::fs::set_permissions(&fake_df, perms).expect("set permissions");

    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let mut new_path = OsString::from(dir.path().as_os_str());
    new_path.push(":");
    new_path.push(&original_path);
    // SAFETY: `set_var` is unsafe in Rust 2024 because concurrent mutation of
    // the process environment is technically a data race. This test runs in its
    // own nextest process and restores the original PATH before returning, so
    // no other test in the same process observes the temporary value.
    unsafe {
        std::env::set_var("PATH", new_path);
    }

    tokio::time::pause();

    let path = dir.path().to_path_buf();
    let cancel = CancellationToken::new();
    let start = tokio::time::Instant::now();
    let handle = tokio::spawn(async move { disk_usage_percent(&path, &cancel).await });

    // WHY: advance past the 10 s df timeout so the test does not wait in real time.
    tokio::time::advance(Duration::from_secs(11)).await;
    let result = handle.await.expect("join task");
    let elapsed = start.elapsed();

    assert!(result.is_err(), "hanging df should time out: {result:?}");
    assert!(
        elapsed < Duration::from_secs(1),
        "timeout should fire quickly under paused time, took {elapsed:?}"
    );

    // SAFETY: paired with the earlier `set_var`; restores the original PATH.
    unsafe {
        std::env::set_var("PATH", original_path);
    }
}

#[test]
fn attention_item_category_label_memory_anomaly() {
    let item = AttentionItem {
        category: AttentionCategory::MemoryAnomaly,
        summary: "orphaned fact".to_owned(),
        urgency: Urgency::Low,
    };
    assert_eq!(item.category_label(), "memory-anomaly");
}

#[test]
fn memory_anomaly_serialization_roundtrip() {
    let item = AttentionItem {
        category: AttentionCategory::MemoryAnomaly,
        summary: "dangling reference".to_owned(),
        urgency: Urgency::Medium,
    };
    let json = serde_json::to_string(&item).expect("serialize");
    assert!(json.contains("MemoryAnomaly"));
    let back: AttentionItem = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.category_label(), "memory-anomaly");
}

// --- Knowledge-store-gated tests ---

#[cfg(feature = "knowledge-store")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length vectors"
)]
mod knowledge_store_tests {
    use super::*;

    /// Helper: create a test fact with the given id string.
    fn make_fact(id: &str, content: &str) -> episteme::knowledge::Fact {
        episteme::knowledge::Fact {
            id: episteme::id::FactId::new(id).expect("valid id"),
            nous_id: "test-nous".to_owned(),
            fact_type: "observation".to_owned(),
            content: content.to_owned(),
            scope: None,
            project_id: None,
            temporal: episteme::knowledge::FactTemporal {
                valid_from: jiff::Timestamp::now(),
                valid_to: jiff::Timestamp::from_second(253_402_207_200).expect("far future"),
                recorded_at: jiff::Timestamp::now(),
            },
            provenance: episteme::knowledge::FactProvenance {
                confidence: 0.9,
                tier: episteme::knowledge::EpistemicTier::Verified,
                source_session_id: None,
                stability_hours: 720.0,
            },
            lifecycle: episteme::knowledge::FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: episteme::knowledge::FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
            sensitivity: episteme::knowledge::FactSensitivity::Public,
            visibility: episteme::knowledge::Visibility::Private,
        }
    }

    /// Helper: insert a `fact_entities` row via raw Datalog.
    fn insert_fact_entity_raw(
        store: &episteme::knowledge_store::KnowledgeStore,
        fact_id: &str,
        entity_id: &str,
    ) {
        use std::collections::BTreeMap;

        use episteme::engine::DataValue;

        let now = episteme::knowledge::format_timestamp(&jiff::Timestamp::now());
        let mut params = BTreeMap::new();
        params.insert("fact_id".to_owned(), DataValue::Str(fact_id.into()));
        params.insert("entity_id".to_owned(), DataValue::Str(entity_id.into()));
        params.insert("created_at".to_owned(), DataValue::Str(now.into()));
        store
            .run_mut_query(
                r"?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]
                  :put fact_entities { fact_id, entity_id => created_at }",
                params,
            )
            .expect("insert fact_entity");
    }

    #[test]
    fn truncate_content_short() {
        assert_eq!(truncate_content("hello", 10), "hello");
    }

    #[test]
    fn truncate_content_exact_boundary() {
        assert_eq!(truncate_content("hello", 5), "hello");
    }

    #[test]
    fn truncate_content_long() {
        let long = "a".repeat(100);
        let result = truncate_content(&long, 10);
        assert_eq!(result.len(), 13); // 10 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_content_multibyte() {
        // 3-byte UTF-8 chars: should not split in the middle.
        let s = "\u{2603}\u{2603}\u{2603}\u{2603}"; // 4 snowmen, 12 bytes
        let result = truncate_content(s, 7);
        assert!(result.ends_with("..."));
        // Should include at most 2 snowmen (6 bytes < 7) + "..."
        assert!(result.starts_with("\u{2603}\u{2603}"));
    }

    #[test]
    fn consistency_check_empty_store() {
        let store = episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");
        let checker = MultiPathConsistencyCheck::new(15);
        let items = checker.check(&store).expect("check");
        assert!(items.is_empty(), "empty store should produce no anomalies");
    }

    #[test]
    fn consistency_check_orphaned_facts() {
        // Insert facts without entity links — they should be flagged as orphaned.
        let store = episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");

        let fact = make_fact("fact-orphan-001", "Rust is a systems programming language");
        store.insert_fact(&fact).expect("insert fact");

        let checker = MultiPathConsistencyCheck::new(15);
        let items = checker.check(&store).expect("check");

        assert_eq!(items.len(), 1, "should detect one orphaned fact");
        assert!(
            matches!(items[0].category, AttentionCategory::MemoryAnomaly),
            "category should be MemoryAnomaly"
        );
        assert!(
            items[0].summary.contains("Orphaned fact"),
            "summary should mention orphaned: {}",
            items[0].summary
        );
        assert_eq!(items[0].urgency, Urgency::Low);
    }

    #[test]
    fn consistency_check_linked_facts_no_anomaly() {
        // Insert a fact WITH entity links — no anomaly should be flagged.
        let store = episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");

        let fact = make_fact("fact-linked-001", "Cody prefers Rust over Go");
        store.insert_fact(&fact).expect("insert fact");

        let entity = episteme::knowledge::Entity {
            id: episteme::id::EntityId::new("entity-cody-001").expect("valid"),
            name: "Cody".to_owned(),
            entity_type: "person".to_owned(),
            aliases: Vec::new(),
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");
        insert_fact_entity_raw(&store, fact.id.as_str(), entity.id.as_str());

        let checker = MultiPathConsistencyCheck::new(15);
        let items = checker.check(&store).expect("check");

        assert!(
            items.is_empty(),
            "linked facts should produce no anomalies, got: {items:?}"
        );
    }

    #[test]
    fn consistency_check_dangling_reference() {
        // Insert a fact_entities entry pointing to a non-existent fact.
        let store = episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");

        let entity = episteme::knowledge::Entity {
            id: episteme::id::EntityId::new("entity-ghost-001").expect("valid"),
            name: "Ghost".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        // Insert a fact_entities row with a fact_id that doesn't exist in facts.
        insert_fact_entity_raw(&store, "phantom-fact-001", entity.id.as_str());

        let checker = MultiPathConsistencyCheck::new(15);
        let items = checker.check(&store).expect("check");

        assert_eq!(items.len(), 1, "should detect one dangling reference");
        assert!(
            matches!(items[0].category, AttentionCategory::MemoryAnomaly),
            "category should be MemoryAnomaly"
        );
        assert!(
            items[0].summary.contains("Dangling"),
            "summary should mention dangling: {}",
            items[0].summary
        );
        assert_eq!(items[0].urgency, Urgency::Medium);
    }

    #[tokio::test]
    async fn prosoche_with_knowledge_store_runs_consistency_check() {
        let store = episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");

        // Insert an orphaned fact (no entity link).
        let fact = make_fact("fact-prosoche-001", "test content");
        store.insert_fact(&fact).expect("insert fact");

        let check = ProsocheCheck::new("test-nous").with_knowledge_store(store);
        let result = check.run(&CancellationToken::new())
            .await
            .expect("should succeed");

        let anomalies: Vec<_> = result
            .items
            .iter()
            .filter(|i| matches!(i.category, AttentionCategory::MemoryAnomaly))
            .collect();
        assert_eq!(anomalies.len(), 1, "should detect the orphaned fact");
    }
}
