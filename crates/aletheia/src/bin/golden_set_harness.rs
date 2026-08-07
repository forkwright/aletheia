//! Golden-set recall harness: runs a drafted query set against a
//! copy-verified snapshot of the `shared` episteme cohort and emits a
//! judging bundle for human review.
//!
//! # Two hard constraints (aletheia#5779 lineage; operator-mandated, structural)
//!
//! 1. **The psyche partition is excluded by construction, not by discipline.**
//!    `knowledge.fjall` holds `psyche` beside `shared`. This tool:
//!    - hardcodes [`SHARED_COHORT`] as the only cohort it will ever resolve
//!      (`--cohort` does not exist as a CLI argument anywhere in [`Args`] —
//!      there is no code path that can point this tool at `psyche`);
//!    - independently refuses, in [`copy_excluding_psyche`], any path
//!      component literally named `psyche` found *below* the copy root,
//!      even though the root is guaranteed by construction to be `shared`
//!      (defense in depth against a legacy drag-along nested `psyche` dir —
//!      see the doc comment on [`copy_excluding_psyche`]);
//!    - is covered by tests (`tests::` below) proving both refusals, not
//!      merely documenting them.
//!
//!    This tool sends retrieved `shared`-cohort text to a judging bundle
//!    that a human (or, later, an automated evaluator) reads — so for THIS
//!    tool psyche is out of scope entirely, full stop. Psyche may be
//!    snapshotted on-box like any other cohort (see
//!    `crates/episteme/src/knowledge_store/snapshot.rs` on
//!    `wave1/migration-atomicity`, which snapshots the psyche cohort's own
//!    root); this tool simply never does that, because it never resolves
//!    any cohort other than `shared`.
//!
//! 2. **The live keyspace is never opened.** `fjall::Keyspace` auto-recovery
//!    deletes segments absent from the levels manifest — proven to cost the
//!    fleet ~600 records once. This tool copies `shared` into a scratch
//!    directory first, verifies the copy is genuinely restorable (fjall
//!    version marker present, and a full-scan record count matches a
//!    pre-copy count taken from the source), and only ever queries the
//!    verified copy. The pattern is ported from
//!    `crates/episteme/src/knowledge_store/snapshot.rs` (`wave1/migration-atomicity`,
//!    aletheia#5779) — the write-new / verify / replace staging, the
//!    zero-background-worker verification open, and the fjall version-marker
//!    fail-closed check are all the same discipline, adapted here for a
//!    read-only eval tool rather than a pre-migration backup.
//!
//! # What this tool does NOT do (see `docs/RECALL-GOLDEN-SET.md`)
//!
//! - Does not exercise tier-2 query rewriting or graph-seeded multi-hop
//!   expansion (`KnowledgeStore::search_tiered_for_recall_scoped`) — only
//!   the fast-path hybrid search (`search_hybrid_scoped`, BM25 + vector,
//!   `seed_entities` empty unless `--seed-entities-file` is supplied).
//! - Does not judge relevance. It retrieves and hydrates content; a human
//!   judges it (`scripts/golden-set-judge.py`).

#![deny(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

/// The only cohort this tool will ever resolve or copy.
///
/// WHY hardcoded rather than a CLI argument: a `--cohort` flag would be a
/// code path that could be pointed at `psyche`. There is deliberately no
/// such flag. See module docs, constraint 1.
const SHARED_COHORT: &str = "shared";

/// Path component name refused at any depth *below* the copy root, mirroring
/// `crates/episteme/src/knowledge_store/snapshot.rs::REFUSED_COMPONENT`
/// (`wave1/migration-atomicity`). Belt-and-suspenders: [`SHARED_COHORT`]
/// already guarantees the copy root is never `psyche` itself, but a legacy
/// drag-along nested `psyche` directory inside `shared` (pre-cohort-split
/// content, per that module's own doc rationale) must still never reach the
/// snapshot.
const REFUSED_COMPONENT: &str = "psyche";

/// fjall's single on-disk keyspace name for an episteme cohort store,
/// matching `krites/src/storage/fjall_backend.rs:69` and
/// `episteme/src/knowledge_store/snapshot.rs::DATA_KEYSPACE`.
const DATA_KEYSPACE: &str = "data";

