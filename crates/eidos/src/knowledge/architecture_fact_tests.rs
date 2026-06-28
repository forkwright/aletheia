#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

fn minimal_fact(id: &str, scope: FactScope, claim: &str) -> ArchitectureFact {
    ArchitectureFact {
        id: id.to_owned(),
        scope,
        claim: claim.to_owned(),
        evidence: vec![],
        mneme_session: None,
        updated_at: "2026-04-22T00:00:00Z".to_owned(),
        updated_by: "PR-1".to_owned(),
    }
}

#[test]
fn serde_round_trip() {
    let fact = ArchitectureFact {
        id: "test.fact.one".to_owned(),
        scope: FactScope::Concept,
        claim: "Agents spawn in-process.".to_owned(),
        evidence: vec!["crates/nous/src/spawn_svc.rs:56".to_owned()],
        mneme_session: Some("session_abc".to_owned()),
        updated_at: "2026-04-22T12:00:00Z".to_owned(),
        updated_by: "PR-9999".to_owned(),
    };
    let json = serde_json::to_string(&fact).expect("serialise");
    let back: ArchitectureFact = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back.id, fact.id);
    assert_eq!(back.scope, fact.scope);
    assert_eq!(back.claim, fact.claim);
    assert_eq!(back.evidence, fact.evidence);
    assert_eq!(back.mneme_session, fact.mneme_session);
    assert_eq!(back.updated_by, fact.updated_by);
}

#[test]
fn serde_optional_mneme_session_omitted_when_none() {
    let fact = ArchitectureFact {
        id: "test.fact.two".to_owned(),
        scope: FactScope::Crate,
        claim: "eidos has no internal deps.".to_owned(),
        evidence: vec![],
        mneme_session: None,
        updated_at: "2026-04-22T00:00:00Z".to_owned(),
        updated_by: "PR-3789".to_owned(),
    };
    let json = serde_json::to_string(&fact).expect("serialise");
    assert!(
        !json.contains("mneme_session"),
        "mneme_session should be omitted when None"
    );
}

#[test]
fn new_sets_jiff_parseable_rfc3339_timestamp() {
    let fact = ArchitectureFact::new(
        "test.fact.timestamp",
        FactScope::Concept,
        "ArchitectureFact::new timestamps are jiff parseable.",
        Vec::new(),
        "test",
    );

    let parsed = fact
        .updated_at
        .parse::<Timestamp>()
        .expect("updated_at parses as jiff timestamp");
    assert_eq!(
        parsed.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        fact.updated_at,
        "timestamp should round-trip through jiff with second precision"
    );
}

#[test]
fn fact_scope_serde_all_variants() {
    for (scope, expected) in [
        (FactScope::Crate, "\"crate\""),
        (FactScope::Module, "\"module\""),
        (FactScope::Concept, "\"concept\""),
        (FactScope::Boundary, "\"boundary\""),
    ] {
        let json = serde_json::to_string(&scope).expect("serialise scope");
        assert_eq!(json, expected, "scope {scope:?} serialise mismatch");
        let back: FactScope = serde_json::from_str(&json).expect("deserialise scope");
        assert_eq!(back, scope, "scope {scope:?} round-trip failed");
    }
}

#[test]
fn fact_scope_display() {
    assert_eq!(FactScope::Crate.to_string(), "crate");
    assert_eq!(FactScope::Module.to_string(), "module");
    assert_eq!(FactScope::Concept.to_string(), "concept");
    assert_eq!(FactScope::Boundary.to_string(), "boundary");
}

#[tokio::test]
async fn put_then_get_returns_fact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    let fact = ArchitectureFact {
        id: "test.put.get".to_owned(),
        scope: FactScope::Boundary,
        claim: "Test claim.".to_owned(),
        evidence: vec!["src/lib.rs:1".to_owned()],
        mneme_session: None,
        updated_at: "2026-04-22T00:00:00Z".to_owned(),
        updated_by: "PR-1".to_owned(),
    };
    store.put(fact.clone()).await.expect("put");
    let got = store.get("test.put.get").await.expect("get");
    let got = got.expect("fact should exist");
    assert_eq!(got.id, "test.put.get");
    assert_eq!(got.claim, "Test claim.");
}

#[tokio::test]
async fn get_missing_returns_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    let result = store.get("nonexistent.fact").await.expect("get");
    assert!(result.is_none(), "missing fact should return None");
}

#[tokio::test]
async fn list_all_facts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    for i in 0..3u32 {
        let fact = ArchitectureFact {
            id: format!("test.list.{i}"),
            scope: if i == 0 {
                FactScope::Crate
            } else {
                FactScope::Concept
            },
            claim: format!("Claim {i}."),
            evidence: vec![],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-1".to_owned(),
        };
        store.put(fact).await.expect("put");
    }
    let all = store.list(None).await.expect("list all");
    assert_eq!(all.len(), 3, "expected 3 facts, got {}", all.len());
}

