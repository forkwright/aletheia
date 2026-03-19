//! Relation type definitions: RelationId, RelationHandle, InputRelationHandle.
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};

use crate::engine::error::InternalResult as Result;
use crate::engine::runtime::error::{InvalidOperationSnafu, SerializationSnafu};
use compact_str::CompactString;
use itertools::Itertools;
use rmp_serde::Serializer;
use serde::Serialize;
use snafu::Snafu;
use tracing::error;

use crate::engine::data::memcmp::MemCmpEncoder;
use crate::engine::data::relation::StoredRelationMetadata;
use crate::engine::data::symb::Symbol;
use crate::engine::data::tuple::{ENCODED_KEY_MIN_LEN, Tuple, TupleT, decode_tuple_from_key};
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::fts::FtsIndexManifest;
use crate::engine::parse::SourceSpan;
use crate::engine::query::compile::IndexPositionUse;
use crate::engine::runtime::hnsw::HnswIndexManifest;
use crate::engine::runtime::minhash_lsh::MinHashLshIndexManifest;
use crate::engine::runtime::transact::SessionTx;
use crate::engine::{NamedRows, StoreTx};

#[derive(
    Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, PartialOrd, Ord,
)]
pub(crate) struct RelationId(pub(crate) u64);

impl RelationId {
    pub(crate) fn new(u: u64) -> Result<Self> {
        if u > 2u64.pow(6 * 8) {
            InvalidOperationSnafu {
                op: "RelationId::new",
                reason: format!("value {u} exceeds 6-byte limit"),
            }
            .fail()
            .map_err(|e| e.into())
        } else {
            Ok(Self(u))
        }
    }
    pub(crate) fn next(&self) -> Result<Self> {
        Self::new(self.0 + 1)
    }
    pub(crate) const SYSTEM: Self = Self(0);
    pub(crate) fn raw_encode(&self) -> [u8; 8] {
        self.0.to_be_bytes()
    }
    pub(crate) fn raw_decode(src: &[u8]) -> Result<Self> {
        let u = u64::from_be_bytes([
            src[0], src[1], src[2], src[3], src[4], src[5], src[6], src[7],
        ]);
        Self::new(u)
    }
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RelationHandle {
    pub(crate) name: CompactString,
    pub(crate) id: RelationId,
    pub(crate) metadata: StoredRelationMetadata,
    pub(crate) put_triggers: Vec<String>,
    pub(crate) rm_triggers: Vec<String>,
    pub(crate) replace_triggers: Vec<String>,
    pub(crate) access_level: AccessLevel,
    pub(crate) is_temp: bool,
    pub(crate) indices: BTreeMap<CompactString, (RelationHandle, Vec<usize>)>,
    pub(crate) hnsw_indices: BTreeMap<CompactString, (RelationHandle, HnswIndexManifest)>,
    pub(crate) fts_indices: BTreeMap<CompactString, (RelationHandle, FtsIndexManifest)>,
    pub(crate) lsh_indices:
        BTreeMap<CompactString, (RelationHandle, RelationHandle, MinHashLshIndexManifest)>,
    pub(crate) description: CompactString,
}

impl RelationHandle {
    pub(crate) fn has_index(&self, index_name: &str) -> bool {
        self.indices.contains_key(index_name)
            || self.hnsw_indices.contains_key(index_name)
            || self.fts_indices.contains_key(index_name)
            || self.lsh_indices.contains_key(index_name)
    }
    pub(crate) fn has_no_index(&self) -> bool {
        self.indices.is_empty()
            && self.hnsw_indices.is_empty()
            && self.fts_indices.is_empty()
            && self.lsh_indices.is_empty()
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    Default,
    Ord,
    PartialOrd,
)]
#[non_exhaustive]
pub enum AccessLevel {
    Hidden,
    ReadOnly,
    Protected,
    #[default]
    Normal,
}

impl Display for AccessLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessLevel::Normal => f.write_str("normal"),
            AccessLevel::Protected => f.write_str("protected"),
            AccessLevel::ReadOnly => f.write_str("read_only"),
            AccessLevel::Hidden => f.write_str("hidden"),
        }
    }
}

#[derive(Debug, Snafu)]
#[snafu(display(
    "Arity mismatch for stored relation {name}: expect {expect_arity}, got {actual_arity}"
))]
pub(crate) struct StoredRelArityMismatch {
    name: String,
    expect_arity: usize,
    actual_arity: usize,
    span: SourceSpan,
}

