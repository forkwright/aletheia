//! Stored relation management.
//!
//! A relation is the engine's persistent or temporary key-value store for
//! tuples. Each relation has a `RelationHandle` containing its schema
//! (keys, non-keys, indices) and access level.
//!
//! Submodules:
//! - `handles`: Type definitions (`RelationId`, `RelationHandle`, `AccessLevel`)
//! - `relation_crud`: Create, get, destroy, rename, describe relations
//! - `index_create`: FTS, HNSW, and MinHash-LSH index creation
//! - `index_management`: Column index creation, removal, relation renaming

mod handles;
mod index_create;
mod index_management;
mod relation_crud;

pub(crate) use handles::{
    AccessLevel, InputRelationHandle, RelationHandle, RelationId, StoredRelArityMismatch,
    decode_tuple_from_kv, extend_tuple_from_v,
};