#[tokio::test]
async fn list_filtered_by_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    store
        .put(ArchitectureFact {
            id: "test.scope.crate".to_owned(),
            scope: FactScope::Crate,
            claim: "Crate fact.".to_owned(),
            evidence: vec![],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-1".to_owned(),
        })
        .await
        .expect("put");
    store
        .put(ArchitectureFact {
            id: "test.scope.concept".to_owned(),
            scope: FactScope::Concept,
            claim: "Concept fact.".to_owned(),
            evidence: vec![],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-1".to_owned(),
        })
        .await
        .expect("put");
    let crates = store
        .list(Some(FactScope::Crate))
        .await
        .expect("list crate");
    assert_eq!(crates.len(), 1, "expected 1 crate fact");
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    let crate_scope = crates[0].scope;
    assert_eq!(crate_scope, FactScope::Crate);
}

#[tokio::test]
async fn search_by_claim_substring() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    store
        .put(ArchitectureFact {
            id: "test.search.one".to_owned(),
            scope: FactScope::Concept,
            claim: "Agents spawn as Tokio tasks.".to_owned(),
            evidence: vec![],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-1".to_owned(),
        })
        .await
        .expect("put");
    store
        .put(ArchitectureFact {
            id: "test.search.two".to_owned(),
            scope: FactScope::Crate,
            claim: "eidos has no internal deps.".to_owned(),
            evidence: vec![],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-1".to_owned(),
        })
        .await
        .expect("put");
    let results = store.search("tokio").await.expect("search");
    assert_eq!(results.len(), 1, "expected 1 result for 'tokio'");
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    let result_id = results[0].id.clone();
    assert_eq!(result_id, "test.search.one");
}

#[tokio::test]
async fn search_empty_when_no_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    store
        .put(ArchitectureFact {
            id: "test.search.nomatch".to_owned(),
            scope: FactScope::Crate,
            claim: "Something unrelated.".to_owned(),
            evidence: vec![],
            mneme_session: None,
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_by: "PR-1".to_owned(),
        })
        .await
        .expect("put");
    let results = store.search("xyzzy_not_found").await.expect("search");
    assert!(
        results.is_empty(),
        "expected no results for nonexistent query"
    );
}

#[tokio::test]
async fn list_returns_empty_when_dir_missing() {
    // Directory that does not exist — should return empty, not error.
    let store = FactStore::new("/tmp/aletheia-facts-does-not-exist-xyzzy-12345");
    let result = store.list(None).await.expect("list");
    assert!(result.is_empty());
}

#[tokio::test]
async fn search_returns_empty_when_dir_missing() {
    // Directory that does not exist — should return empty, not error.
    let store = FactStore::new("/tmp/aletheia-facts-does-not-exist-xyzzy-search");
    let result = store.search("tokio").await.expect("search");
    assert!(result.is_empty());
}

#[tokio::test]
async fn list_reuses_cache_after_source_file_removed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    store
        .put(minimal_fact(
            "test.cache.one",
            FactScope::Concept,
            "Cached.",
        ))
        .await
        .expect("put");
    let first = store.list(None).await.expect("list");
    assert_eq!(first.len(), 1, "expected fact before removal");

    // Remove the backing file. A second list that re-reads disk would return empty.
    let path = dir.path().join(FactStore::id_to_filename("test.cache.one"));
    tokio::fs::remove_file(&path).await.expect("remove file");

    let second = store.list(None).await.expect("list cached");
    assert_eq!(second.len(), 1, "list should reuse the in-memory cache");
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    let cached_id = second[0].id.clone();
    assert_eq!(cached_id, "test.cache.one");
}

#[tokio::test]
async fn search_reuses_cache_after_source_file_removed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    store
        .put(minimal_fact(
            "test.cache.search",
            FactScope::Boundary,
            "Find me.",
        ))
        .await
        .expect("put");
    let first = store.search("find").await.expect("search");
    assert_eq!(first.len(), 1);

    let path = dir
        .path()
        .join(FactStore::id_to_filename("test.cache.search"));
    tokio::fs::remove_file(&path).await.expect("remove file");

    let second = store.search("FIND").await.expect("search cached");
    assert_eq!(second.len(), 1, "search should reuse the in-memory cache");
    #[expect(clippy::indexing_slicing, reason = "test assertion: len checked above")]
    let cached_id = second[0].id.clone();
    assert_eq!(cached_id, "test.cache.search");
}

#[tokio::test]
async fn put_updates_cached_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    store
        .put(minimal_fact("test.cache.a", FactScope::Concept, "A"))
        .await
        .expect("put");
    let first = store.list(None).await.expect("list");
    assert_eq!(first.len(), 1, "expected one fact after initial put");

    store
        .put(minimal_fact("test.cache.b", FactScope::Crate, "B"))
        .await
        .expect("put second");
    let second = store.list(None).await.expect("list after second put");
    assert_eq!(
        second.len(),
        2,
        "put should keep the in-memory cache up to date"
    );
}

#[tokio::test]
async fn search_matches_id_scope_and_claim_case_insensitively() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    store
        .put(minimal_fact(
            "test.cache.id",
            FactScope::Crate,
            "Claim text.",
        ))
        .await
        .expect("put");

    let by_id = store.search("TEST.CACHE.ID").await.expect("search id");
    assert_eq!(by_id.len(), 1, "search should match id case-insensitively");

    let by_scope = store.search("CRATE").await.expect("search scope");
    assert_eq!(
        by_scope.len(),
        1,
        "search should match scope case-insensitively"
    );

    let by_claim = store.search("CLAIM TEXT").await.expect("search claim");
    assert_eq!(
        by_claim.len(),
        1,
        "search should match claim case-insensitively"
    );
}