/// fjall's own version-marker filename (not re-exported by the `fjall`
/// crate — `fjall::file::VERSION_MARKER` lives in a private module). Its
/// absence is what makes `fjall::Database::open` (`== create_or_recover`)
/// silently *create* a fresh empty keyspace instead of opening an existing
/// one; checking for it ourselves before calling into fjall is what makes
/// this tool fail closed instead of "verifying" an empty directory. See
/// `episteme/src/knowledge_store/snapshot.rs::FJALL_VERSION_MARKER`.
const FJALL_VERSION_MARKER: &str = "version";

#[derive(Debug, Clone, Parser)]
#[command(
    about = "Run a drafted golden-set query set against a copy-verified snapshot of the `shared` episteme cohort."
)]
struct Args {
    /// Instance root directory (default: discovered via `ALETHEIA_ROOT` or `./instance`).
    #[arg(long)]
    instance_root: Option<PathBuf>,
    /// Scratch directory for the verified `shared`-cohort snapshot. Never the
    /// live store; safe to point at a disposable directory.
    #[arg(long)]
    work_dir: PathBuf,
    /// Path to the golden-set query JSONL file (see `GoldenQuery`).
    #[arg(long)]
    queries: PathBuf,
    /// Path to write the judging bundle (JSON) that
    /// `scripts/golden-set-judge.py` reads.
    #[arg(long)]
    out: PathBuf,
    /// Number of top results to retrieve per query.
    #[arg(long, default_value_t = 10)]
    top_k: usize,
    /// HNSW `ef` parameter (search breadth / recall-speed tradeoff).
    #[arg(long, default_value_t = 50)]
    ef: usize,
    /// Requester nous ID used for visibility-scoped retrieval. Defaults to a
    /// synthetic identity that owns no facts, so only `Shared`/`Published`
    /// visibility facts surface (see `docs/RECALL-GOLDEN-SET.md` "requester
    /// identity" section). Pass a real nous ID to additionally test that
    /// nous's own private-visibility recall.
    #[arg(long, default_value = "golden-set-harness")]
    requester_nous_id: String,
    /// Optional path to a JSON file mapping `query_id -> [EntityId, ...]`
    /// for queries that should seed the graph-traversal signal. Omit to run
    /// every query with an empty seed set (BM25 + vector signals only —
    /// the harness has no entity-resolution step; see module docs).
    #[arg(long)]
    seed_entities_file: Option<PathBuf>,
    /// Skip re-copying `shared` and reuse an existing snapshot at
    /// `<work_dir>/shared.snapshot`, if one verifies as self-consistent
    /// (fjall marker present, full readback succeeds). WARNING: this does
    /// NOT re-compare against the current live source count, so the reused
    /// snapshot may be stale relative to the live store. Use only to iterate
    /// quickly on the query set against a snapshot you just took.
    #[arg(long)]
    reuse_verified_snapshot: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    #[cfg(feature = "recall")]
    {
        run(&args)
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = args;
        anyhow::bail!(
            "golden-set-harness requires the 'recall' feature. \
             Build with: cargo build --features recall"
        );
    }
}

