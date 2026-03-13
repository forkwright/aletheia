//! Full-text search indexing operations.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use crate::engine::data::expr::{Bytecode, eval_bytecode, eval_bytecode_pred};
use crate::engine::data::program::{FtsScoreKind, FtsSearch};
use crate::engine::data::tuple::{ENCODED_KEY_MIN_LEN, Tuple, decode_tuple_from_key};
use crate::engine::data::value::LARGEST_UTF_CHAR;
use crate::engine::error::InternalResult as Result;
use crate::engine::fts::ast::{FtsExpr, FtsLiteral, FtsNear};
use crate::engine::fts::error::TokenizationFailedSnafu;
use crate::engine::fts::tokenizer::TextAnalyzer;
use crate::engine::parse::fts::parse_fts_query;
use crate::engine::runtime::relation::RelationHandle;
use crate::engine::runtime::transact::SessionTx;
use crate::engine::{DataValue, SourceSpan};
use compact_str::CompactString;
use itertools::Itertools;
use ordered_float::OrderedFloat;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

#[derive(Default)]
pub(crate) struct FtsCache {
    total_n_cache: FxHashMap<CompactString, usize>,
    avg_dl_cache: FxHashMap<CompactString, f64>,
}

impl FtsCache {
    fn get_n_for_relation(&mut self, rel: &RelationHandle, tx: &SessionTx<'_>) -> Result<usize> {
        Ok(match self.total_n_cache.entry(rel.name.clone()) {
            Entry::Vacant(v) => {
                let start = rel.encode_partial_key_for_store(&[]);
                let end = rel.encode_partial_key_for_store(&[DataValue::Bot]);
                let val = tx.store_tx.range_count(&start, &end)?;
                v.insert(val);
                val
            }
            Entry::Occupied(o) => *o.get(),
        })
    }

    fn get_avg_dl_for_relation(&mut self, idx: &RelationHandle, tx: &SessionTx<'_>) -> Result<f64> {
        Ok(match self.avg_dl_cache.entry(idx.name.clone()) {
            Entry::Occupied(o) => *o.get(),
            Entry::Vacant(v) => {
                let start = idx.encode_partial_key_for_store(&[]);
                let end = idx.encode_partial_key_for_store(&[DataValue::Bot]);
                let mut doc_lengths: FxHashMap<Vec<DataValue>, u32> = FxHashMap::default();
                for item in tx.store_tx.range_scan(&start, &end) {
                    let (kvec, vvec) = item?;
                    let key_tuple = decode_tuple_from_key(&kvec, idx.metadata.keys.len());
                    let doc_key = key_tuple[1..].to_vec();
                    let vals: Vec<DataValue> = rmp_serde::from_slice(&vvec[ENCODED_KEY_MIN_LEN..])
                        .map_err(|e| {
                            crate::engine::error::InternalError::from(
                                TokenizationFailedSnafu {
                                    message: e.to_string(),
                                }
                                .build(),
                            )
                        })?;
                    let total_length = vals[3].get_int().unwrap_or(0) as u32;
                    doc_lengths
                        .entry(doc_key)
                        .and_modify(|_| {})
                        .or_insert(total_length);
                }
                let avg = if doc_lengths.is_empty() {
                    0.0
                } else {
                    let sum: u32 = doc_lengths.values().sum();
                    sum as f64 / doc_lengths.len() as f64
                };
                v.insert(avg);
                avg
            }
        })
    }
}

struct PositionInfo {
    // from: u32,
    // to: u32,
    position: u32,
}

struct LiteralStats {
    key: Tuple,
    position_info: Vec<PositionInfo>,
    doc_len: u32,
}

pub(crate) fn bm25_compute_score(
    tf: usize,
    df: usize,
    n: usize,
    dl: u32,
    avgdl: f64,
    booster: f64,
    k1: f64,
    b: f64,
) -> f64 {
    if n == 0 || df == 0 {
        return 0.0;
    }
    if tf == 0 {
        return 0.0;
    }
    let tf = tf as f64;
    let df = df as f64;
    let n = n as f64;
    let dl = dl as f64;
    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
    let normalized_tf = (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avgdl.max(1.0)));
    idf * normalized_tf * booster
}