#[test]
fn seed_facts_are_valid() {
    let facts = seed_facts();
    assert_eq!(facts.len(), 5, "expected 5 seed facts");
    for fact in &facts {
        assert!(!fact.id.is_empty(), "fact id must not be empty");
        assert!(!fact.claim.is_empty(), "fact claim must not be empty");
        assert_eq!(fact.updated_by, "PR-3789");
    }
}

#[test]
fn provider_routing_seed_evidence_resolves() {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root");
    let fact = seed_facts()
        .into_iter()
        .find(|fact| fact.id == "aletheia.providers.llm.routing")
        .expect("provider routing seed fact");

    let repo_paths: Vec<&str> = fact
        .evidence
        .iter()
        .map(String::as_str)
        .filter(|path| path.starts_with("crates/"))
        .collect();
    assert!(!repo_paths.is_empty(), "seed fact should cite repo paths");
    for path in repo_paths {
        assert!(
            workspace_root.join(path).exists(),
            "seed evidence path should resolve: {path}"
        );
    }
}

#[test]
fn id_to_filename_percent_encodes_separators_without_collapsing_dash() {
    let name = FactStore::id_to_filename("aletheia/spawn/model");
    assert_eq!(name, "id-aletheia%2Fspawn%2Fmodel.json");
    let backslash = FactStore::id_to_filename("aletheia\\spawn\\model");
    assert_eq!(backslash, "id-aletheia%5Cspawn%5Cmodel.json");
    let dash = FactStore::id_to_filename("aletheia-spawn-model");
    assert_eq!(dash, "id-aletheia-spawn-model.json");

    assert_ne!(name, dash, "slash id must not collapse into dash id");
    assert_ne!(
        backslash, dash,
        "backslash id must not collapse into dash id"
    );
    assert_ne!(
        name, backslash,
        "slash and backslash ids must not collapse together"
    );
}

#[test]
fn id_to_filename_percent_encodes_unicode_bytes() {
    let unicode_id = "aletheia.\u{03b4}\u{03bf}\u{03ba}\u{03b9}\u{03bc}\u{03ae}";
    let name = FactStore::id_to_filename(unicode_id);

    assert_eq!(
        name,
        "id-aletheia.%CE%B4%CE%BF%CE%BA%CE%B9%CE%BC%CE%AE.json"
    );
}

#[test]
fn id_to_filename_hashes_long_ids_to_bounded_filename() {
    let long_id = format!("aletheia.{}", "long-segment.".repeat(40));
    let name = FactStore::id_to_filename(&long_id);

    assert!(name.starts_with(HASH_FILENAME_PREFIX));
    assert!(name.ends_with(JSON_SUFFIX));
    assert_eq!(
        name.len(),
        HASH_FILENAME_PREFIX.len() + 64 + JSON_SUFFIX.len()
    );
    assert!(
        !name.contains('/') && !name.contains('\\'),
        "hashed filename must not contain path separators"
    );
}

#[tokio::test]
async fn put_get_keeps_separator_dash_unicode_and_long_ids_distinct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    let long_id = format!("aletheia.{}", "long-segment.".repeat(40));
    let ids = [
        "a/b".to_owned(),
        "a\\b".to_owned(),
        "a-b".to_owned(),
        "aletheia.\u{03b4}\u{03bf}\u{03ba}\u{03b9}\u{03bc}\u{03ae}".to_owned(),
        long_id,
    ];

    for id in &ids {
        store
            .put(minimal_fact(id, FactScope::Concept, id))
            .await
            .expect("put distinct fact");
    }

    for id in &ids {
        let got = store
            .get(id)
            .await
            .expect("get distinct fact")
            .expect("fact should exist");
        assert_eq!(got.id, *id);
        assert_eq!(got.claim, *id);
    }

    let all = store.list(None).await.expect("list");
    assert_eq!(all.len(), ids.len(), "all distinct ids should be stored");
}

#[tokio::test]
async fn put_rejects_mapped_file_with_different_embedded_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FactStore::new(dir.path());
    let target_id = "a/b";
    let path = dir.path().join(FactStore::id_to_filename(target_id));
    let existing = minimal_fact("a-b", FactScope::Concept, "Existing fact.");
    let existing_json = serde_json::to_vec_pretty(&existing).expect("serialise");
    tokio::fs::write(&path, existing_json)
        .await
        .expect("write existing file");

    let err = store
        .put(minimal_fact(target_id, FactScope::Concept, "New fact."))
        .await
        .expect_err("put should reject mapped file with a different id");

    assert!(matches!(err, FactError::FilenameCollision { .. }));
    let persisted = FactStore::read_fact_file(&path)
        .await
        .expect("read persisted fact");
    assert_eq!(persisted.id, "a-b", "collision must not overwrite file");
}