#[cfg(feature = "recall")]
fn run(args: &Args) -> anyhow::Result<()> {
    use mneme::embedding::create_provider;
    use mneme::knowledge_store::KnowledgeStore;

    let oikos = match &args.instance_root {
        Some(root) => taxis::oikos::Oikos::from_root(root),
        None => taxis::oikos::Oikos::discover(),
    };

    let source = cohort_source_path(&oikos);
    assert_is_shared_cohort(&source)?;

    let snapshot_dir = args.work_dir.join("shared.snapshot");
    std::fs::create_dir_all(&args.work_dir).map_err(|e| {
        anyhow::anyhow!("failed to create work dir {}: {e}", args.work_dir.display())
    })?;

    let verified_snapshot = if args.reuse_verified_snapshot && snapshot_dir.exists() {
        eprintln!(
            "REUSE: verifying existing snapshot at {} (not re-copying; may be stale relative to the live source)",
            snapshot_dir.display()
        );
        let count = count_data_keyspace_rows(&snapshot_dir)?;
        verify_restorable(&snapshot_dir, count)?;
        snapshot_dir
    } else {
        copy_and_verify_shared_cohort(&source, &snapshot_dir)?
    };
    eprintln!("VERIFIED snapshot ready at {}", verified_snapshot.display());

    let (knowledge_config, embedding_config) = knowledge_config_for_oikos(&oikos)?;
    eprintln!(
        "embedding provider={} model={}",
        embedding_config.provider,
        embedding_config.effective_model_name()
    );
    let provider = create_provider(&embedding_config).map_err(|e| {
        anyhow::anyhow!(
            "failed to create embedding provider '{}': {e}",
            embedding_config.provider
        )
    })?;

    let store = KnowledgeStore::open_fjall(&verified_snapshot, knowledge_config).map_err(|e| {
        anyhow::anyhow!(
            "failed to open verified snapshot {}: {e}",
            verified_snapshot.display()
        )
    })?;

    let queries = load_queries(&args.queries)?;
    if queries.is_empty() {
        anyhow::bail!("query set {} contains no queries", args.queries.display());
    }
    eprintln!(
        "loaded {} golden queries from {}",
        queries.len(),
        args.queries.display()
    );

    let seed_entities: HashMap<String, Vec<String>> = match &args.seed_entities_file {
        Some(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| {
                anyhow::anyhow!("failed to read seed-entities file {}: {e}", path.display())
            })?;
            serde_json::from_str(&raw).map_err(|e| {
                anyhow::anyhow!("failed to parse seed-entities file {}: {e}", path.display())
            })?
        }
        None => HashMap::new(),
    };

    let mut records = Vec::with_capacity(queries.len());
    for q in &queries {
        records.push(run_one_query(
            store.as_ref(),
            provider.as_ref(),
            q,
            seed_entities.get(&q.id).map(Vec::as_slice).unwrap_or(&[]),
            args,
        ));
    }

    let bundle = JudgingBundle {
        generated_at: jiff::Timestamp::now().to_string(),
        instance_root: oikos.root().to_path_buf(),
        source_cohort: SHARED_COHORT.to_owned(),
        snapshot_path: verified_snapshot,
        requester_nous_id: args.requester_nous_id.clone(),
        top_k: args.top_k,
        ef: args.ef,
        embedding_provider: embedding_config.provider.clone(),
        embedding_model: embedding_config.effective_model_name(),
        queries: records,
    };

    let out_file = std::fs::File::create(&args.out)
        .map_err(|e| anyhow::anyhow!("failed to create output file {}: {e}", args.out.display()))?;
    serde_json::to_writer_pretty(out_file, &bundle)
        .map_err(|e| anyhow::anyhow!("failed to write judging bundle: {e}"))?;
    eprintln!(
        "wrote judging bundle for {} queries to {}",
        bundle.queries.len(),
        args.out.display()
    );

    Ok(())
}

/// Resolve the `shared` cohort's on-disk path. The only cohort name this
/// tool ever passes to [`taxis::oikos::Oikos::knowledge_cohort_db`].
fn cohort_source_path(oikos: &taxis::oikos::Oikos) -> PathBuf {
    oikos.knowledge_cohort_db(SHARED_COHORT)
}

/// Defense-in-depth check that `path`'s final component is literally
/// `shared`. Redundant with [`cohort_source_path`] hardcoding
/// [`SHARED_COHORT`] today, but cheap insurance against a future refactor
/// that accidentally parameterizes the cohort name.
///
/// # Errors
/// Returns an error (never panics) when the final path component is
/// anything other than `shared`, including `psyche`.
fn assert_is_shared_cohort(path: &Path) -> anyhow::Result<()> {
    match path.file_name().and_then(std::ffi::OsStr::to_str) {
        Some(SHARED_COHORT) => Ok(()),
        other => anyhow::bail!(
            "refusing to operate on cohort path {} (final component {:?} is not {SHARED_COHORT:?}) — \
             this tool only ever queries the shared cohort",
            path.display(),
            other
        ),
    }
}

#[cfg(feature = "recall")]
fn knowledge_config_for_oikos(
    oikos: &taxis::oikos::Oikos,
) -> anyhow::Result<(
    mneme::knowledge_store::KnowledgeConfig,
    episteme::embedding::EmbeddingConfig,
)> {
    // WHY hard error (unlike seed_psyche_facts's silent default fallback):
    // this tool's entire purpose is measuring embedding-based retrieval
    // quality. A silent dim/model mismatch between the config used to open
    // the store and the config the store was actually built with would
    // corrupt the measurement rather than merely degrade an insert.
    let config = taxis::loader::load_config(oikos).map_err(|e| {
        anyhow::anyhow!(
            "failed to load instance config at {}: {e} — the golden-set harness needs the real \
             embedding config to avoid a silent dim/model mismatch against the live store",
            oikos.config().display()
        )
    })?;
    let embedding_config = config.embedding.to_embedding_config();
    let knowledge_config = mneme::knowledge_store::KnowledgeConfig {
        dim: config.embedding.dimension,
        embedding_model: embedding_config.effective_model_name(),
        ..Default::default()
    };
    Ok((knowledge_config, embedding_config))
}

