//! Atomicity primitives for destructive schema migrations (aletheia#5779).
//!
//! Five migrations dropped a relation with `::remove` and recreated it with
//! no wrapping transaction: a crash between the drop and the recreate lost
//! the relation permanently, with no recovery path. `::rename` is the only
//! atomic swap primitive the engine offers — a single write transaction that
//! rewrites only the name-keyed metadata row (O(1), no data copy;
//! `krites/src/runtime/relation/index_management.rs:219-260`) — so every
//! destructive migration is built on staging + rename here instead of
//! drop-then-recreate.
//!
//! `MultiTransaction` cannot host any step of this sequence: it routes
//! through `DatalogScript::get_single_program()`
//! (`krites/src/parse/mod.rs:238-249`), which hard-errors on
//! `DatalogScript::Sys(_)` — every sys-op below (`::create`, `::remove`,
//! `::rename`, `::index`/`::fts`/`::hnsw`/`::lsh` create/drop) is a
//! standalone `db.run()` call, each its own transaction. Crash-safety comes
//! from [`recovery_sweep`] re-deriving a resume point on every invocation,
//! not from cross-step atomicity — `apply_pending_migrations` re-invokes the
//! same migration function on restart whenever its target version's stamp
//! is still missing, so the sweep runs before any step every time.

use std::collections::BTreeMap;

use crate::engine::{DataValue, NamedRows};

use super::KnowledgeStore;

const REBUILD_SUFFIX: &str = "__rebuild";
const RETIRED_SUFFIX: &str = "__retired";
const BATCH_ROWS: usize = 1000;
const MAX_RETIRED_REMOVE_ATTEMPTS: u32 = 3;

/// One index attached to a relation undergoing rebuild, described in enough
/// detail to reissue its creation script once the rebuilt relation is live.
pub(super) struct IndexSpec {
    /// Index name, without the `<relation>:` prefix (matches what `::indices`
    /// reports).
    pub(super) name: &'static str,
    /// Full `::… create <relation>:<name> { … }` script that recreates it.
    /// Owned rather than `&'static str` so call sites can build it from an
    /// existing DDL-text function (e.g. `fts_ddl()`) without that function
    /// needing to be `const fn`.
    pub(super) recreate_ddl: String,
}

/// Declares one destructive migration in terms the generic rebuild sequence
/// (plan §8.2) can execute.
pub(super) struct RebuildSpec<'a> {
    /// Relation being rebuilt (e.g. `"facts"`).
    pub(super) relation: &'static str,
    /// Migration label used in error messages (e.g. `"v1->v2"`).
    pub(super) label: &'static str,
    /// Schema version this migration stamps on success.
    pub(super) target_version: i64,
    /// Immutable read of every row in the relation's old shape.
    pub(super) read_script: &'static str,
    /// `:create` DDL for the relation's shape *after* this migration. The
    /// staged `<relation>__rebuild` DDL is derived from this by name
    /// substitution ([`stage_ddl`]) so no call site duplicates DDL text.
    pub(super) live_ddl: String,
    /// Indices expected to be attached to the relation before the rebuild.
    /// A live index enumerated in step 5 that isn't named here fails the
    /// migration rather than being silently dropped.
    pub(super) expected_indices: Vec<IndexSpec>,
    /// Row-shape transform: rows read in the old shape in, rows in the new
    /// shape out. May mint per-row values (e.g. a fresh ULID) — called once
    /// per rebuild attempt, not replayed on resume.
    pub(super) transform: &'a dyn Fn(&NamedRows) -> crate::error::Result<NamedRows>,
}

fn integrity_error(message: impl Into<String>) -> crate::error::Error {
    crate::error::MigrationIntegritySnafu {
        message: message.into(),
    }
    .build()
}

/// Where [`recovery_sweep`] determined a migration attempt should resume.
/// Named after the step number in plan §8.2 the run resumes from.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum ResumePoint {
    /// No trace of a prior attempt (or a repaired one): run the full
    /// sequence from step 2.
    CleanStart,
    /// The rename (step 6) committed; `<relation>__retired` still holds the
    /// old data. Resume at step 7.
    SwapCommitted,
    /// The swap and retired-removal both completed; only index recreation
    /// and the version stamp remain. Resume at step 8 (idempotent: already
    /// present indices are skipped, so this also covers a crash between 8
    /// and 9).
    Rebuilt,
}

/// Execute one destructive migration via staging + rename (plan §8.2-§8.3).
///
/// Calls `crash_injection::crash_point(N)` immediately before each numbered
/// step, matching plan §8.4's step numbering exactly — a no-op unless a test
/// binary has registered a crash hook (`crash_injection` module docs).
pub(super) fn rebuild_relation_atomically(
    store: &KnowledgeStore,
    spec: &RebuildSpec<'_>,
) -> crate::error::Result<()> {
    #[cfg(feature = "crash-injection")]
    crate::crash_injection::crash_point(1);
    match recovery_sweep(store, spec)? {
        ResumePoint::CleanStart => {
            rebuild_from_scratch(store, spec)?; // steps 2-5 (free-space precheck runs inside, F5/F6)
            #[cfg(feature = "crash-injection")]
            crate::crash_injection::crash_point(6);
            swap_rename(store, spec)?; // step 6
            #[cfg(feature = "crash-injection")]
            crate::crash_injection::crash_point(7);
            remove_retired(store, spec)?; // step 7
            #[cfg(feature = "crash-injection")]
            crate::crash_injection::crash_point(8);
            recreate_expected_indices(store, spec)?; // step 8
        }
        ResumePoint::SwapCommitted => {
            #[cfg(feature = "crash-injection")]
            crate::crash_injection::crash_point(7);
            remove_retired(store, spec)?; // step 7
            #[cfg(feature = "crash-injection")]
            crate::crash_injection::crash_point(8);
            recreate_expected_indices(store, spec)?; // step 8
        }
        ResumePoint::Rebuilt => {
            #[cfg(feature = "crash-injection")]
            crate::crash_injection::crash_point(8);
            recreate_expected_indices(store, spec)?; // step 8 (idempotent)
        }
    }
    #[cfg(feature = "crash-injection")]
    crate::crash_injection::crash_point(9);
    store.stamp_schema_version(spec.target_version, spec.label) // step 9
}

