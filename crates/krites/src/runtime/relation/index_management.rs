//! SessionTx methods: column index management and relation renaming.
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use itertools::Itertools;
use rmp_serde::Serializer;
use serde::Serialize;

use crate::StoreTx;
use crate::data::relation::StoredRelationMetadata;
use crate::data::symb::Symbol;
use crate::data::tuple::TupleT;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::runtime::error::{
    IndexAlreadyExistsSnafu, IndexNotFoundSnafu, InsufficientAccessSnafu, InvalidOperationSnafu,
    RelationAlreadyExistsSnafu, SerializationSnafu,
};
use crate::runtime::transact::SessionTx;
use crate::utils::TempCollector;

use super::handles::{AccessLevel, InputRelationHandle, RelationId};

impl<'a> SessionTx<'a> {
    pub(crate) fn create_index(
        &mut self,
        rel_name: &Symbol,
        idx_name: &Symbol,
        cols: &[Symbol],
    ) -> Result<()> {
        let mut rel_handle = self.get_relation(rel_name, true)?;

        if rel_handle.has_index(&idx_name.name) {
            IndexAlreadyExistsSnafu {
                index_name: idx_name.name.to_string(),
                relation_name: rel_name.name.to_string(),
            }
            .fail()?;
        }

        let mut col_defs = vec![];
        'outer: for col in cols.iter() {
            for orig_col in rel_handle
                .metadata
                .keys
                .iter()
                .chain(rel_handle.metadata.non_keys.iter())
            {
                if orig_col.name == col.name {
                    col_defs.push(orig_col.clone());
                    continue 'outer;
                }
            }

            InvalidOperationSnafu {
                op: "create index",
                reason: format!(
                    "column '{}' not found in relation '{}'",
                    col.name, rel_name.name
                ),
            }
            .fail()?;
        }

