//! Response envelope types and parser functions for the memory view API calls.

use crate::state::memory::{Entity, EntityMemory, Fact, Relationship};

/// Response wrapper for the facts endpoint.
#[derive(Debug, serde::Deserialize)]
pub(super) struct FactsResponse {
    #[serde(default)]
    pub(super) facts: Vec<Fact>,
    #[serde(default)]
    pub(super) total: usize,
}

/// Response wrapper for entity list endpoint.
#[derive(Debug, serde::Deserialize)]
pub(super) struct EntitiesResponse {
    pub(super) entities: Vec<Entity>,
}

#[derive(Debug, serde::Deserialize)]
struct RelationshipsResponse {
    #[serde(default)]
    relationships: Vec<Relationship>,
}

#[derive(Debug, serde::Deserialize)]
struct EntityMemoriesResponse {
    #[serde(default)]
    memories: Vec<EntityMemory>,
}

/// Parse the `{facts, total}` envelope, falling back to a bare array.
pub(super) fn parse_facts_response(text: &str) -> (Vec<Fact>, usize) {
    match serde_json::from_str::<FactsResponse>(text) {
        Ok(resp) => {
            let total = if resp.total == 0 {
                resp.facts.len()
            } else {
                resp.total
            };
            (resp.facts, total)
        }
        Err(wrapped_err) => match serde_json::from_str::<Vec<Fact>>(text) {
            Ok(list) => {
                let total = list.len();
                (list, total)
            }
            Err(array_err) => {
                tracing::warn!(
                    wrapped_error = %wrapped_err,
                    array_error = %array_err,
                    "failed to parse facts response"
                );
                (Vec::new(), 0)
            }
        },
    }
}

/// Parse the `{relationships: [...]}` envelope, falling back to a bare array.
pub(super) fn parse_relationships_response(text: &str) -> Vec<Relationship> {
    match serde_json::from_str::<RelationshipsResponse>(text) {
        Ok(wrapper) => wrapper.relationships,
        Err(wrapped_err) => match serde_json::from_str::<Vec<Relationship>>(text) {
            Ok(list) => list,
            Err(array_err) => {
                tracing::warn!(
                    wrapped_error = %wrapped_err,
                    array_error = %array_err,
                    "failed to parse relationships response"
                );
                Vec::new()
            }
        },
    }
}

/// Parse the `{memories: [...]}` envelope, falling back to a bare array.
pub(super) fn parse_entity_memories_response(text: &str) -> Vec<EntityMemory> {
    match serde_json::from_str::<EntityMemoriesResponse>(text) {
        Ok(wrapper) => wrapper.memories,
        Err(wrapped_err) => match serde_json::from_str::<Vec<EntityMemory>>(text) {
            Ok(list) => list,
            Err(array_err) => {
                tracing::warn!(
                    wrapped_error = %wrapped_err,
                    array_error = %array_err,
                    "failed to parse entity memories response"
                );
                Vec::new()
            }
        },
    }
}