// ---------------------------------------------------------------------
// Step 1: recovery sweep
// ---------------------------------------------------------------------

/// Determine where a migration attempt should resume, repairing any state a
/// prior crash could have left that a plain retry cannot recover from on its
/// own. Never early-returns "already done" — every branch reaches a resume
/// point from which the caller runs forward through step 9.
fn recovery_sweep(
    store: &KnowledgeStore,
    spec: &RebuildSpec<'_>,
) -> crate::error::Result<ResumePoint> {
    // WHY (F5): the free-space precheck used to run here, before this match
    // classifies and repairs whatever state a prior crash left — meaning it
    // could refuse to reclaim the exact space a stale `<rel>__rebuild` is
    // squatting on, permanently wedging a boot whose data is fully intact.
    // It now runs inside `rebuild_from_scratch`, strictly after every repair
    // branch below has already discarded any reclaimable leftover, and it
    // reuses that step's own full-relation read rather than paying for a
    // second one (F6) — see this module's `free_space_precheck`.

    let rebuild_name = rebuild_name(spec.relation);
    let retired_name = retired_name(spec.relation);

    let live_exists = store.relation_exists(spec.relation)?;
    let retired_exists = store.relation_exists(&retired_name)?;
    let rebuild_exists = store.relation_exists(&rebuild_name)?;

    match (live_exists, retired_exists, rebuild_exists) {
        (true, true, _) => Ok(ResumePoint::SwapCommitted),
        (false, true, _) => {
            // WHY: `::rename` renames both pairs of a swap in one
            // transaction, so this state cannot arise from OUR OWN crash
            // mid-swap — it means something outside this sequence renamed
            // the live relation away and left it there. Repair by putting
            // the name back, then treat it as a clean start.
            undo_rename(store, &retired_name, spec.relation)?;
            // WHY (F7): the wildcard on `rebuild_exists` explicitly admits a
            // co-existing `<rel>__rebuild` in this state. Left behind, the
            // very next thing a `CleanStart` run does is `::create
            // <rel>__rebuild` (step 3), which hard-errors on an already-
            // existing relation — a repair branch that leaves the boot
            // failing is worse than no repair at all.
            if rebuild_exists {
                drop_unindexed_relation(store, &rebuild_name)?;
            }
            Ok(ResumePoint::CleanStart)
        }
        (true, false, true) => {
            // WARNING: never resume the partial insert here — some rebuild
            // transforms (v16->v17) mint a fresh ULID per row; resuming
            // would double-mint rather than reuse whatever landed before
            // the crash. Discard the partial `__rebuild` and start over.
            drop_unindexed_relation(store, &rebuild_name)?;
            Ok(ResumePoint::CleanStart)
        }
        (true, false, false) => column_probe(store, spec),
        (false, false, _) => Err(integrity_error(format!(
            "migration '{}': relation '{}' is missing entirely (no live, retired, or rebuild copy exists); repair by restoring from backup before re-running migrations",
            spec.label, spec.relation
        ))),
    }
}

/// F9: this only tells apart migrations whose target shape genuinely
/// differs from whatever shape the live relation already carries when this
/// site's rebuild starts. On `facts`/`causal_edges`/`entities`, every site
/// declares `spec.live_ddl` as the TERMINAL (current, v19-equivalent) DDL,
/// not that site's own historical intermediate shape — a decision made
/// before this staging+rename mechanism existed (`migrate_v1_to_v2`
/// already recreated with the terminal `FACTS_DDL`,
/// `knowledge_store/migration_old.rs`) and preserved here rather than
/// re-derived, since guessing at historical intermediate DDL shapes that
/// are not otherwise recorded anywhere risks introducing a NEW defect to
/// fix an old one that causes no data loss. The consequence: on a real
/// climb from v1, the first facts rebuild (v1->v2) already leaves `facts`
/// at its terminal shape, so every later facts site's probe here finds
/// `expected == live` immediately and resumes at `Rebuilt` — steps 2-7
/// never run for real on that site, only index recreation + the version
/// stamp do. Not data loss (the terminal shape is already correct), but it
/// means only `migrate_v1_to_v2` (of the facts sites) exercises this
/// module's steps 2-7 on a genuine full climb; the crash-injection matrix
/// and this module's own tests must not claim otherwise (see the
/// `full_climb_from_v1_matches_fresh_store_schema` integration test).
fn column_probe(
    store: &KnowledgeStore,
    spec: &RebuildSpec<'_>,
) -> crate::error::Result<ResumePoint> {
    // WHY: derive the "new shape" column set through the real DDL parser
    // (create a scratch relation, read its columns, remove it) rather than
    // hand-parsing DDL text — a text parser risks silently drifting from the
    // grammar (angle-bracketed vector types, quoted defaults containing
    // commas, ...). This only runs on the crash-recovery path, not the
    // steady-state hot path.
    let rebuild_name = rebuild_name(spec.relation);
    let staged = stage_ddl(&spec.live_ddl, spec.relation, REBUILD_SUFFIX)?;
    store.run_mut(&staged, BTreeMap::new())?;

    let probe = (|| -> crate::error::Result<bool> {
        let expected = column_signatures(store, &rebuild_name)?;
        let live = column_signatures(store, spec.relation)?;
        Ok(expected == live)
    })();
    let cleanup = store.run_mut(&format!("::remove {rebuild_name}"), BTreeMap::new());

    match (probe, cleanup) {
        (Err(e), _) | (Ok(_), Err(e)) => Err(e),
        (Ok(true), Ok(())) => Ok(ResumePoint::Rebuilt),
        (Ok(false), Ok(())) => Ok(ResumePoint::CleanStart),
    }
}

