//! E05 (close/reopen recall) and the bounded initialization-I/O assertion.
//!
//! Needs the `storage-fjall` feature — an in-memory index can't meaningfully
//! test "close the Db and reopen it". Both tests exercise the whole public
//! script surface (`Db::run_script`) against a real on-disk fjall keyspace,
//! dropped and reopened at the same path, mirroring how `episteme`'s own
//! fjall tests establish the close/reopen idiom
//! (`knowledge_store/tests/migration.rs`).
#![cfg(feature = "storage-fjall")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::DataValue;
use crate::data::tuple::{Tuple, TupleT};
use crate::data::value::ValidityTs;
use crate::error::InternalResult;
use crate::runtime::db::ScriptMutability;
use crate::runtime::relation::RelationId;
use crate::storage::error::StorageResult;
use crate::storage::fjall_backend::{
    FjallStorage, FjallTx, initialize_krites_storage, new_krites_fjall, open_krites_fjall_storage,
};
use crate::storage::{Storage, StoreTx};

type TestDb = crate::runtime::db::Db<FjallStorage>;

fn open(path: &Path) -> TestDb {
    new_krites_fjall(path).unwrap()
}

fn run(db: &TestDb, script: &str) {
    db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable)
        .unwrap();
}

fn create_index(db: &TestDb) {
    run(db, ":create v { id: Int => vec: <F32; 4> }");
    run(
        db,
        r"::hnsw create v:idx {
            dim: 4, m: 16, dtype: F32, fields: [vec], distance: L2, ef_construction: 60,
            extend_candidates: false, keep_pruned_connections: false,
        }",
    );
}

/// Deterministic, well-separated coordinates — not random, so the test's
/// own exact-kNN reference is reproducible without a seeded RNG dependency.
#[expect(
    clippy::cast_precision_loss,
    reason = "test fixture with small integers"
)]
fn vec_for(i: usize) -> [f32; 4] {
    let base = i as f32;
    [base, base * 0.3, ((i % 11) * 2) as f32, (i % 17) as f32]
}

fn insert_all(db: &TestDb, ids: impl Iterator<Item = usize>) {
    for i in ids {
        let v = vec_for(i);
        run(
            db,
            &format!(
                "?[id, vec] <- [[{i}, vec([{},{},{},{}])]] :put v {{}}",
                v[0], v[1], v[2], v[3]
            ),
        );
    }
}

fn search<S>(db: &crate::runtime::db::Db<S>, query: [f32; 4], k: usize) -> Vec<i64>
where
    S: for<'s> Storage<'s>,
{
    let res = db
        .run_script(
            &format!(
                "?[id, dist] := ~v:idx{{id | query: vec([{},{},{},{}]), k: {k}, ef: 200, bind_distance: dist}} :order dist",
                query[0], query[1], query[2], query[3]
            ),
            BTreeMap::new(),
            ScriptMutability::Immutable,
        )
        .unwrap();
    res.rows.iter().filter_map(|r| r[0].get_int()).collect()
}

fn exact_topk(present: &[usize], query: [f32; 4], k: usize) -> Vec<i64> {
    let mut scored: Vec<(f64, i64)> = present
        .iter()
        .map(|&i| {
            let v = vec_for(i);
            let d: f64 = v
                .iter()
                .zip(query.iter())
                .map(|(a, b)| f64::from(a - b).powi(2))
                .sum();
            (d, i64::try_from(i).unwrap_or(i64::MAX))
        })
        .collect();
    scored.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("finite test coordinates"));
    scored.into_iter().take(k).map(|(_, id)| id).collect()
}

fn recall(approx: &[i64], exact: &[i64]) -> f64 {
    let exact_set: HashSet<_> = exact.iter().collect();
    let hits = approx.iter().filter(|id| exact_set.contains(id)).count();
    #[expect(clippy::cast_precision_loss, reason = "small test-scale counts")]
    {
        hits as f64 / exact.len() as f64
    }
}

/// The current entry point's own id, read the same way production search
/// does (`scan_bounded_prefix([i64::MIN], [1])`), so the "entry-point-
/// adjacent subset" this test deletes is genuinely adjacent, not a guess.
fn entry_point_id(db: &TestDb) -> i64 {
    let tx = db.transact().unwrap();
    let base = tx.get_relation("v", false).unwrap();
    let (idx_handle, _manifest) = base.hnsw_indices.get("idx").unwrap().clone();
    let row = idx_handle
        .scan_bounded_prefix(
            &tx,
            &[],
            &[DataValue::from(i64::MIN)],
            &[DataValue::from(1)],
        )
        .next()
        .expect("index must have an entry point after inserts")
        .unwrap();
    row[1].get_int().expect("entry point id column is an Int")
}