impl<'a> SessionTx<'a> {
    fn fts_search_literal(
        &self,
        literal: &FtsLiteral,
        idx_handle: &RelationHandle,
    ) -> Result<Vec<LiteralStats>> {
        let start_key_str = &literal.value as &str;
        let start_key = vec![DataValue::Str(CompactString::from(start_key_str))];
        let mut end_key_str = literal.value.clone();
        end_key_str.push(LARGEST_UTF_CHAR);
        let end_key = vec![DataValue::Str(end_key_str)];
        let start_key_bytes = idx_handle.encode_partial_key_for_store(&start_key);
        let end_key_bytes = idx_handle.encode_partial_key_for_store(&end_key);
        let mut results = vec![];
        for item in self.store_tx.range_scan(&start_key_bytes, &end_key_bytes) {
            let (kvec, vvec) = item?;
            let key_tuple = decode_tuple_from_key(&kvec, idx_handle.metadata.keys.len());
            let found_str_key = key_tuple[0]
                .get_str()
                .expect("FTS index key[0] is always a string");
            if literal.is_prefix {
                if !found_str_key.starts_with(start_key_str) {
                    break;
                }
            } else if found_str_key != start_key_str {
                break;
            }

            let vals: Vec<DataValue> = rmp_serde::from_slice(&vvec[ENCODED_KEY_MIN_LEN..])
                .map_err(|e| {
                    crate::engine::error::InternalError::from(
                        TokenizationFailedSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;
            let froms = vals[0]
                .get_slice()
                .expect("FTS index val[0] (froms) is always a list");
            let tos = vals[1]
                .get_slice()
                .expect("FTS index val[1] (tos) is always a list");
            let positions = vals[2]
                .get_slice()
                .expect("FTS index val[2] (positions) is always a list");
            let total_length = vals[3].get_int().unwrap_or(0) as u32;
            let position_info = froms
                .iter()
                .zip(tos.iter())
                .zip(positions.iter())
                .map(|(_, p)| PositionInfo {
                    // from: f.get_int().unwrap() as u32,
                    // to: t.get_int().unwrap() as u32,
                    position: p.get_int().expect("FTS position is always an integer") as u32,
                })
                .collect_vec();
            results.push(LiteralStats {
                key: key_tuple[1..].to_vec(),
                position_info,
                doc_len: total_length,
            });
        }
        Ok(results)
    }
    fn fts_search_impl(
        &self,
        ast: &FtsExpr,
        config: &FtsSearch,
        n: usize,
        avgdl: f64,
    ) -> Result<FxHashMap<Tuple, f64>> {
        Ok(match ast {
            FtsExpr::Literal(l) => {
                let mut res = FxHashMap::default();
                let found_docs = self.fts_search_literal(l, &config.idx_handle)?;
                let found_docs_len = found_docs.len();
                for el in found_docs {
                    let score = match config.score_kind {
                        FtsScoreKind::Bm25 => bm25_compute_score(
                            el.position_info.len(),
                            found_docs_len,
                            n,
                            el.doc_len,
                            avgdl,
                            l.booster.0,
                            1.2,
                            0.75,
                        ),
                        _ => Self::fts_compute_score(
                            el.position_info.len(),
                            found_docs_len,
                            n,
                            l.booster.0,
                            config,
                        ),
                    };
                    res.insert(el.key, score);
                }
                res
            }
            FtsExpr::And(ls) => {
                let mut l_iter = ls.iter();
                let mut res = self.fts_search_impl(
                    l_iter
                        .next()
                        .expect("And node always has at least one child"),
                    config,
                    n,
                    avgdl,
                )?;
                for nxt in l_iter {
                    let nxt_res = self.fts_search_impl(nxt, config, n, avgdl)?;
                    res = res
                        .into_iter()
                        .filter_map(|(k, v)| nxt_res.get(&k).map(|nxt_v| (k, v + nxt_v)))
                        .collect();
                }
                res
            }
            FtsExpr::Or(ls) => {
                let mut res: FxHashMap<Tuple, f64> = FxHashMap::default();
                for nxt in ls {
                    let nxt_res = self.fts_search_impl(nxt, config, n, avgdl)?;
                    for (k, v) in nxt_res {
                        if let Some(old_v) = res.get_mut(&k) {
                            *old_v = (*old_v).max(v);
                        } else {
                            res.insert(k, v);
                        }
                    }
                }
                res
            }
            FtsExpr::Near(FtsNear { literals, distance }) => {
                let mut l_it = literals.iter();
                let mut coll: FxHashMap<_, _> = FxHashMap::default();
                for first_el in self.fts_search_literal(
                    l_it.next()
                        .expect("Near node always has at least one literal"),
                    &config.idx_handle,
                )? {
                    coll.insert(
                        first_el.key,
                        first_el
                            .position_info
                            .into_iter()
                            .map(|el| el.position)
                            .collect_vec(),
                    );
                }
                for lit_nxt in literals {
                    let el_res = self.fts_search_literal(lit_nxt, &config.idx_handle)?;
                    coll = el_res
                        .into_iter()
                        .filter_map(|x| match coll.remove(&x.key) {
                            None => None,
                            Some(prev_pos) => {
                                let mut inner_coll = FxHashSet::default();
                                for p in prev_pos {
                                    for pi in x.position_info.iter() {
                                        let cur = pi.position;
                                        if cur > p {
                                            if cur - p <= *distance {
                                                inner_coll.insert(p);
                                            }
                                        } else if p - cur <= *distance {
                                            inner_coll.insert(cur);
                                        }
                                    }
                                }
                                if inner_coll.is_empty() {
                                    None
                                } else {
                                    Some((x.key, inner_coll.into_iter().collect_vec()))
                                }
                            }
                        })
                        .collect();
                }
                let mut booster = 0.0;
                for lit in literals {
                    booster += lit.booster.0;
                }
                let coll_len = coll.len();
                coll.into_iter()
                    .map(|(k, cands)| {
                        (
                            k,
                            Self::fts_compute_score(cands.len(), coll_len, n, booster, config),
                        )
                    })
                    .collect()
            }
            FtsExpr::Not(fst, snd) => {
                let mut res = self.fts_search_impl(fst, config, n, avgdl)?;
                for el in self.fts_search_impl(snd, config, n, avgdl)?.keys() {
                    res.remove(el);
                }
                res
            }
        })
    }
    fn fts_compute_score(
        tf: usize,
        n_found_docs: usize,
        n_total: usize,
        booster: f64,
        config: &FtsSearch,
    ) -> f64 {
        let tf = tf as f64;
        match config.score_kind {
            FtsScoreKind::Tf => tf * booster,
            FtsScoreKind::TfIdf | FtsScoreKind::Bm25 => {
                let n_found_docs = n_found_docs as f64;
                let idf = (1.0 + (n_total as f64 - n_found_docs + 0.5) / (n_found_docs + 0.5)).ln();
                tf * idf * booster
            }
        }
    }
    pub(crate) fn fts_search(
        &self,
        q: &str,
        config: &FtsSearch,
        filter_code: &Option<(Vec<Bytecode>, SourceSpan)>,
        tokenizer: &TextAnalyzer,
        stack: &mut Vec<DataValue>,
        cache: &mut FtsCache,
    ) -> Result<Vec<Tuple>> {
        let ast = parse_fts_query(q)?.tokenize(tokenizer);
        if ast.is_empty() {
            return Ok(vec![]);
        }
        let n = if config.score_kind == FtsScoreKind::TfIdf
            || config.score_kind == FtsScoreKind::Bm25
        {
            cache.get_n_for_relation(&config.base_handle, self)?
        } else {
            0
        };
        let avgdl = if config.score_kind == FtsScoreKind::Bm25 {
            cache.get_avg_dl_for_relation(&config.idx_handle, self)?
        } else {
            0.0
        };
        let mut result: Vec<_> = self
            .fts_search_impl(&ast, config, n, avgdl)?
            .into_iter()
            .collect();
        result.sort_by_key(|(_, score)| Reverse(OrderedFloat(*score)));
        if config.filter.is_none() {
            result.truncate(config.k);
        }

        let mut ret = Vec::with_capacity(config.k);
        for (found_key, score) in result {
            let mut cand_tuple = config.base_handle.get(self, &found_key)?.ok_or_else(|| {
                crate::engine::error::InternalError::from(
                    TokenizationFailedSnafu {
                        message: "corrupted index".to_string(),
                    }
                    .build(),
                )
            })?;

            if config.bind_score.is_some() {
                cand_tuple.push(DataValue::from(score));
            }

            if let Some((code, span)) = filter_code
                && !eval_bytecode_pred(code, &cand_tuple, stack, *span)? {
                    continue;
                }

            ret.push(cand_tuple);
            if ret.len() >= config.k {
                break;
            }
        }
        Ok(ret)
    }
    pub(crate) fn put_fts_index_item(
        &mut self,
        tuple: &[DataValue],
        extractor: &[Bytecode],
        stack: &mut Vec<DataValue>,
        tokenizer: &TextAnalyzer,
        rel_handle: &RelationHandle,
        idx_handle: &RelationHandle,
    ) -> Result<()> {
        let to_index = match eval_bytecode(extractor, tuple, stack)? {
            DataValue::Null => return Ok(()),
            DataValue::Str(s) => s,
            _val => {
                return Err(TokenizationFailedSnafu {
                    message: "FTS index extractor must return a string".to_string(),
                }
                .build()
                .into());
            }
        };
        let mut token_stream = tokenizer.token_stream(&to_index);
        let mut collector: HashMap<_, (Vec<_>, Vec<_>, Vec<_>), _> = FxHashMap::default();
        let mut count = 0i64;
        while let Some(token) = token_stream.next() {
            let text = CompactString::from(&token.text);
            let (fr, to, position) = collector.entry(text).or_default();
            fr.push(DataValue::from(token.offset_from as i64));
            to.push(DataValue::from(token.offset_to as i64));
            position.push(DataValue::from(token.position as i64));
            count += 1;
        }
        let mut key = Vec::with_capacity(1 + rel_handle.metadata.keys.len());
        key.push(DataValue::Bot);
        for k in &tuple[..rel_handle.metadata.keys.len()] {
            key.push(k.clone());
        }
        let mut val = vec![
            DataValue::Bot,
            DataValue::Bot,
            DataValue::Bot,
            DataValue::from(count),
        ];
        for (text, (from, to, position)) in collector {
            key[0] = DataValue::Str(text);
            val[0] = DataValue::List(from);
            val[1] = DataValue::List(to);
            val[2] = DataValue::List(position);
            let key_bytes = idx_handle.encode_key_for_store(&key, Default::default())?;
            let val_bytes = idx_handle.encode_val_only_for_store(&val, Default::default())?;
            self.store_tx.put(&key_bytes, &val_bytes)?;
        }
        Ok(())
    }
    pub(crate) fn del_fts_index_item(
        &mut self,
        tuple: &[DataValue],
        extractor: &[Bytecode],
        stack: &mut Vec<DataValue>,
        tokenizer: &TextAnalyzer,
        rel_handle: &RelationHandle,
        idx_handle: &RelationHandle,
    ) -> Result<()> {
        let to_index = match eval_bytecode(extractor, tuple, stack)? {
            DataValue::Null => return Ok(()),
            DataValue::Str(s) => s,
            _val => {
                return Err(TokenizationFailedSnafu {
                    message: "FTS index extractor must return a string".to_string(),
                }
                .build()
                .into());
            }
        };
        let mut token_stream = tokenizer.token_stream(&to_index);
        let mut collector = FxHashSet::default();
        while let Some(token) = token_stream.next() {
            let text = CompactString::from(&token.text);
            collector.insert(text);
        }
        let mut key = Vec::with_capacity(1 + rel_handle.metadata.keys.len());
        key.push(DataValue::Bot);
        for k in &tuple[..rel_handle.metadata.keys.len()] {
            key.push(k.clone());
        }
        for text in collector {
            key[0] = DataValue::Str(text);
            let key_bytes = idx_handle.encode_key_for_store(&key, Default::default())?;
            self.store_tx.del(&key_bytes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::bm25_compute_score;
    use crate::engine::data::program::FtsScoreKind;

    #[test]
    fn bm25_score_kind_variant_exists() {
        let kind = FtsScoreKind::Bm25;
        assert_ne!(kind, FtsScoreKind::Tf);
    }

    #[test]
    fn bm25_nonzero_for_typical_input() {
        let score = bm25_compute_score(3, 5, 100, 50, 80.0, 1.0, 1.2, 0.75);
        assert!(score > 0.0, "BM25 score must be nonzero for typical input");
    }

    #[test]
    fn bm25_differs_from_tf_idf() {
        let tf = 3usize;
        let df = 5usize;
        let n = 100usize;
        let booster = 1.0f64;

        let bm25_score = bm25_compute_score(tf, df, n, 50, 80.0, booster, 1.2, 0.75);

        let tf_f = tf as f64;
        let df_f = df as f64;
        let n_f = n as f64;
        let idf = (1.0 + (n_f - df_f + 0.5) / (df_f + 0.5)).ln();
        let tfidf_score = tf_f * idf * booster;

        assert!(
            (bm25_score - tfidf_score).abs() > 1e-9,
            "BM25 and TF-IDF must differ: bm25={bm25_score}, tfidf={tfidf_score}"
        );
    }

    #[test]
    fn bm25_longer_doc_scores_lower() {
        let score_short = bm25_compute_score(3, 5, 100, 20, 80.0, 1.0, 1.2, 0.75);
        let score_long = bm25_compute_score(3, 5, 100, 200, 80.0, 1.0, 1.2, 0.75);
        assert!(
            score_short > score_long,
            "Shorter doc must score higher: short={score_short}, long={score_long}"
        );
    }

    #[test]
    fn bm25_zero_tf_returns_zero() {
        let score = bm25_compute_score(0, 5, 100, 50, 80.0, 1.0, 1.2, 0.75);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn bm25_zero_df_returns_zero() {
        let score = bm25_compute_score(3, 0, 100, 50, 80.0, 1.0, 1.2, 0.75);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn bm25_zero_n_returns_zero() {
        let score = bm25_compute_score(3, 5, 0, 50, 80.0, 1.0, 1.2, 0.75);
        assert_eq!(score, 0.0);
    }
}
