//! Hierarchical Navigable Small World vector index.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use crate::engine::data::expr::{Bytecode, eval_bytecode_pred};
use crate::engine::data::program::HnswSearch;
use crate::engine::data::relation::VecElementType;
use crate::engine::data::tuple::{ENCODED_KEY_MIN_LEN, Tuple};
use crate::engine::data::value::Vector;
use crate::engine::error::InternalResult as Result;
use crate::engine::parse::sys::HnswDistance;
use crate::engine::runtime::error::InvalidOperationSnafu;
use crate::engine::runtime::relation::RelationHandle;
use crate::engine::runtime::transact::SessionTx;
use crate::engine::{DataValue, SourceSpan};
use compact_str::CompactString;
use itertools::Itertools;
use lru::LruCache;
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use rand::Rng;
use rustc_hash::FxHashSet;
use std::cmp::{Reverse, max};
use std::num::NonZeroUsize;

/// Default maximum number of vectors held in the LRU cache.
/// Prevents unbounded memory growth during large index operations.
const DEFAULT_VECTOR_CACHE_CAPACITY: usize = 10_000;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct HnswIndexManifest {
    pub(crate) base_relation: CompactString,
    pub(crate) index_name: CompactString,
    pub(crate) vec_dim: usize,
    pub(crate) dtype: VecElementType,
    pub(crate) vec_fields: Vec<usize>,
    pub(crate) distance: HnswDistance,
    pub(crate) ef_construction: usize,
    pub(crate) m_neighbours: usize,
    pub(crate) m_max: usize,
    pub(crate) m_max0: usize,
    pub(crate) level_multiplier: f64,
    pub(crate) index_filter: Option<String>,
    pub(crate) extend_candidates: bool,
    pub(crate) keep_pruned_connections: bool,
}

impl HnswIndexManifest {
    fn get_random_level(&self) -> i64 {
        let mut rng = rand::rng();
        let uniform_num: f64 = rng.random_range(0.0..1.0);
        let r = -uniform_num.ln() * self.level_multiplier;
        // the level is the largest integer smaller than r
        -(r.floor() as i64)
    }
}

type CompoundKey = (Tuple, usize, i32);

struct VectorCache {
    cache: LruCache<CompoundKey, Vector>,
    distance: HnswDistance,
}

impl VectorCache {
    fn new(distance: HnswDistance, capacity: usize) -> Self {
        Self {
            cache: LruCache::new(
                NonZeroUsize::new(capacity).expect("VectorCache capacity must be > 0"),
            ),
            distance,
        }
    }
    fn insert(&mut self, k: CompoundKey, v: Vector) {
        self.cache.put(k, v);
    }
    fn dist(&self, v1: &Vector, v2: &Vector) -> Result<f64> {
        use ndarray::Zip;
        match self.distance {
            HnswDistance::L2 => match (v1, v2) {
                (Vector::F32(a), Vector::F32(b)) => {
                    Ok(Zip::from(a).and(b).fold(0.0f32, |acc, &x, &y| {
                        let d = x - y;
                        acc + d * d
                    }) as f64)
                }
                (Vector::F64(a), Vector::F64(b)) => {
                    Ok(Zip::from(a).and(b).fold(0.0f64, |acc, &x, &y| {
                        let d = x - y;
                        acc + d * d
                    }))
                }
                _ => {
                    return Err(InvalidOperationSnafu {
                        op: "hnsw_l2",
                        reason: format!("Cannot compute L2 distance between {:?} and {:?}", v1, v2),
                    }
                    .build()
                    .into());
                }
            },
            HnswDistance::Cosine => match (v1, v2) {
                (Vector::F32(a), Vector::F32(b)) => {
                    let (a_norm, b_norm, dot) = Zip::from(a)
                        .and(b)
                        .fold((0.0f32, 0.0f32, 0.0f32), |(an, bn, d), &x, &y| {
                            (an + x * x, bn + y * y, d + x * y)
                        });
                    Ok(1.0 - dot as f64 / (a_norm as f64 * b_norm as f64).sqrt())
                }
                (Vector::F64(a), Vector::F64(b)) => {
                    let (a_norm, b_norm, dot) = Zip::from(a)
                        .and(b)
                        .fold((0.0f64, 0.0f64, 0.0f64), |(an, bn, d), &x, &y| {
                            (an + x * x, bn + y * y, d + x * y)
                        });
                    Ok(1.0 - dot / (a_norm * b_norm).sqrt())
                }
                _ => {
                    return Err(InvalidOperationSnafu {
                        op: "hnsw_cosine",
                        reason: format!(
                            "Cannot compute cosine distance between {:?} and {:?}",
                            v1, v2
                        ),
                    }
                    .build()
                    .into());
                }
            },
            HnswDistance::InnerProduct => match (v1, v2) {
                (Vector::F32(a), Vector::F32(b)) => {
                    let dot = a.dot(b);
                    Ok(1. - dot as f64)
                }
                (Vector::F64(a), Vector::F64(b)) => {
                    let dot = a.dot(b);
                    Ok(1. - dot)
                }
                _ => {
                    return Err(InvalidOperationSnafu {
                        op: "hnsw_ip",
                        reason: format!(
                            "Cannot compute inner product between {:?} and {:?}",
                            v1, v2
                        ),
                    }
                    .build()
                    .into());
                }
            },
        }
    }
    // INVARIANT: callers must call ensure_key() before v_dist/k_dist/get_key.
    // The cache is guaranteed to contain the key after ensure_key succeeds
    // (though LRU eviction may have removed it if capacity is very small and
    // many keys were ensured between the ensure and the access — callers that
    // need multiple keys should ensure them close to their use site).
    fn v_dist(&mut self, v: &Vector, key: &CompoundKey) -> Result<f64> {
        let v2 = self
            .cache
            .peek(key)
            .expect("INVARIANT: ensure_key must be called before v_dist");
        self.dist(v, v2)
    }
    fn k_dist(&mut self, k1: &CompoundKey, k2: &CompoundKey) -> Result<f64> {
        // Clone to avoid overlapping borrows on the cache.
        let v1 = self
            .cache
            .peek(k1)
            .expect("INVARIANT: ensure_key must be called before k_dist")
            .clone();
        let v2 = self
            .cache
            .peek(k2)
            .expect("INVARIANT: ensure_key must be called before k_dist");
        self.dist(&v1, v2)
    }
    fn get_key(&mut self, key: &CompoundKey) -> &Vector {
        self.cache
            .peek(key)
            .expect("INVARIANT: ensure_key must be called before get_key")
    }
    fn ensure_key(
        &mut self,
        key: &CompoundKey,
        handle: &RelationHandle,
        tx: &SessionTx<'_>,
    ) -> Result<()> {
        if !self.cache.contains(key) {
            match handle.get(tx, &key.0)? {
                Some(tuple) => {
                    let mut field = &tuple[key.1];
                    if key.2 >= 0 {
                        match field {
                            DataValue::List(l) => {
                                field = &l[key.2 as usize];
                            }
                            _ => {
                                return Err(InvalidOperationSnafu {
                                    op: "hnsw_index",
                                    reason: format!("Cannot interpret {} as list", field),
                                }
                                .build()
                                .into());
                            }
                        }
                    }
                    match field {
                        DataValue::Vec(v) => {
                            self.cache.put(key.clone(), v.clone());
                        }
                        _ => {
                            return Err(InvalidOperationSnafu {
                                op: "hnsw_index",
                                reason: format!("Cannot interpret {} as vector", field),
                            }
                            .build()
                            .into());
                        }
                    }
                }
                None => {
                    return Err(InvalidOperationSnafu {
                        op: "hnsw_index",
                        reason: format!("Cannot find compound key for HNSW: {:?}", key),
                    }
                    .build()
                    .into());
                }
            }
        }
        Ok(())
    }
    #[cfg(test)]
    fn len(&self) -> usize {
        self.cache.len()
    }
}