/// A single drafted, unlabelled golden-set query.
///
/// Deliberately carries no `relevant_ids` — this harness does not read
/// record content, so it cannot know ground truth. A human fills that in
/// during judging (`scripts/golden-set-judge.py`).
#[derive(Debug, Clone, Deserialize)]
struct GoldenQuery {
    id: String,
    class: String,
    query: String,
    #[serde(default)]
    rationale: String,
}

fn load_queries(path: &Path) -> anyhow::Result<Vec<GoldenQuery>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read query set {}: {e}", path.display()))?;
    let mut queries = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for (idx, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let q: GoldenQuery = serde_json::from_str(line).map_err(|e| {
            anyhow::anyhow!(
                "{}:{}: invalid golden query JSON: {e}",
                path.display(),
                idx + 1
            )
        })?;
        if !seen_ids.insert(q.id.clone()) {
            anyhow::bail!(
                "{}:{}: duplicate query id {:?}",
                path.display(),
                idx + 1,
                q.id
            );
        }
        queries.push(q);
    }
    Ok(queries)
}

#[derive(Debug, Serialize)]
struct RetrievedItem {
    rank: usize,
    fact_id: String,
    content: String,
    rrf_score: f64,
    bm25_rank: i64,
    vec_rank: i64,
    graph_rank: i64,
    nous_id: String,
    visibility: String,
    epistemic_tier: String,
    fact_type: String,
}

#[derive(Debug, Serialize)]
struct QueryResultRecord {
    query_id: String,
    class: String,
    query: String,
    rationale: String,
    seed_entity_count: usize,
    retrieved: Vec<RetrievedItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retrieval_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct JudgingBundle {
    generated_at: String,
    instance_root: PathBuf,
    source_cohort: String,
    snapshot_path: PathBuf,
    requester_nous_id: String,
    top_k: usize,
    ef: usize,
    embedding_provider: String,
    embedding_model: String,
    queries: Vec<QueryResultRecord>,
}

#[cfg(feature = "recall")]
fn run_one_query(
    store: &mneme::knowledge_store::KnowledgeStore,
    provider: &dyn mneme::embedding::EmbeddingProvider,
    q: &GoldenQuery,
    seed_entity_strs: &[String],
    args: &Args,
) -> QueryResultRecord {
    use mneme::knowledge_store::HybridQuery;

    let base = || QueryResultRecord {
        query_id: q.id.clone(),
        class: q.class.clone(),
        query: q.query.clone(),
        rationale: q.rationale.clone(),
        seed_entity_count: seed_entity_strs.len(),
        retrieved: Vec::new(),
        retrieval_error: None,
    };

    let seed_entities: Vec<mneme::id::EntityId> = match seed_entity_strs
        .iter()
        .map(mneme::id::EntityId::new)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(ids) => ids,
        Err(e) => {
            let mut rec = base();
            rec.retrieval_error = Some(format!("invalid seed entity id: {e}"));
            return rec;
        }
    };

    let embedding = match provider.embed(&q.query) {
        Ok(v) => v,
        Err(e) => {
            let mut rec = base();
            rec.retrieval_error = Some(format!("embedding failed: {e}"));
            return rec;
        }
    };

    let hq = HybridQuery {
        text: q.query.clone(),
        embedding,
        seed_entities,
        limit: args.top_k,
        ef: args.ef,
    };

    let hits = match store.search_hybrid_scoped(&hq, &args.requester_nous_id) {
        Ok(h) => h,
        Err(e) => {
            let mut rec = base();
            rec.retrieval_error = Some(format!("hybrid search failed: {e}"));
            return rec;
        }
    };

    let mut retrieved = Vec::with_capacity(hits.len());
    for (idx, hit) in hits.iter().enumerate() {
        let fact = match store.read_visible_facts_by_id(hit.id.as_str(), &args.requester_nous_id) {
            Ok(facts) => facts
                .into_iter()
                .find(|f| !f.lifecycle.is_forgotten && f.lifecycle.superseded_by.is_none()),
            Err(_) => None,
        };
        let Some(fact) = fact else {
            // WHY: search_hybrid_scoped already filters forgotten facts, so this
            // is expected to be rare (a race, or a superseded-but-not-yet-forgotten
            // row) rather than a bug; skip rather than fail the whole query.
            continue;
        };
        retrieved.push(RetrievedItem {
            rank: idx + 1,
            fact_id: fact.id.as_str().to_owned(),
            content: fact.content,
            rrf_score: hit.rrf_score,
            bm25_rank: hit.bm25_rank,
            vec_rank: hit.vec_rank,
            graph_rank: hit.graph_rank,
            nous_id: fact.nous_id,
            visibility: fact.visibility.as_str().to_owned(),
            epistemic_tier: fact.provenance.tier.as_str().to_owned(),
            fact_type: fact.fact_type,
        });
    }

