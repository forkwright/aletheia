//! SessionTx methods: relation CRUD.
#![expect(
    clippy::as_conversions,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::sync::atomic::Ordering;

use compact_str::CompactString;
use rmp_serde::Serializer;
use serde::Serialize;

use crate::engine::StoreTx;
use crate::engine::data::symb::Symbol;
use crate::engine::data::tuple::{Tuple, TupleT};
use crate::engine::data::value::DataValue;
use crate::engine::error::InternalResult as Result;
use crate::engine::runtime::error::{
    InsufficientAccessSnafu, InvalidOperationSnafu, RelationAlreadyExistsSnafu, SerializationSnafu,
};
use crate::engine::runtime::transact::SessionTx;

use super::handles::{AccessLevel, InputRelationHandle, RelationHandle, RelationId};

impl<'a> SessionTx<'a> {
    pub(crate) fn relation_exists(&self, name: &str) -> Result<bool> {
        let key = DataValue::from(name);
        let encoded = vec![key].encode_as_key(RelationId::SYSTEM);
        if name.starts_with('_') {
            self.temp_store_tx
                .exists(&encoded, false)
                .map_err(Into::into)
        } else {
            self.store_tx.exists(&encoded, false).map_err(Into::into)
        }
    }
    pub(crate) fn set_relation_triggers(
        &mut self,
        name: &Symbol,
        puts: &[String],
        rms: &[String],
        replaces: &[String],
    ) -> Result<()> {
        if name.name.starts_with('_') {
            InvalidOperationSnafu {
                op: "set triggers",
                reason: "cannot set triggers for temp store",
            }
            .fail()?;
        }
        let mut original = self.get_relation(name, true)?;
        if original.access_level < AccessLevel::Protected {
            InsufficientAccessSnafu {
                operation: "set triggers on stored relation",
            }
            .fail()?;
        }
        original.put_triggers = puts.to_vec();
        original.rm_triggers = rms.to_vec();
        original.replace_triggers = replaces.to_vec();

        let name_key =
            vec![DataValue::Str(original.name.clone())].encode_as_key(RelationId::SYSTEM);

        let mut meta_val = vec![];
        original
            .serialize(&mut Serializer::new(&mut meta_val).with_struct_map())
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.store_tx.put(&name_key, &meta_val)?;

        Ok(())
    }
    pub(crate) fn create_relation(
        &mut self,
        input_meta: InputRelationHandle,
    ) -> Result<RelationHandle> {
        let key = DataValue::Str(input_meta.name.name.clone());
        let encoded = vec![key].encode_as_key(RelationId::SYSTEM);

        let is_temp = input_meta.name.is_temp_store_name();

        if is_temp {
            if self.store_tx.exists(&encoded, true)? {
                RelationAlreadyExistsSnafu {
                    name: input_meta.name.name.to_string(),
                }
                .fail()?;
            };
        } else if self.temp_store_tx.exists(&encoded, true)? {
            RelationAlreadyExistsSnafu {
                name: input_meta.name.name.to_string(),
            }
            .fail()?;
        }

        let metadata = input_meta.metadata.clone();
        let last_id = if is_temp {
            self.temp_store_id.fetch_add(1, Ordering::Relaxed) as u64
        } else {
            self.relation_store_id.fetch_add(1, Ordering::SeqCst)
        };
        let meta = RelationHandle {
            name: input_meta.name.name,
            id: RelationId::new(last_id + 1)?,
            metadata,
            put_triggers: vec![],
            rm_triggers: vec![],
            replace_triggers: vec![],
            access_level: AccessLevel::Normal,
            is_temp,
            indices: Default::default(),
            hnsw_indices: Default::default(),
            fts_indices: Default::default(),
            lsh_indices: Default::default(),
            description: Default::default(),
        };

        let name_key = vec![DataValue::Str(meta.name.clone())].encode_as_key(RelationId::SYSTEM);
        let mut meta_val = vec![];
        meta.serialize(&mut Serializer::new(&mut meta_val).with_struct_map())
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        let tuple = vec![DataValue::Null];
        let t_encoded = tuple.encode_as_key(RelationId::SYSTEM);

        if is_temp {
            self.temp_store_tx.put(&encoded, &meta.id.raw_encode())?;
            self.temp_store_tx.put(&name_key, &meta_val)?;
            self.temp_store_tx.put(&t_encoded, &meta.id.raw_encode())?;
        } else {
            self.store_tx.put(&encoded, &meta.id.raw_encode())?;
            self.store_tx.put(&name_key, &meta_val)?;
            self.store_tx.put(&t_encoded, &meta.id.raw_encode())?;
        }

        Ok(meta)
    }
    pub(crate) fn get_relation(&self, name: &str, lock: bool) -> Result<RelationHandle> {
        let key = DataValue::from(name);
        let encoded = vec![key].encode_as_key(RelationId::SYSTEM);

        let found = if name.starts_with('_') {
            self.temp_store_tx.get(&encoded, lock)?.ok_or_else(|| {
                crate::engine::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "relation",
                        reason: "Cannot find requested stored relation",
                    }
                    .build(),
                }
            })?
        } else {
            self.store_tx.get(&encoded, lock)?.ok_or_else(|| {
                crate::engine::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "relation",
                        reason: "Cannot find requested stored relation",
                    }
                    .build(),
                }
            })?
        };
        let metadata = RelationHandle::decode(&found)?;
        Ok(metadata)
    }
    pub(crate) fn describe_relation(&mut self, name: &str, description: &str) -> Result<()> {
        let mut meta = self.get_relation(name, true)?;

        meta.description = CompactString::from(description);
        let name_key = vec![DataValue::Str(meta.name.clone())].encode_as_key(RelationId::SYSTEM);
        let mut meta_val = vec![];
        meta.serialize(&mut Serializer::new(&mut meta_val).with_struct_map())
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        if meta.is_temp {
            self.temp_store_tx.put(&name_key, &meta_val)?;
        } else {
            self.store_tx.put(&name_key, &meta_val)?;
        }

        Ok(())
    }
    pub(crate) fn destroy_relation(&mut self, name: &str) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let is_temp = name.starts_with('_');
        let mut to_clean = vec![];

        let store = self.get_relation(name, true)?;
        if !store.has_no_index() {
            InvalidOperationSnafu {
                op: "remove relation",
                reason: format!("stored relation `{name}` has indices attached"),
            }
            .fail()?;
        }
        if store.access_level < AccessLevel::Normal {
            InsufficientAccessSnafu {
                operation: "remove stored relation",
            }
            .fail()?;
        }

        for k in store.hnsw_indices.keys() {
            let more_to_clean = self.destroy_relation(&format!("{name}:{k}"))?;
            to_clean.extend(more_to_clean);
        }

        let key = DataValue::from(name);
        let encoded = vec![key].encode_as_key(RelationId::SYSTEM);
        if is_temp {
            self.temp_store_tx.del(&encoded)?;
        } else {
            self.store_tx.del(&encoded)?;
        }
        let lower_bound = Tuple::default().encode_as_key(store.id);
        let upper_bound = Tuple::default().encode_as_key(store.id.next()?);
        to_clean.push((lower_bound, upper_bound));
        Ok(to_clean)
    }
    pub(crate) fn set_access_level(&mut self, rel: &Symbol, level: AccessLevel) -> Result<()> {
        let mut meta = self.get_relation(rel, true)?;
        meta.access_level = level;

        let name_key = vec![DataValue::Str(meta.name.clone())].encode_as_key(RelationId::SYSTEM);

        let mut meta_val = vec![];
        meta.serialize(&mut Serializer::new(&mut meta_val).with_struct_map())
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.store_tx.put(&name_key, &meta_val)?;

        Ok(())
    }
}
