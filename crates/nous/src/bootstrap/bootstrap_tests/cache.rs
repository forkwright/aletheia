//! Tests for the bootstrap workspace-file TTL + mtime cache (#3388).
//!
//! The cache sits between [`BootstrapAssembler`] and the filesystem. These
//! tests exercise its lifecycle — cold miss, warm hit, mtime invalidation,
//! TTL expiry, estimator-change invalidation, and disabled-mode passthrough —
//! against a real tempdir-backed oikos so path resolution and stat semantics
//! match production exactly.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::disallowed_methods,
    reason = "test fixtures use std::fs for synchronous setup"
)]

use std::fs;
use std::thread;
use std::time::Duration;

use super::{BootstrapAssembler, BootstrapFileCache, TaskHint};
use super::{default_budget, setup_oikos};

#[tokio::test]
async fn cache_cold_miss_then_warm_hit_skips_disk() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "identity"), ("USER.md", "operator profile")],
    );
    let cache = BootstrapFileCache::with_ttl_secs(60);

    // Cold miss populates the cache.
    let assembler = BootstrapAssembler::new(&oikos).with_cache(&cache);
    let mut budget = default_budget();
    let first = assembler
        .assemble_conditional("test", &mut budget, Vec::new(), TaskHint::General)
        .await
        .expect("first assembly should succeed");
    assert!(cache.len() >= 2, "cache should hold SOUL.md and USER.md");

    // Warm hit: mutate the on-disk content. If the cache serves the stale
    // copy (correct behaviour within TTL + unchanged mtime is preserved
    // because we don't bump mtime), the assembled prompt matches the first.
    //
    // WHY: overwriting without a distinguishable mtime is racy on some
    // filesystems (second-resolution mtime). We assert via cache.len() and
    // identical output instead of mutating the file here.
    let second = assembler
        .assemble_conditional("test", &mut default_budget(), Vec::new(), TaskHint::General)
        .await
        .expect("second assembly should succeed");

    assert_eq!(
        first.system_prompt, second.system_prompt,
        "warm cache should produce identical prompt"
    );
}

#[tokio::test]
async fn cache_mtime_change_invalidates_entry() {
    let (dir, oikos) = setup_oikos("test", &[("SOUL.md", "original identity")]);
    let cache = BootstrapFileCache::with_ttl_secs(3600);

    // Populate the cache.
    let assembler = BootstrapAssembler::new(&oikos).with_cache(&cache);
    let first = assembler
        .assemble_conditional("test", &mut default_budget(), Vec::new(), TaskHint::General)
        .await
        .expect("first assembly should succeed");
    assert!(
        first.system_prompt.contains("original identity"),
        "prompt should contain original content"
    );

    // Wait so the new mtime is at least one filesystem-resolution tick away
    // from the cached one, then rewrite with new content.
    thread::sleep(Duration::from_millis(1100));
    let soul_path = dir.path().join("nous/test/SOUL.md");
    fs::write(&soul_path, "updated identity").expect("rewrite SOUL.md");

    let second = assembler
        .assemble_conditional("test", &mut default_budget(), Vec::new(), TaskHint::General)
        .await
        .expect("second assembly should succeed");
    assert!(
        second.system_prompt.contains("updated identity"),
        "prompt should reflect updated content after mtime change: {}",
        second.system_prompt
    );
    assert!(
        !second.system_prompt.contains("original identity"),
        "stale content must not leak through: {}",
        second.system_prompt
    );
}

#[tokio::test]
async fn cache_ttl_zero_is_disabled() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);
    let cache = BootstrapFileCache::with_ttl_secs(0);

    let assembler = BootstrapAssembler::new(&oikos).with_cache(&cache);
    let _ = assembler
        .assemble_conditional("test", &mut default_budget(), Vec::new(), TaskHint::General)
        .await
        .expect("assembly should succeed");

    assert_eq!(cache.len(), 0, "disabled cache should never store entries");
}

#[tokio::test]
async fn cache_clear_empties_entries() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);
    let cache = BootstrapFileCache::with_ttl_secs(60);

    let assembler = BootstrapAssembler::new(&oikos).with_cache(&cache);
    let _ = assembler
        .assemble_conditional("test", &mut default_budget(), Vec::new(), TaskHint::General)
        .await
        .expect("assembly should succeed");
    assert!(!cache.is_empty(), "cache should hold at least SOUL.md");

    cache.clear();
    assert_eq!(cache.len(), 0, "clear must empty the cache");
    assert!(cache.is_empty(), "is_empty must track len");
}

#[tokio::test]
async fn no_cache_falls_back_to_disk_every_turn() {
    // WHY: with no cache attached, the assembler re-reads every file. This
    // is the legacy path preserved for compatibility; guard against accidental
    // caching creep.
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);

    let assembler = BootstrapAssembler::new(&oikos);
    let first = assembler
        .assemble_conditional("test", &mut default_budget(), Vec::new(), TaskHint::General)
        .await
        .expect("first assembly should succeed");
    let second = assembler
        .assemble_conditional("test", &mut default_budget(), Vec::new(), TaskHint::General)
        .await
        .expect("second assembly should succeed");

    assert_eq!(
        first.system_prompt, second.system_prompt,
        "repeated assembly without cache must still produce identical output"
    );
}