        'outer: for key in rel_handle.metadata.keys.iter() {
            for col in cols.iter() {
                if col.name == key.name {
                    continue 'outer;
                }
            }
            col_defs.push(key.clone());
        }

        let key_bindings = col_defs
            .iter()
            .map(|col| Symbol::new(col.name.clone(), Default::default()))
            .collect_vec();
        let idx_meta = StoredRelationMetadata {
            keys: col_defs,
            non_keys: vec![],
        };

        let idx_handle = InputRelationHandle {
            name: Symbol::new(
                format!("{}:{}", rel_name.name, idx_name.name),
                Default::default(),
            ),
            metadata: idx_meta,
            key_bindings,
            dep_bindings: vec![],
            span: Default::default(),
        };

        let idx_handle = self.create_relation(idx_handle)?;

        let extraction_indices = idx_handle
            .metadata
            .keys
            .iter()
            .map(|col| {
                for (i, kc) in rel_handle.metadata.keys.iter().enumerate() {
                    if kc.name == col.name {
                        return i;
                    }
                }
                for (i, kc) in rel_handle.metadata.non_keys.iter().enumerate() {
                    if kc.name == col.name {
                        return i + rel_handle.metadata.keys.len();
                    }
                }
                unreachable!()
            })
            .collect_vec();

        if self.store_tx.supports_par_put() {
            for tuple in rel_handle.scan_all(self) {
                let tuple = tuple?;
                let extracted = extraction_indices
                    .iter()
                    .map(|idx| tuple[*idx].clone())
                    .collect_vec();
                let key = idx_handle.encode_key_for_store(&extracted, Default::default())?;
                self.store_tx.par_put(&key, &[])?;
            }
        } else {
            let mut existing = TempCollector::default();
            for tuple in rel_handle.scan_all(self) {
                existing.push(tuple?);
            }
            for tuple in existing.into_iter() {
                let extracted = extraction_indices
                    .iter()
                    .map(|idx| tuple[*idx].clone())
                    .collect_vec();
                let key = idx_handle.encode_key_for_store(&extracted, Default::default())?;
                self.store_tx.put(&key, &[])?;
            }
        }

        rel_handle
            .indices
            .insert(idx_name.name.clone(), (idx_handle, extraction_indices));

        let new_encoded =
            vec![DataValue::from(&rel_name.name as &str)].encode_as_key(RelationId::SYSTEM);
        let mut meta_val = vec![];
        rel_handle
            .serialize(&mut Serializer::new(&mut meta_val))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.store_tx.put(&new_encoded, &meta_val)?;

        Ok(())
    }

    #[expect(
        clippy::expect_used,
        reason = "RwLock poisoning is unrecoverable — propagating would leave caches inconsistent"
    )]
    pub(crate) fn remove_index(
        &mut self,
        rel_name: &Symbol,
        idx_name: &Symbol,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut rel = self.get_relation(rel_name, true)?;
        let is_lsh = rel.lsh_indices.contains_key(&idx_name.name);
        let is_fts = rel.fts_indices.contains_key(&idx_name.name);
        if is_lsh || is_fts {
            self.tokenizers
                .named_cache
                .write()
                .expect("lock poisoned")
                .clear();
            self.tokenizers
                .hashed_cache
                .write()
                .expect("lock poisoned")
                .clear();
        }
        if rel.indices.remove(&idx_name.name).is_none()
            && rel.hnsw_indices.remove(&idx_name.name).is_none()
            && rel.lsh_indices.remove(&idx_name.name).is_none()
            && rel.fts_indices.remove(&idx_name.name).is_none()
        {
            IndexNotFoundSnafu {
                relation_name: rel_name.name.to_string(),
            }
            .fail()?;
        }

        let mut to_clean =
            self.destroy_relation(&format!("{}:{}", rel_name.name, idx_name.name))?;
        if is_lsh {
            to_clean.extend(
                self.destroy_relation(&format!("{}:{}:inv", rel_name.name, idx_name.name))?,
            );
        }

        let new_encoded =
            vec![DataValue::from(&rel_name.name as &str)].encode_as_key(RelationId::SYSTEM);
        let mut meta_val = vec![];
        rel.serialize(&mut Serializer::new(&mut meta_val))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.store_tx.put(&new_encoded, &meta_val)?;

        Ok(to_clean)
    }

    pub(crate) fn rename_relation(&mut self, old: &Symbol, new: &Symbol) -> Result<()> {
        if old.name.starts_with('_') || new.name.starts_with('_') {
            InvalidOperationSnafu {
                op: "rename relation",
                reason: "temp store names (starting with '_') cannot be renamed",
            }
            .fail()?;
        }
        let new_key = DataValue::Str(new.name.clone());
        let new_encoded = vec![new_key].encode_as_key(RelationId::SYSTEM);

        if self.store_tx.exists(&new_encoded, true)? {
            RelationAlreadyExistsSnafu {
                name: new.name.to_string(),
            }
            .fail()?;
        };

        let old_key = DataValue::Str(old.name.clone());
        let old_encoded = vec![old_key].encode_as_key(RelationId::SYSTEM);

        let mut rel = self.get_relation(old, true)?;
        if rel.access_level < AccessLevel::Normal {
            InsufficientAccessSnafu {
                operation: format!("rename stored relation '{}'", old.name),
            }
            .fail()?;
        }
        rel.name = new.name.clone();

        let mut meta_val = vec![];
        rel.serialize(&mut Serializer::new(&mut meta_val))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.store_tx.del(&old_encoded)?;
        self.store_tx.put(&new_encoded, &meta_val)?;

        Ok(())
    }
    pub(crate) fn rename_temp_relation(&mut self, old: Symbol, new: Symbol) -> Result<()> {
        let new_key = DataValue::Str(new.name.clone());
        let new_encoded = vec![new_key].encode_as_key(RelationId::SYSTEM);

        if self.temp_store_tx.exists(&new_encoded, true)? {
            RelationAlreadyExistsSnafu {
                name: new.name.to_string(),
            }
            .fail()?;
        };

        let old_key = DataValue::Str(old.name.clone());
        let old_encoded = vec![old_key].encode_as_key(RelationId::SYSTEM);

        let mut rel = self.get_relation(&old, true)?;
        rel.name = new.name;

        let mut meta_val = vec![];
        rel.serialize(&mut Serializer::new(&mut meta_val))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.temp_store_tx.del(&old_encoded)?;
        self.temp_store_tx.put(&new_encoded, &meta_val)?;

        Ok(())
    }
}