    let mut rec = base();
    rec.retrieved = retrieved;
    rec
}

// ── Copy-and-verify (ported from episteme/src/knowledge_store/snapshot.rs,
//    wave1/migration-atomicity, aletheia#5779) ───────────────────────────

/// Recursively copy `source` into `dest`, refusing any path component
/// literally named `psyche` **below** `source` itself — not the root (the
/// root is guaranteed by [`SHARED_COHORT`] + [`assert_is_shared_cohort`] to
/// never be `psyche` in this tool). Symlinks and any other non-regular
/// entry are refused outright rather than silently skipped: a partition
/// symlinked to another volume must not produce a snapshot that looks
/// complete but is truncated.
///
/// # Errors
/// Returns an error if a filesystem operation fails partway through, or a
/// non-regular entry (symlink, device node, ...) is encountered anywhere in
/// the tree. On error the destination may hold a partial copy; callers must
/// not treat that as usable (see [`copy_and_verify_shared_cohort`], which
/// gates on a subsequent open-and-read against a pre-copy count).
fn copy_excluding_psyche(source: &Path, dest: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let Some(name) = entry.path().file_name().map(std::ffi::OsStr::to_os_string) else {
                continue;
            };
            if name == REFUSED_COMPONENT {
                continue;
            }
            copy_excluding_psyche(&entry.path(), &dest.join(&name))?;
        }
        Ok(())
    } else if metadata.is_file() {
        std::fs::copy(source, dest).map(|_bytes| ())
    } else {
        Err(std::io::Error::other(format!(
            "refusing to copy non-regular filesystem entry at {}: a golden-set snapshot must not silently truncate",
            source.display()
        )))
    }
}

/// Sibling path used as the staging target for a new copy — never the live
/// snapshot directory itself. Write-new, verify, then replace: the previous
/// verified snapshot is never deleted before its replacement exists.
fn staging_sibling(snapshot_dir: &Path) -> PathBuf {
    let mut name = snapshot_dir
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".new");
    snapshot_dir.with_file_name(name)
}

/// Copy `source` (must already be the verified `shared` cohort path — see
/// [`assert_is_shared_cohort`]) into a freshly verified snapshot at
/// `snapshot_dir`, following the write-new / verify / replace discipline
/// from `episteme/src/knowledge_store/snapshot.rs::pre_migration_snapshot`.
///
/// # Errors
/// Returns an error if the source does not exist, the pre-copy count cannot
/// be taken, the copy cannot be made, or the copy cannot be opened and read
/// back with a record count matching the pre-copy count. On any failure the
/// prior verified snapshot at `snapshot_dir` (if one exists) is left
/// untouched.
fn copy_and_verify_shared_cohort(source: &Path, snapshot_dir: &Path) -> anyhow::Result<PathBuf> {
    if !source.exists() {
        anyhow::bail!(
            "shared cohort does not exist at {} — nothing to snapshot (has the instance ever run recall?)",
            source.display()
        );
    }

    let source_count = count_data_keyspace_rows(source)?;
    eprintln!(
        "COPY: {} row(s) in source shared cohort at {}",
        source_count,
        source.display()
    );

    let staging_dir = staging_sibling(snapshot_dir);
    // WHY: clear a stale `.new` left by a prior crashed attempt — never the
    // live `snapshot_dir`. A leftover partial copy from an earlier crash
    // must not silently merge with this run's copy.
    let _ = std::fs::remove_dir_all(&staging_dir);

    copy_excluding_psyche(source, &staging_dir).map_err(|e| {
        anyhow::anyhow!(
            "golden-set snapshot copy from {} to {} failed: {e}",
            source.display(),
            staging_dir.display()
        )
    })?;

    verify_restorable(&staging_dir, source_count)?;

    // WHY: POSIX rename(2) cannot atomically replace a non-empty directory.
    // Removing the old snapshot first is safe here specifically because
    // staging_dir has already been fully verified restorable above: a crash
    // in the narrow window between this removal and the rename leaves
    // staging_dir in place, still fully valid, and the next attempt's
    // stale-.new cleanup re-copies rather than promotes it.
    let _ = std::fs::remove_dir_all(snapshot_dir);
    std::fs::rename(&staging_dir, snapshot_dir).map_err(|e| {
        anyhow::anyhow!(
            "golden-set snapshot verified at {} but could not be promoted to {}: {e}",
            staging_dir.display(),
            snapshot_dir.display()
        )
    })?;

    Ok(snapshot_dir.to_path_buf())
}