/// One column's full identity as reported by `::columns`: name, whether
/// it's part of the key, its type, and its default expression (if any).
///
/// F9: the prior comparison used only the column NAME set — invisible to a
/// future migration that changes a column's type, moves it between key and
/// value, or changes its default without renaming it, which `column_probe`
/// would then silently treat as "already rebuilt" and skip entirely.
type ColumnSignature = (String, bool, String, Option<String>);

fn column_signatures(
    store: &KnowledgeStore,
    relation: &str,
) -> crate::error::Result<std::collections::BTreeSet<ColumnSignature>> {
    let rows = store.run_read(&format!("::columns {relation}"), BTreeMap::new())?;
    rows.rows
        .iter()
        .map(|row| {
            let name = row
                .first()
                .and_then(DataValue::get_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    integrity_error(format!(
                        "::columns {relation} returned a row with no column-name field"
                    ))
                })?;
            let is_key = row.get(1).and_then(DataValue::get_bool).ok_or_else(|| {
                integrity_error(format!(
                    "::columns {relation} returned a row with no is_key field"
                ))
            })?;
            let column_type = row
                .get(3)
                .and_then(DataValue::get_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    integrity_error(format!(
                        "::columns {relation} returned a row with no type field"
                    ))
                })?;
            let default_expr = row.get(5).and_then(DataValue::get_str).map(str::to_owned);
            Ok((name, is_key, column_type, default_expr))
        })
        .collect()
}

fn undo_rename(store: &KnowledgeStore, from: &str, to: &str) -> crate::error::Result<()> {
    store.run_mut(&format!("::rename {from} -> {to}"), BTreeMap::new())
}

fn drop_unindexed_relation(store: &KnowledgeStore, relation: &str) -> crate::error::Result<()> {
    // INVARIANT: `<relation>__rebuild` only ever gains indices in step 8,
    // which runs strictly after the swap — a rebuild-in-progress relation
    // never carries indices in this sequence, so a plain `::remove`
    // suffices (the engine itself refuses an indexed relation, which would
    // surface as a real, fail-closed error here if that invariant broke).
    store.run_mut(&format!("::remove {relation}"), BTreeMap::new())
}

// ---------------------------------------------------------------------
// Free-space precheck
// ---------------------------------------------------------------------

/// F6: sized from `old_rows` — the same full read `rebuild_from_scratch`'s
/// step 2 already had to take for the real transform — rather than a
/// second `export_relations` call that existed purely to estimate bytes.
/// The prior shape materialised the relation twice and scanned it twice per
/// site; on the fleet's real `facts` relation across 8 sites on a full
/// climb that was a real OOM/latency risk on exactly the operation whose
/// failure mode is data loss.
///
/// WHY this does not also account for index bytes: fjall doesn't expose
/// per-relation (let alone per-index) byte usage — see the estimate below.
/// The real peak during steps 4-5 is old data + old FTS/HNSW index + staged
/// copy, and this heuristic only covers the first and third. Known,
/// documented gap; not fixed here.
fn free_space_precheck(
    store: &KnowledgeStore,
    spec: &RebuildSpec<'_>,
    old_rows: &NamedRows,
) -> crate::error::Result<()> {
    let Some(path) = store.path.as_deref() else {
        return Ok(()); // in-memory store: no filesystem headroom to check
    };

    // WHY: a conservative byte-size heuristic, not an exact on-disk
    // accounting. 2x the estimated live size covers the old copy plus the
    // staged rebuild existing side by side until the swap.
    let estimated_bytes = estimate_relation_bytes(old_rows);
    let needed_bytes = estimated_bytes.saturating_mul(2);

    let available_bytes = koina::disk_space::available_space(path).map_err(|e| {
        integrity_error(format!(
            "free-space precheck: statvfs on {} failed: {e}",
            path.display()
        ))
    })?;

    check_headroom(spec.relation, needed_bytes, available_bytes)
}

/// Pure decision extracted from [`free_space_precheck`] so the ENOSPC path
/// (plan §8.4's fault kind) is deterministically testable without needing a
/// real disk actually near-full — see this module's tests.
fn check_headroom(
    relation: &str,
    needed_bytes: u64,
    available_bytes: u64,
) -> crate::error::Result<()> {
    if available_bytes < needed_bytes {
        return Err(crate::error::MigrationDiskHeadroomSnafu {
            relation: relation.to_owned(),
            needed_bytes,
            available_bytes,
        }
        .build());
    }
    Ok(())
}

fn estimate_relation_bytes(rows: &NamedRows) -> u64 {
    rows.rows
        .iter()
        .map(|row| {
            row.iter()
                .map(estimate_value_bytes)
                .fold(0_u64, u64::saturating_add)
        })
        .fold(0_u64, u64::saturating_add)
}

/// `usize` byte-length to `u64`, saturating rather than truncating on
/// theoretical values above `u64::MAX` (never reachable in practice, but
/// keeps this a pure heuristic with no `as` cast — see `koina::disk_space`
/// for the same convention on the statvfs side of this check).
fn len_as_u64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

fn estimate_value_bytes(value: &DataValue) -> u64 {
    match value {
        DataValue::Null | DataValue::Bot | DataValue::Bool(_) => 1,
        DataValue::Num(_) => 8,
        DataValue::Str(s) => len_as_u64(s.len()).saturating_add(1),
        DataValue::Bytes(b) => len_as_u64(b.len()),
        DataValue::List(items) => items
            .iter()
            .map(estimate_value_bytes)
            .fold(0_u64, u64::saturating_add)
            .saturating_add(8),
        DataValue::Json(json) => len_as_u64(json.0.to_string().len()),
        // WHY: Uuid/Vec/Set/Regex/Validity are rare in migrated relations
        // and don't need exact accounting for a 2x-headroom heuristic.
        _ => 32,
    }
}

// ---------------------------------------------------------------------
// Steps 2-5: rebuild from scratch
// ---------------------------------------------------------------------