/// E05: insert N, close the Db, reopen, search — recall against the exact
/// oracle must hold. Then delete an entry-point-adjacent subset, close,
/// reopen, search again — recall must still hold, and the deleted ids must
/// never reappear.
///
/// Recall is averaged over a 15-query set, not asserted per individual
/// query — the standard ANN-benchmarking convention, and not merely a
/// convenience here: removing the entry point's own neighbourhood measurably
/// degrades recall for *some* queries at this graph scale with no repair
/// pass on delete, a real effect confirmed against the *derived* module
/// (same test shape, unmodified: mean post-delete recall ~0.87, but with a
/// heavy tail — single-trial dips as low as 0.0 observed at n=300 and
/// ~0.27-0.46 at n=1000 across independent process runs). Two implementation-
/// independent sources plausibly widen that tail further: (a) HNSW gives no
/// recall guarantee after adversarial hub deletion with no repair pass, and
/// (b) `PriorityQueue`'s hash-backed internal ordering means tie-broken
/// iteration order over equal-distance candidates is not guaranteed stable
/// across process runs, so two "identical" runs can walk the diversity
/// heuristic in a different order on ties. Both apply to `super::hnsw`
/// unmodified, not only here. The floor below is set from this module's own
/// worst observed sample (0.07) with headroom, not guessed — a strict
/// per-run floor tight enough to be a precise regression gate is not
/// available without seeding the RNG and pinning `PriorityQueue`'s hasher,
/// which is out of this wave's scope.
///
/// The `hnsw-recall:` eprintln markers exist so #6952's discriminator
/// harness (scripts/hnsw-recall-discriminator.py, run via the
/// hnsw-recall-discriminator workflow) can harvest the recall distribution
/// from passing runs — a value visible only inside a failure message can
/// never build the distribution that decides whether the 0.05 floor is
/// vestigial or load-bearing. The post-delete assertion is split so an exact
/// 0.00 (the index came back dead) reads differently from a sub-floor
/// nonzero miss (the index came back degraded): today both print "average
/// recall too low" and only one of them means the graph walk found nothing.
#[test]
fn close_reopen_preserves_recall_across_inserts_and_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    let n = 1000;

    {
        let db = open(path);
        create_index(&db);
        insert_all(&db, 0..n);
    } // db dropped here: fjall keyspace closed, lock released.

    let present: Vec<usize> = (0..n).collect();

    // Query points are exactly `vec_for(target)`, so each query's true
    // nearest neighbour is unambiguous (itself, distance 0) instead of a
    // hand-picked coordinate whose nearest id is a coincidence of the
    // vec_for formula's modular components.
    let ep_before = {
        let db = open(path);
        entry_point_id(&db)
    };
    let ep = usize::try_from(ep_before).unwrap();
    // 15 query targets spread every 1/16th of the ring away from the entry
    // point, so the 15-id neighbourhood this test deletes below (tightly
    // clustered around `ep`) can never overlap a query region — deletion
    // adjacency and query-region choice are deliberately disjoint, not
    // merely "usually far enough".
    let step = n / 16;
    let query_targets: Vec<usize> = (1..16).map(|k| (ep + k * step) % n).collect();
    let queries: Vec<[f32; 4]> = query_targets.iter().map(|&i| vec_for(i)).collect();

    {
        let db = open(path);
        let avg = average_recall(&db, &queries, &present, 10);
        eprintln!("hnsw-recall: phase=post-reopen avg={avg:.4}");
        assert!(avg >= 0.85, "post-reopen average recall too low ({avg:.2})");
    }

    let to_delete: Vec<i64> = {
        let db = open(path);
        let adjacent = search(&db, vec_for(ep), 15);
        for id in &adjacent {
            run(&db, &format!("?[id] <- [[{id}]] :rm v {{}}"));
        }
        adjacent
    }; // db dropped here: fjall keyspace closed, lock released.

    let remaining: Vec<usize> = present
        .into_iter()
        .filter(|i| !to_delete.contains(&i64::try_from(*i).unwrap_or(i64::MAX)))
        .collect();

    {
        let db = open(path);
        for &q in &queries {
            let approx = search(&db, q, 10);
            for id in &approx {
                assert!(
                    !to_delete.contains(id),
                    "deleted id {id} reappeared after reopen"
                );
            }
        }
        let avg = average_recall(&db, &queries, &remaining, 10);
        eprintln!("hnsw-recall: phase=post-delete-reopen avg={avg:.4}");
        assert!(
            avg > 0.0,
            "post-delete-and-reopen average recall is exactly 0.00 — every query returned zero \
             relevant neighbours, so the index came back dead (empty graph, lost entry point, or \
             a reopen that silently rebuilt nothing), not degraded. #6952: this is a different \
             failure class from a sub-floor nonzero miss and must not be retried into passing; \
             reproduce it with the hnsw-recall-discriminator workflow and treat it as a \
             persistence defect until that measurement says otherwise"
        );
        assert!(
            avg >= 0.05,
            "post-delete-and-reopen average recall {avg:.2} is below the 0.05 floor but nonzero — \
             the index came back alive and degraded, a different failure class than the \
             intermittent exact 0.00. #6952: a sustained sub-floor distribution is evidence the \
             floor itself needs re-deriving (raise it toward the sibling's 0.85, or document what \
             makes this path genuinely worse), not a flake to retry"
        );
    }
}