/// Count records in `dir`'s `"data"` fjall keyspace via a brief, sequential,
/// opened-then-immediately-dropped handle with zero background workers —
/// never held concurrently with anything else, and never spinning up
/// flush/compaction threads for a throwaway count. Returns `0` if `dir` is
/// not (yet) a real fjall store.
fn count_data_keyspace_rows(dir: &Path) -> anyhow::Result<u64> {
    use fjall::Readable as _;

    if !dir.join(FJALL_VERSION_MARKER).is_file() {
        return Ok(0);
    }

    let db = fjall::SingleWriterTxDatabase::builder(dir)
        .worker_threads_unchecked(0)
        .open()
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to open {} to take its record count: {e}",
                dir.display()
            )
        })?;

    if !db.keyspace_exists(DATA_KEYSPACE) {
        return Ok(0);
    }
    let keyspace = db
        .keyspace(DATA_KEYSPACE, fjall::KeyspaceCreateOptions::default)
        .map_err(|e| {
            anyhow::anyhow!(
                "'data' keyspace at {} did not open for counting: {e}",
                dir.display()
            )
        })?;
    let count = db
        .read_tx()
        .len(&keyspace)
        .map_err(|e| anyhow::anyhow!("failed to count records in {}: {e}", dir.display()))?;
    Ok(u64::try_from(count).unwrap_or(u64::MAX))
}