fn rebuild_from_scratch(
    store: &KnowledgeStore,
    spec: &RebuildSpec<'_>,
) -> crate::error::Result<()> {
    #[cfg(feature = "crash-injection")]
    crate::crash_injection::crash_point(2);
    let old_rows = store.run_read(spec.read_script, BTreeMap::new())?; // step 2
    // F5/F6: precheck now runs here — after `recovery_sweep`'s classify/
    // repair has already discarded any reclaimable leftover, and reusing
    // this step's own full read instead of a second full materialization.
    free_space_precheck(store, spec, &old_rows)?;
    #[cfg(feature = "crash-injection")]
    crate::crash_injection::crash_point(3);
    create_rebuild_relation(store, spec)?; // step 3
    let new_rows = (spec.transform)(&old_rows)?;
    #[cfg(feature = "crash-injection")]
    crate::crash_injection::crash_point(4);
    batch_insert(store, &rebuild_name(spec.relation), &new_rows)?; // step 4
    #[cfg(feature = "crash-injection")]
    crate::crash_injection::crash_point(5);
    drop_all_indices(store, spec)?; // step 5
    Ok(())
}

fn create_rebuild_relation(
    store: &KnowledgeStore,
    spec: &RebuildSpec<'_>,
) -> crate::error::Result<()> {
    let staged = stage_ddl(&spec.live_ddl, spec.relation, REBUILD_SUFFIX)?;
    store.run_mut(&staged, BTreeMap::new())
}

fn batch_insert(
    store: &KnowledgeStore,
    relation: &str,
    rows: &NamedRows,
) -> crate::error::Result<()> {
    for chunk in rows.rows.chunks(BATCH_ROWS) {
        let batch = NamedRows::new(rows.headers.clone(), chunk.to_vec());
        let (script, params) = batch.into_payload(relation, "put");
        store.run_mut(&script, params)?;
    }
    Ok(())
}

/// Derive the `<relation>__rebuild` DDL from the live-shape DDL by
/// substituting the relation name in its `:create <relation>` prefix. Every
/// DDL constant in this module starts with exactly that prefix; avoids each
/// call site hand-duplicating a second copy of its DDL text under a
/// different name.
fn stage_ddl(live_ddl: &str, relation: &str, suffix: &str) -> crate::error::Result<String> {
    let needle = format!(":create {relation}");
    let Some(pos) = live_ddl.find(&needle) else {
        return Err(integrity_error(format!(
            "DDL for '{relation}' does not start with the expected `:create {relation}` prefix"
        )));
    };
    // WHY: `split_at` (method syntax) instead of `[..pos]`/`[pos..]` index
    // syntax — both boundaries come from `find()` on an ASCII needle, so
    // they're always valid char boundaries; `split_at` gets that safety
    // without tripping the crate's `indexing_slicing`/`string_slice` lints.
    let (prefix, _) = live_ddl.split_at(pos);
    let after_needle_at = pos.saturating_add(needle.len());
    let (_, after_needle) = live_ddl.split_at(after_needle_at);

    let mut staged = String::with_capacity(live_ddl.len() + suffix.len());
    staged.push_str(prefix);
    staged.push_str(":create ");
    staged.push_str(relation);
    staged.push_str(suffix);
    staged.push_str(after_needle);
    Ok(staged)
}

// ---------------------------------------------------------------------
// Index enumeration, drop (step 5), and recreate (step 8)
// ---------------------------------------------------------------------

struct LiveIndex {
    name: String,
    kind: String,
}

fn list_live_indices(
    store: &KnowledgeStore,
    relation: &str,
) -> crate::error::Result<Vec<LiveIndex>> {
    let rows = store.run_read(&format!("::indices {relation}"), BTreeMap::new())?;
    rows.rows
        .iter()
        .map(|row| {
            let name = row
                .first()
                .and_then(DataValue::get_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    integrity_error(format!("::indices {relation} returned a row with no name"))
                })?;
            let kind = row
                .get(1)
                .and_then(DataValue::get_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    integrity_error(format!("::indices {relation} returned a row with no type"))
                })?;
            Ok(LiveIndex { name, kind })
        })
        .collect()
}

fn drop_verb_for_kind(kind: &str) -> crate::error::Result<&'static str> {
    match kind {
        "normal" => Ok("::index drop"),
        "hnsw" => Ok("::hnsw drop"),
        "fts" => Ok("::fts drop"),
        "lsh" => Ok("::lsh drop"),
        other => Err(integrity_error(format!(
            "unknown index kind '{other}' reported by ::indices"
        ))),
    }
}

/// Step 5: enumerate every index live on `spec.relation`, fail closed on
/// anything not declared in `spec.expected_indices`, then drop all of them
/// (required — `::rename` doesn't rename `{name}:{k}` sub-relations, so an
/// indexed relation carried into `<relation>__retired` cannot be removed in
/// step 7; and `destroy_relation` refuses an indexed relation outright).
fn drop_all_indices(store: &KnowledgeStore, spec: &RebuildSpec<'_>) -> crate::error::Result<()> {
    let live = list_live_indices(store, spec.relation)?;
    for idx in &live {
        if !spec.expected_indices.iter().any(|e| e.name == idx.name) {
            return Err(integrity_error(format!(
                "migration '{}': relation '{}' carries unexpected live index '{}' ({}) not declared in this migration's expected index set; investigate before re-running",
                spec.label, spec.relation, idx.name, idx.kind
            )));
        }
    }
    for idx in &live {
        let verb = drop_verb_for_kind(&idx.kind)?;
        store.run_mut(
            &format!("{verb} {}:{}", spec.relation, idx.name),
            BTreeMap::new(),
        )?;
    }
    Ok(())
}