fn average_recall(db: &TestDb, queries: &[[f32; 4]], present: &[usize], k: usize) -> f64 {
    let per_query: Vec<f64> = queries
        .iter()
        .map(|&q| {
            let approx = search(db, q, k);
            let exact = exact_topk(present, q, k);
            recall(&approx, &exact)
        })
        .collect();
    #[expect(clippy::cast_precision_loss, reason = "small test-scale counts")]
    {
        per_query.iter().sum::<f64>() / per_query.len() as f64
    }
}

#[derive(Clone)]
struct InitializationTracingStorage {
    inner: FjallStorage,
    accesses: Arc<Mutex<Vec<InitializationAccess>>>,
}

struct InitializationTracingTx<'s> {
    inner: FjallTx<'s>,
    accesses: Arc<Mutex<Vec<InitializationAccess>>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum InitializationAccess {
    Transaction { write: bool },
    Get { key: Vec<u8>, for_update: bool },
    Put,
    SupportsParallelPut,
    ParallelPut,
    Delete,
    PersistedRangeDelete,
    Exists,
    Commit,
    TupleRangeScan,
    ValidityRangeScan,
    RawRangeScan,
    RangeCount,
    Compact,
    BatchPut,
}

fn record_access(accesses: &Mutex<Vec<InitializationAccess>>, access: InitializationAccess) {
    accesses.lock().unwrap().push(access);
}

impl<'s> Storage<'s> for InitializationTracingStorage {
    type Tx = InitializationTracingTx<'s>;

    fn storage_kind(&self) -> &'static str {
        self.inner.storage_kind()
    }

    fn transact(&'s self, write: bool) -> StorageResult<Self::Tx> {
        record_access(&self.accesses, InitializationAccess::Transaction { write });
        Ok(InitializationTracingTx {
            inner: self.inner.transact(write)?,
            accesses: Arc::clone(&self.accesses),
        })
    }

    fn range_compact(&'s self, lower: &[u8], upper: &[u8]) -> StorageResult<()> {
        record_access(&self.accesses, InitializationAccess::Compact);
        self.inner.range_compact(lower, upper)
    }

    fn batch_put<'a>(
        &'a self,
        data: Box<dyn Iterator<Item = StorageResult<(Vec<u8>, Vec<u8>)>> + 'a>,
    ) -> StorageResult<()> {
        record_access(&self.accesses, InitializationAccess::BatchPut);
        self.inner.batch_put(data)
    }
}