impl RelationHandle {
    pub(crate) fn raw_binding_map(&self) -> BTreeMap<Symbol, usize> {
        let mut ret = BTreeMap::new();
        for (i, col) in self.metadata.keys.iter().enumerate() {
            ret.insert(Symbol::new(col.name.clone(), Default::default()), i);
        }
        for (i, col) in self.metadata.non_keys.iter().enumerate() {
            ret.insert(
                Symbol::new(col.name.clone(), Default::default()),
                i + self.metadata.keys.len(),
            );
        }
        ret
    }
    pub(crate) fn has_triggers(&self) -> bool {
        !self.put_triggers.is_empty() || !self.rm_triggers.is_empty()
    }
    fn encode_key_prefix(&self, len: usize) -> Vec<u8> {
        let mut ret = Vec::with_capacity(4 + 4 * len + 10 * len);
        let prefix_bytes = self.id.0.to_be_bytes();
        ret.extend(prefix_bytes);
        ret
    }
    pub(crate) fn as_named_rows(&self, tx: &SessionTx<'_>) -> Result<NamedRows> {
        let rows: Vec<_> = self.scan_all(tx).try_collect()?;
        let mut headers = self
            .metadata
            .keys
            .iter()
            .map(|col| col.name.to_string())
            .collect_vec();
        headers.extend(
            self.metadata
                .non_keys
                .iter()
                .map(|col| col.name.to_string()),
        );
        Ok(NamedRows::new(headers, rows))
    }
    #[expect(
        clippy::expect_used,
        reason = "arg_uses and mapper guaranteed non-empty by callers"
    )]
    pub(crate) fn choose_index(
        &self,
        arg_uses: &[IndexPositionUse],
        validity_query: bool,
    ) -> Option<(RelationHandle, Vec<usize>, bool)> {
        if self.indices.is_empty() {
            return None;
        }
        if *arg_uses.first().expect("arg_uses is non-empty") == IndexPositionUse::Join {
            return None;
        }
        let mut max_prefix_len = 0;
        let required_positions = arg_uses
            .iter()
            .enumerate()
            .filter_map(|(i, pos_use)| {
                if *pos_use != IndexPositionUse::Ignored {
                    Some(i)
                } else {
                    None
                }
            })
            .collect_vec();
        let mut chosen = None;
        for (manifest, mapper) in self.indices.values() {
            if validity_query
                && *mapper.last().expect("mapper is non-empty") != self.metadata.keys.len() - 1
            {
                continue;
            }

            let mut cur_prefix_len = 0;
            for i in mapper {
                if arg_uses[*i] == IndexPositionUse::Join {
                    cur_prefix_len += 1;
                } else {
                    break;
                }
            }
            if cur_prefix_len > max_prefix_len {
                max_prefix_len = cur_prefix_len;
                let mut need_join = false;
                for need_pos in required_positions.iter() {
                    if !mapper.contains(need_pos) {
                        need_join = true;
                        break;
                    }
                }
                chosen = Some((manifest.clone(), mapper.clone(), need_join))
            }
        }
        chosen
    }
    pub(crate) fn encode_key_for_store(
        &self,
        tuple: &[DataValue],
        span: SourceSpan,
    ) -> Result<Vec<u8>> {
        let len = self.metadata.keys.len();
        if tuple.len() < len {
            StoredRelArityMismatchSnafu {
                name: self.name.to_string(),
                expect_arity: self.arity(),
                actual_arity: tuple.len(),
                span,
            }
            .fail()?;
        }
        let mut ret = self.encode_key_prefix(len);
        for val in &tuple[0..len] {
            ret.encode_datavalue(val);
        }
        Ok(ret)
    }
    pub(crate) fn encode_partial_key_for_store(&self, tuple: &[DataValue]) -> Vec<u8> {
        let mut ret = self.encode_key_prefix(tuple.len());
        for val in tuple {
            ret.encode_datavalue(val);
        }
        ret
    }
    pub(crate) fn encode_val_for_store(
        &self,
        tuple: &[DataValue],
        _span: SourceSpan,
    ) -> Result<Vec<u8>> {
        let start = self.metadata.keys.len();
        let len = self.metadata.non_keys.len();
        let mut ret = self.encode_key_prefix(len);
        tuple[start..]
            .serialize(&mut Serializer::new(&mut ret))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        Ok(ret)
    }
    pub(crate) fn encode_val_only_for_store(
        &self,
        tuple: &[DataValue],
        _span: SourceSpan,
    ) -> Result<Vec<u8>> {
        let mut ret = self.encode_key_prefix(tuple.len());
        tuple
            .serialize(&mut Serializer::new(&mut ret))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        Ok(ret)
    }
    pub(crate) fn ensure_compatible(
        &self,
        inp: &InputRelationHandle,
        is_remove_or_update: bool,
    ) -> Result<()> {
        let InputRelationHandle { metadata, .. } = inp;
        // check that every given key is found and compatible
        for col in metadata.keys.iter().chain(self.metadata.non_keys.iter()) {
            self.metadata.compatible_with_col(col)?
        }
        // check that every key is provided or has default
        for col in &self.metadata.keys {
            metadata.satisfied_by_required_col(col)?;
        }
        if !is_remove_or_update {
            for col in &self.metadata.non_keys {
                metadata.satisfied_by_required_col(col)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct InputRelationHandle {
    pub(crate) name: Symbol,
    pub(crate) metadata: StoredRelationMetadata,
    pub(crate) key_bindings: Vec<Symbol>,
    pub(crate) dep_bindings: Vec<Symbol>,
    pub(crate) span: SourceSpan,
}

impl Debug for RelationHandle {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Relation<{}>", self.name)
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("Cannot deserialize relation"))]
pub(crate) struct RelationDeserError;

impl RelationHandle {
    pub(crate) fn arity(&self) -> usize {
        self.metadata.non_keys.len() + self.metadata.keys.len()
    }
    pub(crate) fn decode(data: &[u8]) -> Result<Self> {
        rmp_serde::from_slice(data).map_err(|e| {
            error!(
                error = %e,
                "cannot deserialize relation metadata"
            );
            crate::engine::error::InternalError::Runtime {
                source: InvalidOperationSnafu {
                    op: "stored_relation",
                    reason: format!("cannot deserialize relation: {e}"),
                }
                .build(),
            }
        })
    }
    pub(crate) fn scan_all<'a>(
        &self,
        tx: &'a SessionTx<'_>,
    ) -> impl Iterator<Item = Result<Tuple>> + use<'a> {
        let lower = Tuple::default().encode_as_key(self.id);
        #[expect(
            clippy::expect_used,
            reason = "RelationId from store is always < 2^48; next() cannot overflow"
        )]
        let upper =
            Tuple::default().encode_as_key(self.id.next().expect("stored RelationId overflow"));
        if self.is_temp {
            tx.temp_store_tx.range_scan_tuple(&lower, &upper)
        } else {
            tx.store_tx.range_scan_tuple(&lower, &upper)
        }
    }

    pub(crate) fn skip_scan_all<'a>(
        &self,
        tx: &'a SessionTx<'_>,
        valid_at: ValidityTs,
    ) -> impl Iterator<Item = Result<Tuple>> + use<'a> {
        let lower = Tuple::default().encode_as_key(self.id);
        #[expect(
            clippy::expect_used,
            reason = "RelationId from store is always < 2^48; next() cannot overflow"
        )]
        let upper =
            Tuple::default().encode_as_key(self.id.next().expect("stored RelationId overflow"));
        if self.is_temp {
            tx.temp_store_tx
                .range_skip_scan_tuple(&lower, &upper, valid_at)
        } else {
            tx.store_tx.range_skip_scan_tuple(&lower, &upper, valid_at)
        }
    }

    pub(crate) fn get(&self, tx: &SessionTx<'_>, key: &[DataValue]) -> Result<Option<Tuple>> {
        let key_data = key.encode_as_key(self.id);
        if self.is_temp {
            Ok(tx
                .temp_store_tx
                .get(&key_data, false)?
                .map(|val_data| decode_tuple_from_kv(&key_data, &val_data, Some(self.arity()))))
        } else {
            Ok(tx
                .store_tx
                .get(&key_data, false)?
                .map(|val_data| decode_tuple_from_kv(&key_data, &val_data, Some(self.arity()))))
        }
    }

    pub(crate) fn get_val_only(
        &self,
        tx: &SessionTx<'_>,
        key: &[DataValue],
    ) -> Result<Option<Tuple>> {
        let key_data = key.encode_as_key(self.id);
        if self.is_temp {
            tx.temp_store_tx
                .get(&key_data, false)?
                .map(|val_data| {
                    rmp_serde::from_slice::<Vec<DataValue>>(&val_data[ENCODED_KEY_MIN_LEN..])
                        .map_err(|e| crate::engine::error::InternalError::Runtime {
                            source: InvalidOperationSnafu {
                                op: "stored_relation",
                                reason: format!("failed to deserialize stored tuple: {e}"),
                            }
                            .build(),
                        })
                })
                .transpose()
        } else {
            tx.store_tx
                .get(&key_data, false)?
                .map(|val_data| {
                    rmp_serde::from_slice::<Vec<DataValue>>(&val_data[ENCODED_KEY_MIN_LEN..])
                        .map_err(|e| crate::engine::error::InternalError::Runtime {
                            source: InvalidOperationSnafu {
                                op: "stored_relation",
                                reason: format!("failed to deserialize stored tuple: {e}"),
                            }
                            .build(),
                        })
                })
                .transpose()
        }
    }

    pub(crate) fn exists(&self, tx: &SessionTx<'_>, key: &[DataValue]) -> Result<bool> {
        let key_data = key.encode_as_key(self.id);
        if self.is_temp {
            tx.temp_store_tx
                .exists(&key_data, false)
                .map_err(Into::into)
        } else {
            tx.store_tx.exists(&key_data, false).map_err(Into::into)
        }
    }

    pub(crate) fn scan_prefix<'a>(
        &self,
        tx: &'a SessionTx<'_>,
        prefix: &Tuple,
    ) -> impl Iterator<Item = Result<Tuple>> + use<'a> {
        let mut lower = prefix.clone();
        lower.truncate(self.metadata.keys.len());
        let mut upper = lower.clone();
        upper.push(DataValue::Bot);
        let prefix_encoded = lower.encode_as_key(self.id);
        let upper_encoded = upper.encode_as_key(self.id);
        if self.is_temp {
            tx.temp_store_tx
                .range_scan_tuple(&prefix_encoded, &upper_encoded)
        } else {
            tx.store_tx
                .range_scan_tuple(&prefix_encoded, &upper_encoded)
        }
    }

    pub(crate) fn skip_scan_prefix<'a>(
        &self,
        tx: &'a SessionTx<'_>,
        prefix: &Tuple,
        valid_at: ValidityTs,
    ) -> impl Iterator<Item = Result<Tuple>> + use<'a> {
        let mut lower = prefix.clone();
        lower.truncate(self.metadata.keys.len());
        let mut upper = lower.clone();
        upper.push(DataValue::Bot);
        let prefix_encoded = lower.encode_as_key(self.id);
        let upper_encoded = upper.encode_as_key(self.id);
        if self.is_temp {
            tx.temp_store_tx
                .range_skip_scan_tuple(&prefix_encoded, &upper_encoded, valid_at)
        } else {
            tx.store_tx
                .range_skip_scan_tuple(&prefix_encoded, &upper_encoded, valid_at)
        }
    }

    pub(crate) fn scan_bounded_prefix<'a>(
        &self,
        tx: &'a SessionTx<'_>,
        prefix: &[DataValue],
        lower: &[DataValue],
        upper: &[DataValue],
    ) -> impl Iterator<Item = Result<Tuple>> + use<'a> {
        let mut lower_t = prefix.to_vec();
        lower_t.extend_from_slice(lower);
        let mut upper_t = prefix.to_vec();
        upper_t.extend_from_slice(upper);
        upper_t.push(DataValue::Bot);
        let lower_encoded = lower_t.encode_as_key(self.id);
        let upper_encoded = upper_t.encode_as_key(self.id);
        if self.is_temp {
            tx.temp_store_tx
                .range_scan_tuple(&lower_encoded, &upper_encoded)
        } else {
            tx.store_tx.range_scan_tuple(&lower_encoded, &upper_encoded)
        }
    }
    pub(crate) fn skip_scan_bounded_prefix<'a>(
        &self,
        tx: &'a SessionTx<'_>,
        prefix: &Tuple,
        lower: &[DataValue],
        upper: &[DataValue],
        valid_at: ValidityTs,
    ) -> impl Iterator<Item = Result<Tuple>> + use<'a> {
        let mut lower_t = prefix.clone();
        lower_t.extend_from_slice(lower);
        let mut upper_t = prefix.clone();
        upper_t.extend_from_slice(upper);
        upper_t.push(DataValue::Bot);
        let lower_encoded = lower_t.encode_as_key(self.id);
        let upper_encoded = upper_t.encode_as_key(self.id);
        if self.is_temp {
            tx.temp_store_tx
                .range_skip_scan_tuple(&lower_encoded, &upper_encoded, valid_at)
        } else {
            tx.store_tx
                .range_skip_scan_tuple(&lower_encoded, &upper_encoded, valid_at)
        }
    }
}

const DEFAULT_SIZE_HINT: usize = 16;

/// Decode tuple from key-value pairs. Used for customizing storage
/// in trait [`StoreTx`](crate::engine::StoreTx).
#[inline]
pub fn decode_tuple_from_kv(key: &[u8], val: &[u8], size_hint: Option<usize>) -> Tuple {
    let mut tup = decode_tuple_from_key(key, size_hint.unwrap_or(DEFAULT_SIZE_HINT));
    extend_tuple_from_v(&mut tup, val);
    tup
}

#[expect(
    clippy::expect_used,
    reason = "storage layer invariant — msgpack corruption is unrecoverable"
)]
pub fn extend_tuple_from_v(key: &mut Tuple, val: &[u8]) {
    if !val.is_empty() {
        // INVARIANT: storage layer writes well-formed msgpack tuples; deserialization only fails on data corruption
        let vals: Vec<DataValue> = rmp_serde::from_slice(&val[ENCODED_KEY_MIN_LEN..])
            .expect("INVARIANT: storage layer writes well-formed msgpack tuples");
        key.extend(vals);
    }
}