/// Step 8: recreate every declared index, idempotently — a resume from the
/// 7->8 crash point needs to create them; a resume from the 8->9 crash point
/// needs this to be a no-op (some or all may already be live).
fn recreate_expected_indices(
    store: &KnowledgeStore,
    spec: &RebuildSpec<'_>,
) -> crate::error::Result<()> {
    let live = list_live_indices(store, spec.relation)?;
    for expected in &spec.expected_indices {
        if live.iter().any(|idx| idx.name == expected.name) {
            continue;
        }
        store.run_mut(&expected.recreate_ddl, BTreeMap::new())?;
    }

    // WHY (§8.4 "additional assertion"): `export_relations` snapshots don't
    // carry index metadata, so a byte-identity equivalence check alone
    // cannot catch a silently-missing index. Assert parity here rather than
    // relying solely on tests to notice.
    let after = list_live_indices(store, spec.relation)?;
    for expected in &spec.expected_indices {
        if !after.iter().any(|idx| idx.name == expected.name) {
            return Err(integrity_error(format!(
                "migration '{}': index '{}' is not live on '{}' after recreation",
                spec.label, expected.name, spec.relation
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Step 6: swap rename
// ---------------------------------------------------------------------

fn swap_rename(store: &KnowledgeStore, spec: &RebuildSpec<'_>) -> crate::error::Result<()> {
    let rebuild = rebuild_name(spec.relation);
    let retired = retired_name(spec.relation);
    // WHY: both pairs in one script = one transaction (`sys.rs:213-231`
    // collects all `RenameRelation` pairs and commits once) — the swap is
    // genuinely atomic, not two dependent renames.
    store.run_mut(
        &format!(
            "::rename {relation} -> {retired}, {rebuild} -> {relation}",
            relation = spec.relation
        ),
        BTreeMap::new(),
    )
}

// ---------------------------------------------------------------------
// Step 7: remove retired
// ---------------------------------------------------------------------

/// Remove `<relation>__retired`. Idempotent (a no-op if already gone, for
/// the `Rebuilt`/repeat-resume paths). On failure, force-detach whatever
/// `::indices` reports on the retired copy and retry, bounded — the known
/// trap (§8.3) is a sub-relation `{name}:{k}` that `::rename` didn't rename,
/// still attached under the retired base name, which makes `::remove` fail
/// forever on a blind retry.
fn remove_retired(store: &KnowledgeStore, spec: &RebuildSpec<'_>) -> crate::error::Result<()> {
    let retired = retired_name(spec.relation);
    if !store.relation_exists(&retired)? {
        return Ok(());
    }

    let mut attempts: u32 = 0;
    // F8: the last `::remove` failure AND the last force-detach failure, so
    // the eventual bounded-retry error (if attempts are exhausted) carries
    // the engine's own reasons rather than nothing — the prior shape used
    // `.is_ok()` on the remove and discarded the detach result entirely, so
    // an operator staring at a permanent boot wedge got zero cause. The
    // detach failure is usually the more diagnostic of the two: the outer
    // `::remove` error is typically the generic "has indices attached",
    // while the detach failure names the actual reason detachment itself
    // didn't work.
    // WARNING: no `= None` initializer on `last_remove_error` — the loop's
    // own `match` below runs first on every iteration and either returns
    // (`Ok`) or reassigns it (`Err`) before any read reachable from this
    // declaration, so an initial `None` is provably dead (rustc
    // `unused_assignments`, not cosmetic — it fails the `-D warnings` gate).
    let mut last_remove_error: Option<crate::error::Error>;
    let mut last_detach_error: Option<crate::error::Error> = None;
    loop {
        match store.run_mut(&format!("::remove {retired}"), BTreeMap::new()) {
            Ok(()) => return Ok(()),
            Err(e) => last_remove_error = Some(e),
        }
        if attempts >= MAX_RETIRED_REMOVE_ATTEMPTS {
            return Err(integrity_error(force_detach_exhausted_message(
                spec.label,
                &retired,
                attempts,
                last_remove_error.as_ref(),
                last_detach_error.as_ref(),
            )));
        }
        attempts += 1;
        let stale = list_live_indices(store, &retired)?;
        if stale.is_empty() {
            let remove_cause = last_remove_error.as_ref().map_or_else(
                || "no engine error captured".to_owned(),
                ToString::to_string,
            );
            return Err(integrity_error(format!(
                "migration '{}': retired relation '{retired}' exists, cannot be removed, and carries no live indices to force-detach; last removal error: {remove_cause}; repair by manual inspection",
                spec.label
            )));
        }
        for idx in &stale {
            let verb = drop_verb_for_kind(&idx.kind)?;
            // NOTE: the outcome (success or failure) is captured — never
            // silently swallowed into an apparent success — so the eventual
            // bounded-retry error names the real reason detachment failed,
            // not just that removal kept failing.
            if let Err(e) =
                store.run_mut(&format!("{verb} {retired}:{}", idx.name), BTreeMap::new())
            {
                last_detach_error = Some(e);
            }
        }
    }
}

fn force_detach_exhausted_message(
    label: &str,
    retired: &str,
    attempts: u32,
    last_remove_error: Option<&crate::error::Error>,
    last_detach_error: Option<&crate::error::Error>,
) -> String {
    let remove_cause = last_remove_error.map_or_else(
        || "no engine error captured".to_owned(),
        ToString::to_string,
    );
    let detach_cause = last_detach_error.map_or_else(
        || "no force-detach was attempted or all attempts reported success yet the relation is still not removable".to_owned(),
        ToString::to_string,
    );
    format!(
        "migration '{label}': retired relation '{retired}' exists and cannot be removed after {attempts} force-detach attempt(s); last removal error: {remove_cause}; last force-detach error: {detach_cause}; repair by restoring from backup or manual inspection"
    )
}

// ---------------------------------------------------------------------
// Naming helpers
// ---------------------------------------------------------------------

fn rebuild_name(relation: &str) -> String {
    format!("{relation}{REBUILD_SUFFIX}")
}

fn retired_name(relation: &str) -> String {
    format!("{relation}{RETIRED_SUFFIX}")
}

// ---------------------------------------------------------------------
// Fast prefix state-machine test (plan §8.4: "runs every PR with no child
// processes"). Drives the same private step functions production migrations
// use to construct the exact on-disk state a crash at each step would leave,
// then asserts the sweep resumes correctly and a full run converges.
// ---------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::knowledge_store::{KnowledgeConfig, KnowledgeStore};

    const WIDGET_OLD_DDL: &str = ":create widgets { id: String => label: String }";
    const WIDGET_READ: &str = "?[id, label] := *widgets{id, label}";

    fn widget_live_ddl() -> String {
        ":create widgets { id: String => label: String, tag: String }".to_owned()
    }

    // WHY: signature must match `RebuildSpec::transform`'s trait-object type
    // (`&dyn Fn(&NamedRows) -> Result<NamedRows>`) even though this
    // particular transform never fails — production transforms do.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "must match the RebuildSpec::transform trait-object signature"
    )]
    fn widget_transform(old: &NamedRows) -> crate::error::Result<NamedRows> {
        let rows: Vec<Vec<DataValue>> = old
            .rows
            .iter()
            .map(|row| {
                let mut r = row.clone();
                r.push(DataValue::Str("untagged".into()));
                r
            })
            .collect();
        Ok(NamedRows::new(
            vec!["id".to_owned(), "label".to_owned(), "tag".to_owned()],
            rows,
        ))
    }

    fn make_store() -> std::sync::Arc<KnowledgeStore> {
        KnowledgeStore::open_mem_with_config(KnowledgeConfig {
            dim: 4,
            ..Default::default()
        })
        .expect("open in-memory knowledge store")
    }

    fn seed_widgets(store: &KnowledgeStore) {
        store
            .run_mut(WIDGET_OLD_DDL, BTreeMap::new())
            .expect("create old-shape widgets");
        store
            .run_mut(
                r#"?[id, label] <- [["w1", "one"], ["w2", "two"]] :put widgets {id => label}"#,
                BTreeMap::new(),
            )
            .expect("seed widget rows");
    }

    fn widget_spec() -> RebuildSpec<'static> {
        RebuildSpec {
            relation: "widgets",
            label: "test-widgets",
            target_version: 999,
            read_script: WIDGET_READ,
            live_ddl: widget_live_ddl(),
            expected_indices: vec![],
            transform: &widget_transform,
        }
    }

    const WIDGET_LABEL_FTS_DDL: &str = "::fts create widgets:label_fts { extractor: label, tokenizer: Simple, filters: [Lowercase] }";

    fn widget_spec_with_index() -> RebuildSpec<'static> {
        RebuildSpec {
            expected_indices: vec![IndexSpec {
                name: "label_fts",
                recreate_ddl: WIDGET_LABEL_FTS_DDL.to_owned(),
            }],
            ..widget_spec()
        }
    }

    fn exported_widgets(store: &KnowledgeStore) -> NamedRows {
        store
            .db
            .export_relations(std::iter::once("widgets"))
            .expect("export widgets")
            .remove("widgets")
            .expect("widgets present in export")
    }

    #[test]
    fn sweep_clean_start_when_old_shape_untouched() {
        let store = make_store();
        seed_widgets(&store);
        let resume = recovery_sweep(&store, &widget_spec()).expect("sweep");
        assert_eq!(resume, ResumePoint::CleanStart);
    }

    #[test]
    fn sweep_missing_entirely_is_a_typed_integrity_error() {
        let store = make_store();
        let err = recovery_sweep(&store, &widget_spec()).expect_err("no widgets relation at all");
        assert!(
            err.to_string().contains("missing entirely"),
            "expected a missing-entirely integrity error, got: {err}"
        );
    }

    #[test]
    fn sweep_rebuilt_when_live_already_matches_new_shape() {
        let store = make_store();
        // Simulate a crash after 6->7->8/9 completed on-disk but before this
        // process learned about it (e.g. re-entering `apply_pending_migrations`
        // after a restart with the stamp still missing).
        store
            .run_mut(&widget_live_ddl(), BTreeMap::new())
            .expect("create new-shape widgets directly");
        let resume = recovery_sweep(&store, &widget_spec()).expect("sweep");
        assert_eq!(resume, ResumePoint::Rebuilt);
    }

    #[test]
    fn sweep_swap_committed_resumes_to_completion() {
        let store = make_store();
        seed_widgets(&store);
        let spec = widget_spec();

        // Drive steps 2-5 for real, then perform the swap (step 6) manually,
        // then stop — exactly the on-disk state a crash between step 6 and
        // step 7 leaves.
        rebuild_from_scratch(&store, &spec).expect("steps 2-5");
        swap_rename(&store, &spec).expect("step 6");

        let resume = recovery_sweep(&store, &spec).expect("sweep after simulated 6->7 crash");
        assert_eq!(resume, ResumePoint::SwapCommitted);

        // A full run from here must still converge: retired gone, stamped,
        // and (since this spec declares no indices) nothing left to recreate.
        rebuild_relation_atomically(&store, &spec).expect("resume to completion");
        assert!(
            !store.relation_exists(&retired_name("widgets")).unwrap(),
            "retired copy must be gone after resume"
        );
        let exported = exported_widgets(&store);
        assert_eq!(exported.rows.len(), 2, "both seed rows must survive resume");
        assert_eq!(
            store.schema_version().expect("schema version"),
            999,
            "resume must still stamp the target version"
        );
    }

    #[test]
    fn sweep_undoes_a_foreign_rename_and_resumes_clean() {
        let store = make_store();
        seed_widgets(&store);
        let spec = widget_spec();

        // Simulate the (false, true) defensive case: something outside this
        // sequence renamed the live relation away.
        store
            .run_mut(
                &format!("::rename widgets -> {}", retired_name("widgets")),
                BTreeMap::new(),
            )
            .expect("simulate foreign rename");

        let resume = recovery_sweep(&store, &spec).expect("sweep repairs foreign rename");
        assert_eq!(resume, ResumePoint::CleanStart);
        assert!(
            store.relation_exists("widgets").unwrap(),
            "repair must restore the live relation name"
        );
        assert!(
            !store.relation_exists(&retired_name("widgets")).unwrap(),
            "repair must not leave a retired copy behind"
        );
    }

    #[test]
    fn sweep_discards_partial_rebuild_and_resumes_clean() {
        let store = make_store();
        seed_widgets(&store);
        let spec = widget_spec();

        create_rebuild_relation(&store, &spec).expect("simulate a step-3/4 crash");

        let resume = recovery_sweep(&store, &spec).expect("sweep discards partial rebuild");
        assert_eq!(resume, ResumePoint::CleanStart);
        assert!(
            !store.relation_exists(&rebuild_name("widgets")).unwrap(),
            "partial rebuild must be discarded, not resumed"
        );
    }

    #[test]
    fn full_run_from_clean_matches_full_run_resumed_from_every_prefix() {
        // Baseline: a clean, uninterrupted run.
        let baseline_store = make_store();
        seed_widgets(&baseline_store);
        rebuild_relation_atomically(&baseline_store, &widget_spec()).expect("baseline run");
        let baseline = exported_widgets(&baseline_store);

        // Resume from "crashed after step 6" (rename committed).
        let resumed_at_7 = make_store();
        seed_widgets(&resumed_at_7);
        let spec = widget_spec();
        rebuild_from_scratch(&resumed_at_7, &spec).expect("steps 2-5");
        swap_rename(&resumed_at_7, &spec).expect("step 6");
        rebuild_relation_atomically(&resumed_at_7, &spec).expect("resume from step 7");
        assert_eq!(exported_widgets(&resumed_at_7).rows, baseline.rows);

        // Resume from "crashed after step 7" (retired removed, unstamped).
        let resumed_at_8 = make_store();
        seed_widgets(&resumed_at_8);
        let spec = widget_spec();
        rebuild_from_scratch(&resumed_at_8, &spec).expect("steps 2-5");
        swap_rename(&resumed_at_8, &spec).expect("step 6");
        remove_retired(&resumed_at_8, &spec).expect("step 7");
        rebuild_relation_atomically(&resumed_at_8, &spec).expect("resume from step 8");
        assert_eq!(exported_widgets(&resumed_at_8).rows, baseline.rows);
        assert_eq!(resumed_at_8.schema_version().expect("schema version"), 999);
    }

    #[test]
    fn resume_from_crash_between_step_8_and_9_recreates_nothing_twice_and_still_stamps() {
        // Plan §8.3's crash-safety table's last row ("8->9": `<rel>` new,
        // indexed, unstamped -> probe -> resume at 9 -> none lost) — the one
        // row not otherwise covered by an existing test, and only
        // meaningful with a spec that actually declares an index (an empty
        // `expected_indices` makes step 8 a true no-op either way).
        let baseline_store = make_store();
        seed_widgets(&baseline_store);
        let indexed_spec = widget_spec_with_index();
        rebuild_relation_atomically(&baseline_store, &indexed_spec).expect("baseline run");
        let baseline = exported_widgets(&baseline_store);

        let resumed_at_9 = make_store();
        seed_widgets(&resumed_at_9);
        let spec = widget_spec_with_index();
        rebuild_from_scratch(&resumed_at_9, &spec).expect("steps 2-5");
        swap_rename(&resumed_at_9, &spec).expect("step 6");
        remove_retired(&resumed_at_9, &spec).expect("step 7");
        recreate_expected_indices(&resumed_at_9, &spec).expect("step 8 (indices live, unstamped)");

        // A crash lands here: `widgets` is new-shape, indexed, but the
        // version stamp never ran. `column_probe` must resolve this to
        // `Rebuilt` (not `CleanStart`) so the resumed run recreates nothing
        // a second time and only stamps.
        let resume =
            recovery_sweep(&resumed_at_9, &spec).expect("sweep after simulated 8->9 crash");
        assert_eq!(resume, ResumePoint::Rebuilt);

        rebuild_relation_atomically(&resumed_at_9, &spec).expect("resume from step 9");
        assert_eq!(exported_widgets(&resumed_at_9).rows, baseline.rows);
        assert_eq!(resumed_at_9.schema_version().expect("schema version"), 999);
        let live = list_live_indices(&resumed_at_9, "widgets").expect("list widgets indices");
        assert_eq!(
            live.len(),
            1,
            "the index recreated before the simulated crash must still be exactly once live, \
             not duplicated or dropped by the resume"
        );
    }

    #[test]
    fn free_space_precheck_is_a_noop_for_in_memory_stores() {
        let store = make_store();
        seed_widgets(&store);
        // In-memory stores have no filesystem path; the precheck must not
        // error just because there's nothing to statvfs.
        free_space_precheck(&store, &widget_spec(), &NamedRows::new(vec![], vec![]))
            .expect("no-op for mem store");
    }

    /// F8: plan §8.3 named this branch the difference between recoverable
    /// and "permanent boot failure on a store whose data is intact" — but
    /// it is unreachable by construction on any path this code produces
    /// (step 5 always empties the index set before the swap), so no test
    /// in the original diff ever attached a real index to a retired
    /// relation and drove `remove_retired` against it. This constructs
    /// exactly that state directly (bypassing step 5, as an out-of-band
    /// actor would) to get real evidence rather than the absence of proof.
    ///
    /// **Result, empirically confirmed by this test**: the force-detach
    /// does NOT recover. `krites::runtime::relation::index_management::remove_index`
    /// (`index_management.rs:190-195`) reconstructs the index's sub-relation
    /// name as `"{current_parent_name}:{idx_name}"` instead of using the
    /// already-known physical name captured off the index map's own
    /// `RelationHandle` entry — a name that is only ever correct when the
    /// parent has never been renamed since the index was created. Once the
    /// swap has renamed the parent to `<rel>__retired`, the sub-relation is
    /// still physically named `<rel>:<idx_name>` (rename never touches
    /// `{name}:{k}` sub-relations — verified independently at
    /// `index_management.rs:213-254` / `relation_crud.rs:210-213`), so
    /// `destroy_relation` looks up a name that was never created and fails
    /// with "Cannot find requested stored relation"
    /// (`relation_crud.rs:142-169`). This is a **krites-level defect**, not
    /// something this call site's command string can work around: the
    /// caller only names `<parent>:<index>`, and krites' own internal
    /// reconstruction — not the caller's string — picks the wrong physical
    /// target. Out of scope for this repair (a foundational
    /// engine function used by every index kind on every relation, not a
    /// "snapshot and sweep" defect); recorded here so the bounded-retry
    /// path stays genuinely diagnosable (F8's other half, fixed above) and
    /// so this is not mistaken for "fixed" — file as its own krites issue.
    #[test]
    fn remove_retired_force_detach_cannot_recover_an_index_attached_before_the_rename() {
        let store = make_store();
        seed_widgets(&store);
        store
            .run_mut(
                "::fts create widgets:label_fts { extractor: label, tokenizer: Simple, filters: [Lowercase] }",
                BTreeMap::new(),
            )
            .expect("attach a real fts index to widgets before any rename");
        let retired = retired_name("widgets");
        store
            .run_mut(&format!("::rename widgets -> {retired}"), BTreeMap::new())
            .expect(
                "rename widgets away with the index still attached (out-of-band, as F8 posits)",
            );

        let live_on_retired =
            list_live_indices(&store, &retired).expect("list indices on the retired relation");
        assert_eq!(
            live_on_retired.len(),
            1,
            "rename_relation must carry the parent's own index map over to the new name \
             (verified: this enumeration succeeds) even though it does not rename the \
             sub-relation itself"
        );

        let spec = widget_spec();
        let err = remove_retired(&store, &spec).expect_err(
            "empirically: force-detach cannot recover this state today (krites-level defect \
             in remove_index's sub-relation name reconstruction, see this test's doc comment) \
             — if this starts passing, krites' remove_index was fixed and this test (plus its \
             doc comment) should be updated to assert success instead",
        );
        let message = err.to_string();
        assert!(
            message.contains("force-detach attempt"),
            "expected the bounded-retry exhaustion message, got: {message}"
        );
        assert!(
            message.contains("last force-detach error"),
            "F8: the bounded-retry error must surface the force-detach failure, not discard it \
             — got: {message}"
        );
        assert!(
            message.contains("Cannot find requested stored relation")
                || message.contains("not found")
                || message.contains("relation"),
            "expected the surfaced force-detach error to name a real engine cause, got: {message}"
        );
    }

    #[test]
    fn sweep_repair_of_a_foreign_rename_also_discards_a_co_existing_rebuild() {
        // F7: `(false, true, _)` explicitly admits `<rel>__rebuild` may
        // exist alongside a foreign-renamed retired copy. Left behind, the
        // very next `CleanStart` run hard-errors on step 3's
        // `::create <rel>__rebuild` finding one already there — a repair
        // branch that leaves the boot failing.
        let store = make_store();
        seed_widgets(&store);
        let spec = widget_spec();

        // Simulate the (false, true) case AND a leftover `__rebuild` from
        // some earlier, unrelated partial attempt.
        store
            .run_mut(
                &format!("::rename widgets -> {}", retired_name("widgets")),
                BTreeMap::new(),
            )
            .expect("simulate foreign rename");
        create_rebuild_relation(&store, &spec).expect("simulate a leftover __rebuild");

        let resume = recovery_sweep(&store, &spec)
            .expect("sweep must repair both the foreign rename and the leftover rebuild");
        assert_eq!(resume, ResumePoint::CleanStart);
        assert!(
            !store.relation_exists(&rebuild_name("widgets")).unwrap(),
            "the leftover __rebuild must be discarded by the repair, not left to collide with step 3"
        );

        // The repair must leave the store in a state a real run actually
        // completes from — not just a resume point that then fails.
        rebuild_relation_atomically(&store, &spec)
            .expect("a full run must succeed after this repair, not hard-error on step 3");
    }

    #[test]
    fn column_probe_full_signature_catches_a_type_change_a_name_only_comparison_would_miss() {
        // F9: the prior comparison used only the column NAME set, which is
        // blind to a column whose type (or key/value placement, or default)
        // changed without being renamed.
        let store = make_store();
        store
            .run_mut(
                ":create typed { id: String => count: String }",
                BTreeMap::new(),
            )
            .expect("create typed with a String count column");
        store
            .run_mut(
                ":create typed__rebuild { id: String => count: Int }",
                BTreeMap::new(),
            )
            .expect("create rebuild with an Int count column (same name, different type)");

        let live = column_signatures(&store, "typed").expect("read live signatures");
        let rebuild = column_signatures(&store, "typed__rebuild").expect("read rebuild signatures");

        let name_only_live: std::collections::BTreeSet<&str> =
            live.iter().map(|(name, ..)| name.as_str()).collect();
        let name_only_rebuild: std::collections::BTreeSet<&str> =
            rebuild.iter().map(|(name, ..)| name.as_str()).collect();
        assert_eq!(
            name_only_live, name_only_rebuild,
            "fixture sanity check: the two shapes must share identical column NAMES \
             so a name-only comparison would have wrongly called this a match"
        );

        assert_ne!(
            live, rebuild,
            "a type change on an identically-named column must be visible to the full-tuple \
             signature comparison, not silently treated as an already-matching shape"
        );
    }

    #[test]
    fn check_headroom_refuses_when_available_is_short() {
        // Plan §8.4's ENOSPC fault kind, exercised deterministically: the
        // decision logic itself, not a real near-full disk.
        let err = check_headroom("widgets", 1_000, 500)
            .expect_err("insufficient headroom must be refused");
        assert!(
            err.to_string().contains("insufficient disk headroom"),
            "expected a disk-headroom refusal, got: {err}"
        );
    }

    #[test]
    fn check_headroom_permits_when_available_is_sufficient() {
        check_headroom("widgets", 1_000, 1_000).expect("exactly enough headroom must pass");
        check_headroom("widgets", 1_000, 1_001).expect("more than enough headroom must pass");
    }
}