impl<'s> StoreTx<'s> for InitializationTracingTx<'s> {
    fn get(&self, key: &[u8], for_update: bool) -> StorageResult<Option<Vec<u8>>> {
        record_access(
            &self.accesses,
            InitializationAccess::Get {
                key: key.to_vec(),
                for_update,
            },
        );
        self.inner.get(key, for_update)
    }

    fn put(&mut self, key: &[u8], val: &[u8]) -> StorageResult<()> {
        record_access(&self.accesses, InitializationAccess::Put);
        self.inner.put(key, val)
    }

    fn supports_par_put(&self) -> bool {
        record_access(&self.accesses, InitializationAccess::SupportsParallelPut);
        self.inner.supports_par_put()
    }

    fn par_put(&self, key: &[u8], val: &[u8]) -> StorageResult<()> {
        record_access(&self.accesses, InitializationAccess::ParallelPut);
        self.inner.par_put(key, val)
    }

    fn del(&mut self, key: &[u8]) -> StorageResult<()> {
        record_access(&self.accesses, InitializationAccess::Delete);
        self.inner.del(key)
    }

    fn del_range_from_persisted(&mut self, lower: &[u8], upper: &[u8]) -> StorageResult<()> {
        record_access(&self.accesses, InitializationAccess::PersistedRangeDelete);
        self.inner.del_range_from_persisted(lower, upper)
    }

    fn exists(&self, key: &[u8], for_update: bool) -> StorageResult<bool> {
        record_access(&self.accesses, InitializationAccess::Exists);
        self.inner.exists(key, for_update)
    }

    fn commit(&mut self) -> StorageResult<()> {
        record_access(&self.accesses, InitializationAccess::Commit);
        self.inner.commit()
    }

    fn range_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a>
    where
        's: 'a,
    {
        record_access(&self.accesses, InitializationAccess::TupleRangeScan);
        self.inner.range_scan_tuple(lower, upper)
    }

    fn range_skip_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
        valid_at: ValidityTs,
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a> {
        record_access(&self.accesses, InitializationAccess::ValidityRangeScan);
        self.inner.range_skip_scan_tuple(lower, upper, valid_at)
    }

    fn range_scan<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = InternalResult<(Vec<u8>, Vec<u8>)>> + 'a>
    where
        's: 'a,
    {
        record_access(&self.accesses, InitializationAccess::RawRangeScan);
        self.inner.range_scan(lower, upper)
    }

    fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> StorageResult<usize>
    where
        's: 'a,
    {
        record_access(&self.accesses, InitializationAccess::RangeCount);
        self.inner.range_count(lower, upper)
    }
}

fn initialize_with_access_trace(
    path: &Path,
) -> (
    crate::runtime::db::Db<InitializationTracingStorage>,
    Arc<Mutex<Vec<InitializationAccess>>>,
) {
    // WHY: Fjall recovery legitimately replays its journal and opens its LSM tables.
    // Open the backend first so those storage-engine costs are outside the
    // boundary this test observes; only Krites initialization is counted.
    let storage = open_krites_fjall_storage(path).unwrap();
    let accesses = Arc::new(Mutex::new(Vec::new()));
    let counted = InitializationTracingStorage {
        inner: storage,
        accesses: Arc::clone(&accesses),
    };
    let db = initialize_krites_storage(counted).unwrap();
    (db, accesses)
}

/// Krites initialization must not read an HNSW graph into process memory.
/// A scan-on-open shortcut would either invoke a range operation or issue one
/// point read per graph record. The store-resident design has the same fixed
/// access signature regardless of how many vectors the index contains.
#[test]
fn initialization_storage_access_is_independent_of_index_size() {
    let small_dir = tempfile::tempdir().unwrap();
    let large_dir = tempfile::tempdir().unwrap();
    let small_n = 15;
    let large_n = 600;

    for (dir, n) in [(&small_dir, small_n), (&large_dir, large_n)] {
        let db = open(dir.path());
        create_index(&db);
        insert_all(&db, 0..n);
    }

    let expected = vec![
        InitializationAccess::Transaction { write: true },
        InitializationAccess::Get {
            key: vec![DataValue::Null].encode_as_key(RelationId::SYSTEM),
            for_update: false,
        },
        InitializationAccess::Get {
            key: vec![DataValue::Null, DataValue::from("STORAGE_VERSION")]
                .encode_as_key(RelationId::SYSTEM),
            for_update: false,
        },
        InitializationAccess::Commit,
    ];
    let (_small_db, small_accesses) = initialize_with_access_trace(small_dir.path());
    let (large_db, large_accesses) = initialize_with_access_trace(large_dir.path());

    assert_eq!(
        small_accesses.lock().unwrap().as_slice(),
        expected.as_slice(),
        "unexpected small-index initialization storage accesses"
    );
    assert_eq!(
        large_accesses.lock().unwrap().as_slice(),
        expected.as_slice(),
        "unexpected large-index initialization storage accesses"
    );

    // WHY: The probe must see the real graph range access used
    // by an HNSW search, otherwise a missing instrumentation path could make
    // the initialization assertion false-green.
    large_accesses.lock().unwrap().clear();
    assert!(!search(&large_db, vec_for(0), 10).is_empty());
    assert!(
        large_accesses
            .lock()
            .unwrap()
            .contains(&InitializationAccess::TupleRangeScan),
        "storage-access probe did not observe an HNSW graph scan"
    );
}