impl<'a> SessionTx<'a> {
    fn hnsw_put_vector(
        &mut self,
        tuple: &[DataValue],
        q: &Vector,
        idx: usize,
        subidx: i32,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<()> {
        let tuple_key = &tuple[..orig_table.metadata.keys.len()];
        vec_cache.insert((tuple_key.to_vec(), idx, subidx), q.clone());
        let hash = q.get_hash();
        let mut canary_tuple = vec![DataValue::from(0)];
        for _ in 0..2 {
            canary_tuple.extend_from_slice(tuple_key);
            canary_tuple.push(DataValue::from(idx as i64));
            canary_tuple.push(DataValue::from(subidx as i64));
        }
        if let Some(v) = idx_table.get(self, &canary_tuple)? {
            if let DataValue::Bytes(b) = &v[tuple_key.len() * 2 + 6] {
                if b == hash.as_ref() {
                    return Ok(());
                }
            }
            self.hnsw_remove_vec(tuple_key, idx, subidx, orig_table, idx_table)?;
        }

        let ep_res = idx_table
            .scan_bounded_prefix(
                self,
                &[],
                &[DataValue::from(i64::MIN)],
                &[DataValue::from(0)],
            )
            .next();
        if let Some(ep) = ep_res {
            let ep = ep?;
            // bottom level since we are going up
            let bottom_level = ep[0]
                .get_int()
                .expect("HNSW index stores integers at this position");
            let ep_t_key = ep[1..orig_table.metadata.keys.len() + 1].to_vec();
            let ep_idx = ep[orig_table.metadata.keys.len() + 1]
                .get_int()
                .expect("HNSW index stores integers at this position")
                as usize;
            let ep_subidx = ep[orig_table.metadata.keys.len() + 2]
                .get_int()
                .expect("HNSW index stores integers at this position")
                as i32;
            let ep_key = (ep_t_key, ep_idx, ep_subidx);
            vec_cache.ensure_key(&ep_key, orig_table, self)?;
            let ep_distance = vec_cache.v_dist(q, &ep_key)?;
            // max queue
            let mut found_nn = PriorityQueue::new();
            found_nn.push(ep_key, OrderedFloat(ep_distance));
            let target_level = manifest.get_random_level();
            if target_level < bottom_level {
                // this becomes the entry point
                self.hnsw_put_fresh_at_levels(
                    hash.as_ref(),
                    tuple_key,
                    idx,
                    subidx,
                    orig_table,
                    idx_table,
                    target_level,
                    bottom_level - 1,
                )?;
            }
            for current_level in bottom_level..target_level {
                self.hnsw_search_level(
                    q,
                    1,
                    current_level,
                    orig_table,
                    idx_table,
                    &mut found_nn,
                    vec_cache,
                )?;
            }
            let mut self_tuple_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
            self_tuple_key.push(DataValue::from(0));
            for _ in 0..2 {
                self_tuple_key.extend_from_slice(tuple_key);
                self_tuple_key.push(DataValue::from(idx as i64));
                self_tuple_key.push(DataValue::from(subidx as i64));
            }
            let mut self_tuple_val = vec![
                DataValue::from(0.0),
                DataValue::Bytes(hash.as_ref().to_vec()),
                DataValue::from(false),
            ];
            for current_level in max(target_level, bottom_level)..=0 {
                let m_max = if current_level == 0 {
                    manifest.m_max0
                } else {
                    manifest.m_max
                };
                self.hnsw_search_level(
                    q,
                    manifest.ef_construction,
                    current_level,
                    orig_table,
                    idx_table,
                    &mut found_nn,
                    vec_cache,
                )?;
                // add bidirectional links to the nearest neighbors
                let neighbours = self.hnsw_select_neighbours_heuristic(
                    q,
                    &found_nn,
                    m_max,
                    current_level,
                    manifest,
                    idx_table,
                    orig_table,
                    vec_cache,
                )?;
                // add self-link
                self_tuple_key[0] = DataValue::from(current_level);
                self_tuple_val[0] = DataValue::from(neighbours.len() as f64);

                let self_tuple_key_bytes =
                    idx_table.encode_key_for_store(&self_tuple_key, Default::default())?;
                let self_tuple_val_bytes =
                    idx_table.encode_val_only_for_store(&self_tuple_val, Default::default())?;
                self.store_tx
                    .put(&self_tuple_key_bytes, &self_tuple_val_bytes)?;

                // add bidirectional links
                for (neighbour, Reverse(OrderedFloat(dist))) in neighbours.iter() {
                    let mut out_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                    let out_val = vec![
                        DataValue::from(*dist),
                        DataValue::Null,
                        DataValue::from(false),
                    ];
                    out_key.push(DataValue::from(current_level));
                    out_key.extend_from_slice(tuple_key);
                    out_key.push(DataValue::from(idx as i64));
                    out_key.push(DataValue::from(subidx as i64));
                    out_key.extend_from_slice(&neighbour.0);
                    out_key.push(DataValue::from(neighbour.1 as i64));
                    out_key.push(DataValue::from(neighbour.2 as i64));
                    let out_key_bytes =
                        idx_table.encode_key_for_store(&out_key, Default::default())?;
                    let out_val_bytes =
                        idx_table.encode_val_only_for_store(&out_val, Default::default())?;
                    self.store_tx.put(&out_key_bytes, &out_val_bytes)?;

                    let mut in_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                    let in_val = vec![
                        DataValue::from(*dist),
                        DataValue::Null,
                        DataValue::from(false),
                    ];
                    in_key.push(DataValue::from(current_level));
                    in_key.extend_from_slice(&neighbour.0);
                    in_key.push(DataValue::from(neighbour.1 as i64));
                    in_key.push(DataValue::from(neighbour.2 as i64));
                    in_key.extend_from_slice(tuple_key);
                    in_key.push(DataValue::from(idx as i64));
                    in_key.push(DataValue::from(subidx as i64));

                    let in_key_bytes =
                        idx_table.encode_key_for_store(&in_key, Default::default())?;
                    let in_val_bytes =
                        idx_table.encode_val_only_for_store(&in_val, Default::default())?;
                    self.store_tx.put(&in_key_bytes, &in_val_bytes)?;

                    // shrink links if necessary
                    let mut target_self_key =
                        Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                    target_self_key.push(DataValue::from(current_level));
                    for _ in 0..2 {
                        target_self_key.extend_from_slice(&neighbour.0);
                        target_self_key.push(DataValue::from(neighbour.1 as i64));
                        target_self_key.push(DataValue::from(neighbour.2 as i64));
                    }
                    let target_self_key_bytes =
                        idx_table.encode_key_for_store(&target_self_key, Default::default())?;
                    let target_self_val_bytes = match self
                        .store_tx
                        .get(&target_self_key_bytes, false)?
                    {
                        Some(bytes) => bytes,
                        None => return Err(InvalidOperationSnafu {
                            op: "hnsw_index",
                            reason: "Indexed vector not found, this signifies a bug in the index implementation".to_string(),
                        }.build().into()),
                    };
                    let mut target_self_val: Vec<DataValue> =
                        rmp_serde::from_slice(&target_self_val_bytes[ENCODED_KEY_MIN_LEN..])
                            .map_err(|e| crate::engine::error::InternalError::Runtime {
                                source: InvalidOperationSnafu {
                                    op: "hnsw_index",
                                    reason: e.to_string(),
                                }
                                .build(),
                            })?;
                    let mut target_degree = target_self_val[0]
                        .get_float()
                        .expect("HNSW index stores floats at this position")
                        as usize
                        + 1;
                    if target_degree > m_max {
                        // shrink links
                        target_degree = self.hnsw_shrink_neighbour(
                            neighbour,
                            m_max,
                            current_level,
                            manifest,
                            idx_table,
                            orig_table,
                            vec_cache,
                        )?;
                    }
                    // update degree
                    target_self_val[0] = DataValue::from(target_degree as f64);
                    self.store_tx.put(
                        &target_self_key_bytes,
                        &idx_table
                            .encode_val_only_for_store(&target_self_val, Default::default())?,
                    )?;
                }
            }
        } else {
            // This is the first vector in the index.
            let level = manifest.get_random_level();
            self.hnsw_put_fresh_at_levels(
                hash.as_ref(),
                tuple_key,
                idx,
                subidx,
                orig_table,
                idx_table,
                level,
                0,
            )?;
        }
        Ok(())
    }
    fn hnsw_shrink_neighbour(
        &mut self,
        target_key: &CompoundKey,
        m: usize,
        level: i64,
        manifest: &HnswIndexManifest,
        idx_table: &RelationHandle,
        orig_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<usize> {
        vec_cache.ensure_key(target_key, orig_table, self)?;
        let vec = vec_cache.get_key(target_key).clone();
        let mut candidates = PriorityQueue::new();
        for (neighbour_key, neighbour_dist) in
            self.hnsw_get_neighbours(target_key, level, idx_table, false)?
        {
            candidates.push(neighbour_key, OrderedFloat(neighbour_dist));
        }
        let new_candidates = self.hnsw_select_neighbours_heuristic(
            &vec,
            &candidates,
            m,
            level,
            manifest,
            idx_table,
            orig_table,
            vec_cache,
        )?;
        let mut old_candidate_set = FxHashSet::default();
        for (old, _) in &candidates {
            old_candidate_set.insert(old.clone());
        }
        let mut new_candidate_set = FxHashSet::default();
        for (new, _) in &new_candidates {
            new_candidate_set.insert(new.clone());
        }
        let new_degree = new_candidates.len();
        for (new, Reverse(OrderedFloat(new_dist))) in new_candidates {
            if !old_candidate_set.contains(&new) {
                let mut new_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                let new_val = vec![
                    DataValue::from(new_dist),
                    DataValue::Null,
                    DataValue::from(false),
                ];
                new_key.push(DataValue::from(level));
                new_key.extend_from_slice(&target_key.0);
                new_key.push(DataValue::from(target_key.1 as i64));
                new_key.push(DataValue::from(target_key.2 as i64));
                new_key.extend_from_slice(&new.0);
                new_key.push(DataValue::from(new.1 as i64));
                new_key.push(DataValue::from(new.2 as i64));
                let new_key_bytes = idx_table.encode_key_for_store(&new_key, Default::default())?;
                let new_val_bytes =
                    idx_table.encode_val_only_for_store(&new_val, Default::default())?;
                self.store_tx.put(&new_key_bytes, &new_val_bytes)?;
            }
        }
        for (old, OrderedFloat(old_dist)) in candidates {
            if !new_candidate_set.contains(&old) {
                let mut old_key = Vec::with_capacity(orig_table.metadata.keys.len() * 2 + 5);
                old_key.push(DataValue::from(level));
                old_key.extend_from_slice(&target_key.0);
                old_key.push(DataValue::from(target_key.1 as i64));
                old_key.push(DataValue::from(target_key.2 as i64));
                old_key.extend_from_slice(&old.0);
                old_key.push(DataValue::from(old.1 as i64));
                old_key.push(DataValue::from(old.2 as i64));
                let old_key_bytes = idx_table.encode_key_for_store(&old_key, Default::default())?;
                let old_existing_val = match self.store_tx.get(&old_key_bytes, false)? {
                    Some(bytes) => bytes,
                    None => {
                        return Err(InvalidOperationSnafu {
                            op: "hnsw_index",
                            reason: "Indexed vector not found, this signifies a bug in the index implementation".to_string(),
                        }.build().into())
                    }
                };
                let old_existing_val: Vec<DataValue> = rmp_serde::from_slice(
                    &old_existing_val[ENCODED_KEY_MIN_LEN..],
                )
                .map_err(|e| crate::engine::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "hnsw_index",
                        reason: e.to_string(),
                    }
                    .build(),
                })?;
                if old_existing_val[2]
                    .get_bool()
                    .expect("HNSW index stores bools at this position")
                {
                    self.store_tx.del(&old_key_bytes)?;
                } else {
                    let old_val = vec![
                        DataValue::from(old_dist),
                        DataValue::Null,
                        DataValue::from(true),
                    ];
                    let old_val_bytes =
                        idx_table.encode_val_only_for_store(&old_val, Default::default())?;
                    self.store_tx.put(&old_key_bytes, &old_val_bytes)?;
                }
            }
        }

        Ok(new_degree)
    }
    fn hnsw_select_neighbours_heuristic(
        &self,
        q: &Vector,
        found: &PriorityQueue<CompoundKey, OrderedFloat<f64>>,
        m: usize,
        level: i64,
        manifest: &HnswIndexManifest,
        idx_table: &RelationHandle,
        orig_table: &RelationHandle,
        vec_cache: &mut VectorCache,
    ) -> Result<PriorityQueue<CompoundKey, Reverse<OrderedFloat<f64>>>> {
        let mut candidates = PriorityQueue::new();
        let mut ret: PriorityQueue<CompoundKey, Reverse<OrderedFloat<_>>> = PriorityQueue::new();
        let mut discarded: PriorityQueue<_, Reverse<OrderedFloat<_>>> = PriorityQueue::new();
        for (item, dist) in found.iter() {
            // Add to candidates
            candidates.push(item.clone(), Reverse(*dist));
        }
        if manifest.extend_candidates {
            for (item, _) in found.iter() {
                // Extend by neighbours
                for (neighbour_key, _) in self.hnsw_get_neighbours(item, level, idx_table, false)? {
                    vec_cache.ensure_key(&neighbour_key, orig_table, self)?;
                    let dist = vec_cache.v_dist(q, &neighbour_key)?;
                    candidates.push(
                        (neighbour_key.0, neighbour_key.1, neighbour_key.2),
                        Reverse(OrderedFloat(dist)),
                    );
                }
            }
        }
        while !candidates.is_empty() && ret.len() < m {
            let (cand_key, Reverse(OrderedFloat(cand_dist_to_q))) = candidates
                .pop()
                .expect("checked !is_empty() in while condition");
            let mut should_add = true;
            for (existing, _) in ret.iter() {
                vec_cache.ensure_key(&cand_key, orig_table, self)?;
                vec_cache.ensure_key(existing, orig_table, self)?;
                let dist_to_existing = vec_cache.k_dist(existing, &cand_key)?;
                if dist_to_existing < cand_dist_to_q {
                    should_add = false;
                    break;
                }
            }
            if should_add {
                ret.push(cand_key, Reverse(OrderedFloat(cand_dist_to_q)));
            } else if manifest.keep_pruned_connections {
                discarded.push(cand_key, Reverse(OrderedFloat(cand_dist_to_q)));
            }
        }
        if manifest.keep_pruned_connections {
            while !discarded.is_empty() && ret.len() < m {
                let (nearest_triple, Reverse(OrderedFloat(nearest_dist))) = discarded
                    .pop()
                    .expect("checked !is_empty() in while condition");
                ret.push(nearest_triple, Reverse(OrderedFloat(nearest_dist)));
            }
        }
        Ok(ret)
    }
    fn hnsw_search_level(
        &self,
        q: &Vector,
        ef: usize,
        cur_level: i64,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        found_nn: &mut PriorityQueue<CompoundKey, OrderedFloat<f64>>,
        vec_cache: &mut VectorCache,
    ) -> Result<()> {
        let mut visited: FxHashSet<CompoundKey> = FxHashSet::default();
        // min queue
        let mut candidates: PriorityQueue<CompoundKey, Reverse<OrderedFloat<f64>>> =
            PriorityQueue::new();

        for item in found_nn.iter() {
            visited.insert(item.0.clone());
            candidates.push(item.0.clone(), Reverse(*item.1));
        }

        while let Some((candidate, Reverse(OrderedFloat(candidate_dist)))) = candidates.pop() {
            let (_, OrderedFloat(furtherest_dist)) = found_nn
                .peek()
                .expect("found_nn is non-empty: populated before search loop");
            let furtherest_dist = *furtherest_dist;
            if candidate_dist > furtherest_dist {
                break;
            }
            // loop over each of the candidate's neighbors
            for (neighbour_key, _) in
                self.hnsw_get_neighbours(&candidate, cur_level, idx_table, false)?
            {
                if visited.contains(&neighbour_key) {
                    continue;
                }
                vec_cache.ensure_key(&neighbour_key, orig_table, self)?;
                let neighbour_dist = vec_cache.v_dist(q, &neighbour_key)?;
                let (_, OrderedFloat(cand_furtherest_dist)) = found_nn
                    .peek()
                    .expect("found_nn is non-empty: populated before search loop");
                if found_nn.len() < ef || neighbour_dist < *cand_furtherest_dist {
                    candidates.push(neighbour_key.clone(), Reverse(OrderedFloat(neighbour_dist)));
                    found_nn.push(neighbour_key.clone(), OrderedFloat(neighbour_dist));
                    if found_nn.len() > ef {
                        found_nn.pop();
                    }
                }
                visited.insert(neighbour_key);
            }
        }

        Ok(())
    }
    fn hnsw_get_neighbours<'b>(
        &'b self,
        cand_key: &'b CompoundKey,
        level: i64,
        idx_handle: &RelationHandle,
        include_deleted: bool,
    ) -> Result<impl Iterator<Item = (CompoundKey, f64)> + 'b> {
        let mut start_tuple = Vec::with_capacity(cand_key.0.len() + 3);
        start_tuple.push(DataValue::from(level));
        start_tuple.extend_from_slice(&cand_key.0);
        start_tuple.push(DataValue::from(cand_key.1 as i64));
        start_tuple.push(DataValue::from(cand_key.2 as i64));
        let key_len = cand_key.0.len();
        Ok(idx_handle
            .scan_prefix(self, &start_tuple)
            .filter_map(move |res| {
                let tuple = res.ok()?;

                let key_idx = tuple[2 * key_len + 3]
                    .get_int()
                    .expect("HNSW index stores integers at this position")
                    as usize;
                let key_subidx = tuple[2 * key_len + 4]
                    .get_int()
                    .expect("HNSW index stores integers at this position")
                    as i32;
                let key_tup = tuple[key_len + 3..2 * key_len + 3].to_vec();
                if key_tup == cand_key.0 {
                    None
                } else {
                    if include_deleted {
                        return Some((
                            (key_tup, key_idx, key_subidx),
                            tuple[2 * key_len + 5]
                                .get_float()
                                .expect("HNSW index stores floats at this position"),
                        ));
                    }
                    let is_deleted = tuple[2 * key_len + 7]
                        .get_bool()
                        .expect("HNSW index stores bools at this position");
                    if is_deleted {
                        None
                    } else {
                        Some((
                            (key_tup, key_idx, key_subidx),
                            tuple[2 * key_len + 5]
                                .get_float()
                                .expect("HNSW index stores floats at this position"),
                        ))
                    }
                }
            }))
    }
    fn hnsw_put_fresh_at_levels(
        &mut self,
        hash: &[u8],
        tuple: &[DataValue],
        idx: usize,
        subidx: i32,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        bottom_level: i64,
        top_level: i64,
    ) -> Result<()> {
        let mut target_key = vec![DataValue::Null];
        let mut canary_key = vec![DataValue::from(1)];
        for _ in 0..2 {
            for i in 0..orig_table.metadata.keys.len() {
                target_key.push(tuple.get(i).expect("i bounded by keys.len()").clone());
                canary_key.push(DataValue::Null);
            }
            target_key.push(DataValue::from(idx as i64));
            target_key.push(DataValue::from(subidx as i64));
            canary_key.push(DataValue::Null);
            canary_key.push(DataValue::Null);
        }
        let target_value = [
            DataValue::from(0.0),
            DataValue::Bytes(hash.to_vec()),
            DataValue::from(false),
        ];
        let target_key_bytes = idx_table.encode_key_for_store(&target_key, Default::default())?;

        // canary value is for conflict detection: prevent the scenario of disconnected graphs at all levels
        let canary_value = [
            DataValue::from(bottom_level),
            DataValue::Bytes(target_key_bytes),
            DataValue::from(false),
        ];
        let canary_key_bytes = idx_table.encode_key_for_store(&canary_key, Default::default())?;
        let canary_value_bytes =
            idx_table.encode_val_only_for_store(&canary_value, Default::default())?;
        self.store_tx.put(&canary_key_bytes, &canary_value_bytes)?;

        for cur_level in bottom_level..=top_level {
            target_key[0] = DataValue::from(cur_level);
            let key = idx_table.encode_key_for_store(&target_key, Default::default())?;
            let val = idx_table.encode_val_only_for_store(&target_value, Default::default())?;
            self.store_tx.put(&key, &val)?;
        }
        Ok(())
    }
    pub(crate) fn hnsw_put(
        &mut self,
        manifest: &HnswIndexManifest,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        filter: Option<&Vec<Bytecode>>,
        stack: &mut Vec<DataValue>,
        tuple: &[DataValue],
    ) -> Result<bool> {
        if let Some(code) = filter {
            if !eval_bytecode_pred(code, tuple, stack, Default::default())? {
                self.hnsw_remove(orig_table, idx_table, tuple)?;
                return Ok(false);
            }
        }
        let mut extracted_vectors = vec![];
        for idx in &manifest.vec_fields {
            let val = tuple
                .get(*idx)
                .expect("vec_fields indices validated at index creation");
            if let DataValue::Vec(v) = val {
                extracted_vectors.push((v, *idx, -1));
            } else if let DataValue::List(l) = val {
                for (sidx, v) in l.iter().enumerate() {
                    if let DataValue::Vec(v) = v {
                        extracted_vectors.push((v, *idx, sidx as i32));
                    }
                }
            }
        }
        if extracted_vectors.is_empty() {
            return Ok(false);
        }
        let mut vec_cache = VectorCache::new(manifest.distance, DEFAULT_VECTOR_CACHE_CAPACITY);
        for (vec, idx, sub) in extracted_vectors {
            self.hnsw_put_vector(
                tuple,
                vec,
                idx,
                sub,
                manifest,
                orig_table,
                idx_table,
                &mut vec_cache,
            )?;
        }
        Ok(true)
    }
    pub(crate) fn hnsw_remove(
        &mut self,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
        tuple: &[DataValue],
    ) -> Result<()> {
        let mut prefix = vec![DataValue::from(0)];
        prefix.extend_from_slice(&tuple[0..orig_table.metadata.keys.len()]);
        let candidates: FxHashSet<_> = idx_table
            .scan_prefix(self, &prefix)
            .filter_map(|t| match t {
                Ok(t) => Some({
                    (
                        t[1..orig_table.metadata.keys.len() + 1].to_vec(),
                        t[orig_table.metadata.keys.len() + 1]
                            .get_int()
                            .expect("HNSW index stores integers at this position")
                            as usize,
                        t[orig_table.metadata.keys.len() + 2]
                            .get_int()
                            .expect("HNSW index stores integers at this position")
                            as i32,
                    )
                }),
                Err(_) => None,
            })
            .collect();
        for (tuple_key, idx, subidx) in candidates {
            self.hnsw_remove_vec(&tuple_key, idx, subidx, orig_table, idx_table)?;
        }
        Ok(())
    }
    fn hnsw_remove_vec(
        &mut self,
        tuple_key: &[DataValue],
        idx: usize,
        subidx: i32,
        orig_table: &RelationHandle,
        idx_table: &RelationHandle,
    ) -> Result<()> {
        let compound_key = (tuple_key.to_vec(), idx, subidx);
        // Go down the layers and remove all the links
        let mut encountered_singletons = false;
        for neg_layer in 0i64.. {
            let layer = -neg_layer;
            let mut self_key = vec![DataValue::from(layer)];
            for _ in 0..2 {
                self_key.extend_from_slice(tuple_key);
                self_key.push(DataValue::from(idx as i64));
                self_key.push(DataValue::from(subidx as i64));
            }
            let self_key_bytes = idx_table.encode_key_for_store(&self_key, Default::default())?;
            if self.store_tx.exists(&self_key_bytes, false)? {
                self.store_tx.del(&self_key_bytes)?;
            } else {
                break;
            }

            let neigbours = self
                .hnsw_get_neighbours(&compound_key, layer, idx_table, true)?
                .collect_vec();
            encountered_singletons |= neigbours.is_empty();
            for (neighbour_key, _) in neigbours {
                // REMARK: this still has some probability of disconnecting the graph.
                // Should we accept that as a consequence of the probabilistic nature of the algorithm?
                let mut out_key = vec![DataValue::from(layer)];
                out_key.extend_from_slice(tuple_key);
                out_key.push(DataValue::from(idx as i64));
                out_key.push(DataValue::from(subidx as i64));
                out_key.extend_from_slice(&neighbour_key.0);
                out_key.push(DataValue::from(neighbour_key.1 as i64));
                out_key.push(DataValue::from(neighbour_key.2 as i64));
                let out_key_bytes = idx_table.encode_key_for_store(&out_key, Default::default())?;
                self.store_tx.del(&out_key_bytes)?;
                let mut in_key = vec![DataValue::from(layer)];
                in_key.extend_from_slice(&neighbour_key.0);
                in_key.push(DataValue::from(neighbour_key.1 as i64));
                in_key.push(DataValue::from(neighbour_key.2 as i64));
                in_key.extend_from_slice(tuple_key);
                in_key.push(DataValue::from(idx as i64));
                in_key.push(DataValue::from(subidx as i64));
                let in_key_bytes = idx_table.encode_key_for_store(&in_key, Default::default())?;
                self.store_tx.del(&in_key_bytes)?;
                let mut neighbour_self_key = vec![DataValue::from(layer)];
                for _ in 0..2 {
                    neighbour_self_key.extend_from_slice(&neighbour_key.0);
                    neighbour_self_key.push(DataValue::from(neighbour_key.1 as i64));
                    neighbour_self_key.push(DataValue::from(neighbour_key.2 as i64));
                }
                let neighbour_val_bytes = match self
                    .store_tx
                    .get(
                        &idx_table.encode_key_for_store(&neighbour_self_key, Default::default())?,
                        false,
                    )? {
                    Some(bytes) => bytes,
                    None => return Err(InvalidOperationSnafu {
                        op: "hnsw_remove",
                        reason: "HNSW neighbour self-key not found during removal, index may be corrupted".to_string(),
                    }.build().into()),
                };
                let mut neighbour_val: Vec<DataValue> = rmp_serde::from_slice(
                    &neighbour_val_bytes[ENCODED_KEY_MIN_LEN..],
                )
                .map_err(|e| crate::engine::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "hnsw_index",
                        reason: e.to_string(),
                    }
                    .build(),
                })?;
                neighbour_val[0] = DataValue::from(
                    neighbour_val[0]
                        .get_float()
                        .expect("HNSW index stores floats at this position")
                        - 1.,
                );
                self.store_tx.put(
                    &idx_table.encode_key_for_store(&neighbour_self_key, Default::default())?,
                    &idx_table.encode_val_only_for_store(&neighbour_val, Default::default())?,
                )?;
            }
        }

        if encountered_singletons {
            // the entry point is removed, we need to do something
            let ep_res = idx_table
                .scan_bounded_prefix(
                    self,
                    &[],
                    &[DataValue::from(i64::MIN)],
                    &[DataValue::from(1)],
                )
                .next();
            let mut canary_key = vec![DataValue::from(1)];
            for _ in 0..2 {
                for _ in 0..orig_table.metadata.keys.len() {
                    canary_key.push(DataValue::Null);
                }
                canary_key.push(DataValue::Null);
                canary_key.push(DataValue::Null);
            }
            let canary_key_bytes =
                idx_table.encode_key_for_store(&canary_key, Default::default())?;
            if let Some(ep) = ep_res {
                let ep = ep?;
                let target_key_bytes = idx_table.encode_key_for_store(&ep, Default::default())?;
                let bottom_level = ep[0]
                    .get_int()
                    .expect("HNSW index stores integers at this position");
                // canary value is for conflict detection: prevent the scenario of disconnected graphs at all levels
                let canary_value = [
                    DataValue::from(bottom_level),
                    DataValue::Bytes(target_key_bytes),
                    DataValue::from(false),
                ];
                let canary_value_bytes =
                    idx_table.encode_val_only_for_store(&canary_value, Default::default())?;
                self.store_tx.put(&canary_key_bytes, &canary_value_bytes)?;
            } else {
                // HA! we have removed the last item in the index
                self.store_tx.del(&canary_key_bytes)?;
            }
        }

        Ok(())
    }
    pub(crate) fn hnsw_knn(
        &self,
        q: Vector,
        config: &HnswSearch,
        filter_bytecode: &Option<(Vec<Bytecode>, SourceSpan)>,
        stack: &mut Vec<DataValue>,
    ) -> Result<Vec<Tuple>> {
        if q.len() != config.manifest.vec_dim {
            return Err(InvalidOperationSnafu {
                op: "hnsw_query",
                reason: "query vector dimension mismatch".to_string(),
            }
            .build()
            .into());
        }
        let q = match (q, config.manifest.dtype) {
            (v @ Vector::F32(_), VecElementType::F32) => v,
            (v @ Vector::F64(_), VecElementType::F64) => v,
            (Vector::F32(v), VecElementType::F64) => Vector::F64(v.mapv(|x| x as f64)),
            (Vector::F64(v), VecElementType::F32) => Vector::F32(v.mapv(|x| x as f32)),
        };

        let mut vec_cache =
            VectorCache::new(config.manifest.distance, DEFAULT_VECTOR_CACHE_CAPACITY);

        let ep_res = config
            .idx_handle
            .scan_bounded_prefix(
                self,
                &[],
                &[DataValue::from(i64::MIN)],
                &[DataValue::from(1)],
            )
            .next();
        if let Some(ep) = ep_res {
            let ep = ep?;
            let bottom_level = ep[0]
                .get_int()
                .expect("HNSW index stores integers at this position");
            let ep_idx = match ep[config.base_handle.metadata.keys.len() + 1].get_int() {
                Some(x) => x as usize,
                None => {
                    // this occurs if the index is empty
                    return Ok(vec![]);
                }
            };
            let ep_t_key = ep[1..config.base_handle.metadata.keys.len() + 1].to_vec();
            let ep_subidx = ep[config.base_handle.metadata.keys.len() + 2]
                .get_int()
                .expect("HNSW index stores integers at this position")
                as i32;
            let ep_key = (ep_t_key, ep_idx, ep_subidx);
            vec_cache.ensure_key(&ep_key, &config.base_handle, self)?;
            let ep_distance = vec_cache.v_dist(&q, &ep_key)?;
            let mut found_nn = PriorityQueue::new();
            found_nn.push(ep_key, OrderedFloat(ep_distance));
            for current_level in bottom_level..0 {
                self.hnsw_search_level(
                    &q,
                    1,
                    current_level,
                    &config.base_handle,
                    &config.idx_handle,
                    &mut found_nn,
                    &mut vec_cache,
                )?;
            }
            self.hnsw_search_level(
                &q,
                config.ef,
                0,
                &config.base_handle,
                &config.idx_handle,
                &mut found_nn,
                &mut vec_cache,
            )?;
            if found_nn.is_empty() {
                return Ok(vec![]);
            }

            if config.filter.is_none() {
                while found_nn.len() > config.k {
                    found_nn.pop();
                }
            }

            let mut ret = vec![];

            while let Some((cand_key, OrderedFloat(distance))) = found_nn.pop() {
                if let Some(r) = config.radius {
                    if distance > r {
                        continue;
                    }
                }

                let mut cand_tuple =
                    config.base_handle.get(self, &cand_key.0)?.ok_or_else(|| {
                        crate::engine::error::InternalError::Runtime {
                            source: InvalidOperationSnafu {
                                op: "hnsw_query",
                                reason: "corrupted index",
                            }
                            .build(),
                        }
                    })?;

                // make sure the order is the same as in all_bindings()!!!
                if config.bind_field.is_some() {
                    let field = if cand_key.1 < config.base_handle.metadata.keys.len() {
                        config.base_handle.metadata.keys[cand_key.1].name.clone()
                    } else {
                        config.base_handle.metadata.non_keys
                            [cand_key.1 - config.base_handle.metadata.keys.len()]
                        .name
                        .clone()
                    };
                    cand_tuple.push(DataValue::Str(field));
                }
                if config.bind_field_idx.is_some() {
                    cand_tuple.push(if cand_key.2 < 0 {
                        DataValue::Null
                    } else {
                        DataValue::from(cand_key.2 as i64)
                    });
                }
                if config.bind_distance.is_some() {
                    cand_tuple.push(DataValue::from(distance));
                }
                if config.bind_vector.is_some() {
                    let vec = if cand_key.2 < 0 {
                        cand_tuple[cand_key.1].clone()
                    } else {
                        match &cand_tuple[cand_key.1] {
                            DataValue::List(v) => v[cand_key.2 as usize].clone(),
                            v => {
                                return Err(InvalidOperationSnafu {
                                    op: "hnsw_index",
                                    reason: format!("corrupted index value {:?}", v),
                                }
                                .build()
                                .into());
                            }
                        }
                    };
                    cand_tuple.push(vec);
                }

                if let Some((code, span)) = filter_bytecode {
                    if !eval_bytecode_pred(code, &cand_tuple, stack, *span)? {
                        continue;
                    }
                }

                ret.push(cand_tuple);
            }
            ret.reverse();
            ret.truncate(config.k);

            Ok(ret)
        } else {
            Ok(vec![])
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use rand::Rng;
    use std::collections::BTreeMap;

    use super::*;
    use crate::engine::DbInstance;

    #[test]
    fn test_random_level() {
        let m = 20;
        let mult = 1. / (m as f64).ln();
        let mut rng = rand::rng();
        let mut collected = BTreeMap::new();
        for _ in 0..10000 {
            let uniform_num: f64 = rng.random_range(0.0..1.0);
            let r = -uniform_num.ln() * mult;
            let level = -(r.floor() as i64);
            collected.entry(level).and_modify(|x| *x += 1).or_insert(1);
        }
        assert!(!collected.is_empty());
    }

    #[test]
    fn hnsw_cache_eviction() {
        let mut cache = VectorCache::new(HnswDistance::L2, 10);
        for i in 0..20u8 {
            let key = (vec![DataValue::from(i as i64)], 0, -1);
            let vec = Vector::F64(ndarray::Array1::zeros(4));
            cache.insert(key, vec);
        }
        assert_eq!(cache.len(), 10, "cache should be bounded at capacity");
    }

    #[test]
    fn hnsw_cache_retains_recent() {
        let mut cache = VectorCache::new(HnswDistance::L2, 5);
        for i in 0..10u8 {
            let key = (vec![DataValue::from(i as i64)], 0, -1);
            let vec = Vector::F64(ndarray::Array1::zeros(4));
            cache.insert(key, vec);
        }
        // Most recent insertions (5..10) should be in cache
        for i in 5..10u8 {
            let key = (vec![DataValue::from(i as i64)], 0, -1);
            assert!(
                cache.cache.contains(&key),
                "recent key {i} should be in cache"
            );
        }
        // Oldest insertions (0..5) should have been evicted
        for i in 0..5u8 {
            let key = (vec![DataValue::from(i as i64)], 0, -1);
            assert!(!cache.cache.contains(&key), "old key {i} should be evicted");
        }
    }

    #[test]
    fn hnsw_dist_mismatched_types_returns_error() {
        let cache = VectorCache::new(HnswDistance::L2, 10);
        let v1 = Vector::F32(ndarray::Array1::from_vec(vec![1.0f32, 2.0]));
        let v2 = Vector::F64(ndarray::Array1::from_vec(vec![1.0f64, 2.0]));
        assert!(cache.dist(&v1, &v2).is_err());
    }

    #[test]
    fn hnsw_dist_l2_correctness() {
        let cache = VectorCache::new(HnswDistance::L2, 10);
        let v1 = Vector::F64(ndarray::Array1::from_vec(vec![0.0, 0.0]));
        let v2 = Vector::F64(ndarray::Array1::from_vec(vec![3.0, 4.0]));
        let d = cache.dist(&v1, &v2).unwrap();
        assert!(
            (d - 25.0).abs() < 1e-10,
            "L2 squared distance should be 25.0, got {d}"
        );
    }

    #[test]
    fn hnsw_dist_cosine_identical_vectors() {
        let cache = VectorCache::new(HnswDistance::Cosine, 10);
        let v = Vector::F64(ndarray::Array1::from_vec(vec![1.0, 0.0, 0.0]));
        let d = cache.dist(&v, &v).unwrap();
        assert!(
            d.abs() < 1e-10,
            "cosine distance of identical vectors should be ~0, got {d}"
        );
    }

    #[test]
    fn hnsw_insert_and_search() {
        let db = DbInstance::default();
        db.run_default(":create vectors { id: Int => vec: <F32; 4> }")
            .unwrap();
        db.run_default(
            r#"::hnsw create vectors:idx {
                dim: 4,
                m: 16,
                dtype: F32,
                fields: [vec],
                distance: L2,
                ef_construction: 50,
                extend_candidates: false,
                keep_pruned_connections: false,
            }"#,
        )
        .unwrap();
        // Insert 20 vectors
        for i in 0..20 {
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        // Search for nearest to [5,5,5,5]
        let res = db.run_default(
            r#"?[id, dist] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 3, ef: 50, bind_distance: dist}"#,
        ).unwrap();
        assert!(!res.rows.is_empty(), "search should return results");
        assert!(res.rows.len() <= 3, "should return at most k=3 results");
        // The closest vector should be id=5 (exact match)
        // HNSW is approximate — the exact match (id=5) should be among top results
        let ids: Vec<i64> = res.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(
            ids.contains(&5),
            "exact match id=5 should be among top-k results, got {:?}",
            ids
        );
    }

    #[test]
    fn hnsw_empty_search() {
        let db = DbInstance::default();
        db.run_default(":create vectors { id: Int => vec: <F32; 4> }")
            .unwrap();
        db.run_default(
            r#"::hnsw create vectors:idx {
                dim: 4,
                m: 16,
                dtype: F32,
                fields: [vec],
                distance: L2,
                ef_construction: 50,
                extend_candidates: false,
                keep_pruned_connections: false,
            }"#,
        )
        .unwrap();
        // Search in empty index should return empty, not panic
        let res = db.run_default(
            r#"?[id, dist] := ~vectors:idx{id | query: vec([1.0, 2.0, 3.0, 4.0]), k: 5, ef: 50, bind_distance: dist}"#,
        ).unwrap();
        assert!(
            res.rows.is_empty(),
            "empty index search should return no results"
        );
    }

    #[test]
    fn hnsw_delete() {
        let db = DbInstance::default();
        db.run_default(":create vectors { id: Int => vec: <F32; 4> }")
            .unwrap();
        db.run_default(
            r#"::hnsw create vectors:idx {
                dim: 4,
                m: 16,
                dtype: F32,
                fields: [vec],
                distance: L2,
                ef_construction: 50,
                extend_candidates: false,
                keep_pruned_connections: false,
            }"#,
        )
        .unwrap();
        // Insert vectors
        for i in 0..10 {
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, {val}, {val}, {val}])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        // Delete vector id=5
        db.run_default("?[id] <- [[5]] :rm vectors {}").unwrap();
        // Search for nearest to [5,5,5,5] — should NOT return id=5
        let res = db.run_default(
            r#"?[id, dist] := ~vectors:idx{id | query: vec([5.0, 5.0, 5.0, 5.0]), k: 3, ef: 50, bind_distance: dist}"#,
        ).unwrap();
        let ids: Vec<i64> = res.rows.iter().filter_map(|r| r[0].get_int()).collect();
        assert!(
            !ids.contains(&5),
            "deleted vector id=5 should not appear in results"
        );
    }

    #[test]
    fn hnsw_search_results_ordered_by_distance() {
        let db = DbInstance::default();
        db.run_default(":create vectors { id: Int => vec: <F32; 4> }")
            .unwrap();
        db.run_default(
            r#"::hnsw create vectors:idx {
                dim: 4,
                m: 16,
                dtype: F32,
                fields: [vec],
                distance: L2,
                ef_construction: 50,
                extend_candidates: false,
                keep_pruned_connections: false,
            }"#,
        )
        .unwrap();
        for i in 0..50 {
            let val = i as f32;
            db.run_default(&format!(
                "?[id, vec] <- [[{i}, vec([{val}, 0.0, 0.0, 0.0])]] :put vectors {{}}"
            ))
            .unwrap();
        }
        let res = db.run_default(
            r#"?[id, dist] := ~vectors:idx{id | query: vec([25.0, 0.0, 0.0, 0.0]), k: 10, ef: 50, bind_distance: dist} :order dist"#,
        ).unwrap();
        let distances: Vec<f64> = res.rows.iter().filter_map(|r| r[1].get_float()).collect();
        assert!(!distances.is_empty(), "should return results");
        for window in distances.windows(2) {
            assert!(
                window[0] <= window[1],
                "results should be ordered by distance: {} <= {}",
                window[0],
                window[1]
            );
        }
    }
}
