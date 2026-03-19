//! Stored relation management.

mod handles;
mod index_create;
mod index_management;
mod relation_crud;

pub(crate) use handles::{
    AccessLevel, InputRelationHandle, RelationHandle, RelationId, StoredRelArityMismatch,
    decode_tuple_from_kv, extend_tuple_from_v,
};