/// **Verified, not copied-and-assumed.** Opens the COPY (never `source`)
/// with zero background workers and performs a real full-scan read whose
/// record count must equal `expected_count` (taken from `source` before the
/// copy). Fails closed: never lets fjall's create-or-recover silently
/// manufacture an empty keyspace and call that "verified".
fn verify_restorable(snapshot_dir: &Path, expected_count: u64) -> anyhow::Result<()> {
    use fjall::Readable as _;

    // F2 (episteme#5779, proven empirically): refuse to even call into fjall
    // until its own marker file is confirmed present — otherwise
    // `Database::open` treats a missing/never-copied snapshot as "create a
    // fresh empty one" and every check below "passes" against that empty store.
    if !snapshot_dir.join(FJALL_VERSION_MARKER).is_file() {
        anyhow::bail!(
            "snapshot at {} carries no fjall version marker — it was never actually written; refusing to open it \
             (fjall's create-or-recover would silently manufacture an empty one)",
            snapshot_dir.display()
        );
    }

    let db = fjall::SingleWriterTxDatabase::builder(snapshot_dir)
        .worker_threads_unchecked(0)
        .open()
        .map_err(|e| {
            anyhow::anyhow!(
                "snapshot at {} did not open — not verified-restorable: {e}",
                snapshot_dir.display()
            )
        })?;

    // F2: `Database::keyspace(name, create_options)` CREATES the keyspace
    // when absent — calling it directly on an empty copy would silently
    // manufacture the very keyspace being verified. `keyspace_exists` never
    // creates anything.
    if !db.keyspace_exists(DATA_KEYSPACE) {
        anyhow::bail!(
            "snapshot at {} opened but its 'data' keyspace does not exist — not verified-restorable",
            snapshot_dir.display()
        );
    }

    let keyspace = db
        .keyspace(DATA_KEYSPACE, fjall::KeyspaceCreateOptions::default)
        .map_err(|e| {
            anyhow::anyhow!(
                "snapshot at {} 'data' keyspace did not open — not verified-restorable: {e}",
                snapshot_dir.display()
            )
        })?;

    // WHY: a genuine full-scan read (Readable::len), not the O(1)
    // approximate_len() — "verified restorable" means the copy can actually
    // be read end to end, not merely that its metadata opened.
    let restored_count = db.read_tx().len(&keyspace).map_err(|e| {
        anyhow::anyhow!(
            "snapshot at {} opened but a full read failed — not verified-restorable: {e}",
            snapshot_dir.display()
        )
    })?;
    let restored_count = u64::try_from(restored_count).unwrap_or(u64::MAX);

    if restored_count != expected_count {
        anyhow::bail!(
            "snapshot at {} restored {restored_count} record(s) but the source held {expected_count} before the \
             copy — not verified-restorable",
            snapshot_dir.display()
        );
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        let mut file = std::fs::File::create(path).expect("create file");
        file.write_all(content.as_bytes()).expect("write file");
    }

    // ── Structural refusal: constraint 1 ─────────────────────────────────

    #[test]
    fn cohort_source_path_is_always_shared_regardless_of_instance_root() {
        for root in ["/srv/aletheia/instance", "/tmp/whatever", "/custom/root"] {
            let oikos = taxis::oikos::Oikos::from_root(root);
            let path = cohort_source_path(&oikos);
            assert_eq!(
                path.file_name().and_then(std::ffi::OsStr::to_str),
                Some(SHARED_COHORT),
                "cohort_source_path must always resolve to the shared cohort for root {root}"
            );
        }
    }

    #[test]
    fn assert_is_shared_cohort_accepts_shared_path() {
        let path = Path::new("/srv/aletheia/instance/data/knowledge.fjall/shared");
        assert!(assert_is_shared_cohort(path).is_ok());
    }

    #[test]
    fn assert_is_shared_cohort_rejects_psyche_path() {
        let path = Path::new("/srv/aletheia/instance/data/knowledge.fjall/psyche");
        let err = assert_is_shared_cohort(path).expect_err("psyche cohort path must be refused");
        assert!(
            err.to_string().contains("shared"),
            "expected refusal to mention the shared-only policy, got: {err}"
        );
    }

    #[test]
    fn assert_is_shared_cohort_rejects_arbitrary_other_cohort() {
        let path = Path::new("/srv/aletheia/instance/data/knowledge.fjall/some-other-cohort");
        assert!(assert_is_shared_cohort(path).is_err());
    }

    #[test]
    fn copy_preserves_ordinary_files_and_structure() {
        let src = tempfile::tempdir().expect("src tempdir");
        let dst = tempfile::tempdir().expect("dst tempdir");
        write(&src.path().join("a.txt"), "hello");
        write(&src.path().join("nested/b.txt"), "world");

        copy_excluding_psyche(src.path(), dst.path()).expect("copy");

        assert_eq!(
            std::fs::read_to_string(dst.path().join("a.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_to_string(dst.path().join("nested/b.txt")).unwrap(),
            "world"
        );
    }

    #[test]
    fn copy_refuses_nested_psyche_at_any_depth() {
        let src = tempfile::tempdir().expect("src tempdir");
        let dst = tempfile::tempdir().expect("dst tempdir");
        // NOTE: mirrors the legacy drag-along shape documented in
        // episteme/src/knowledge_store/snapshot.rs: a nested psyche dir
        // found while copying a DIFFERENT cohort's tree.
        write(&src.path().join("sub/psyche/0.jnl"), "journal");
        write(&src.path().join("sub/psyche/lock"), "lock");
        write(&src.path().join("sub/other/data.bin"), "ok");

        copy_excluding_psyche(src.path(), dst.path()).expect("copy");

        assert!(
            !dst.path().join("sub/psyche").exists(),
            "nested psyche must be refused at any depth below the copy root"
        );
        assert!(dst.path().join("sub/other/data.bin").exists());
    }

    #[test]
    fn copy_refuses_symlink_entries() {
        let src = tempfile::tempdir().expect("src tempdir");
        let dst = tempfile::tempdir().expect("dst tempdir");
        write(&src.path().join("real.bin"), "real data");
        std::os::unix::fs::symlink(src.path().join("real.bin"), src.path().join("linked.bin"))
            .unwrap();

        let err = copy_excluding_psyche(src.path(), dst.path())
            .expect_err("a symlinked entry must be refused, not silently skipped");
        assert!(err.to_string().contains("non-regular"), "got: {err}");
    }

    // ── Copy-and-verify: constraint 2 ────────────────────────────────────

    fn seed_fjall_store(path: &Path, rows: &[(&str, &str)]) {
        let db = fjall::SingleWriterTxDatabase::builder(path)
            .open()
            .expect("open source fjall db");
        let keyspace = db
            .keyspace(DATA_KEYSPACE, fjall::KeyspaceCreateOptions::default)
            .expect("open data keyspace");
        let mut tx = db.write_tx();
        for (k, v) in rows {
            tx.insert(&keyspace, *k, *v);
        }
        tx.commit().expect("commit seed rows");
        db.persist(fjall::PersistMode::SyncAll)
            .expect("flush before copying");
    }

    #[test]
    fn verify_restorable_fails_closed_when_marker_absent() {
        let base = tempfile::tempdir().expect("tempdir");
        let empty_dir = base.path().join("not-a-store");
        std::fs::create_dir_all(&empty_dir).unwrap();

        let err = verify_restorable(&empty_dir, 0).expect_err("no marker must never verify");
        assert!(err.to_string().contains("version marker"), "got: {err}");
    }

    #[test]
    fn verify_restorable_fails_closed_on_count_mismatch() {
        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("store");
        seed_fjall_store(&dir, &[("k1", "v1")]);

        let err = verify_restorable(&dir, 999).expect_err("count mismatch must fail closed");
        assert!(err.to_string().contains("restored 1 record"), "got: {err}");
    }

    #[test]
    fn copy_and_verify_shared_cohort_round_trips_data_and_excludes_dragged_along_psyche() {
        use fjall::Readable as _;

        let base = tempfile::tempdir().expect("tempdir");
        let source = base.path().join("data/knowledge.fjall/shared");
        let snapshot_dir = base.path().join("work/shared.snapshot");
        seed_fjall_store(&source, &[("k1", "v1"), ("k2", "v2")]);
        // NOTE: legacy drag-along nested psyche dir sitting inside the shared cohort tree.
        write(
            &source.join("legacy-psyche-drag/psyche/leftover.bin"),
            "must never be copied",
        );

        assert_is_shared_cohort(&source).expect("fixture path ends in shared");
        let result = copy_and_verify_shared_cohort(&source, &snapshot_dir).expect("copy+verify");
        assert_eq!(result, snapshot_dir);
        assert!(
            !snapshot_dir.join("legacy-psyche-drag/psyche").exists(),
            "dragged-along nested psyche must not appear in the verified snapshot"
        );

        let db = fjall::SingleWriterTxDatabase::builder(&snapshot_dir)
            .worker_threads_unchecked(0)
            .open()
            .expect("open verified snapshot");
        let keyspace = db
            .keyspace(DATA_KEYSPACE, fjall::KeyspaceCreateOptions::default)
            .unwrap();
        let read_tx = db.read_tx();
        assert_eq!(
            read_tx.get(&keyspace, "k1").unwrap().as_deref(),
            Some(b"v1".as_slice())
        );
        assert_eq!(
            read_tx.get(&keyspace, "k2").unwrap().as_deref(),
            Some(b"v2".as_slice())
        );
    }

    #[test]
    fn copy_and_verify_shared_cohort_leaves_prior_snapshot_untouched_on_stale_new_retry() {
        let base = tempfile::tempdir().expect("tempdir");
        let source = base.path().join("data/knowledge.fjall/shared");
        let snapshot_dir = base.path().join("work/shared.snapshot");
        seed_fjall_store(&source, &[("k1", "v1")]);
        copy_and_verify_shared_cohort(&source, &snapshot_dir).expect("first snapshot");

        // NOTE: simulate a crashed prior retry leaving a stale .new sibling.
        std::fs::create_dir_all(staging_sibling(&snapshot_dir).join("garbage")).unwrap();
        seed_fjall_store(&source, &[("k1", "v1"), ("k2", "v2")]);
        copy_and_verify_shared_cohort(&source, &snapshot_dir).expect("second snapshot");

        assert!(
            !staging_sibling(&snapshot_dir).exists(),
            ".new staging dir must not survive promotion"
        );
        assert_eq!(count_data_keyspace_rows(&snapshot_dir).unwrap(), 2);
    }

    // ── Query set loading ─────────────────────────────────────────────────

    #[test]
    fn load_queries_rejects_duplicate_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("q.jsonl");
        write(
            &path,
            "{\"id\":\"a\",\"class\":\"entity_lookup\",\"query\":\"x\"}\n{\"id\":\"a\",\"class\":\"entity_lookup\",\"query\":\"y\"}\n",
        );
        let err = load_queries(&path).expect_err("duplicate id must be rejected");
        assert!(err.to_string().contains("duplicate"), "got: {err}");
    }

    #[test]
    fn load_queries_skips_blank_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("q.jsonl");
        write(
            &path,
            "\n{\"id\":\"a\",\"class\":\"entity_lookup\",\"query\":\"x\"}\n\n",
        );
        let queries = load_queries(&path).expect("parse");
        assert_eq!(queries.len(), 1);
    }
}
