//! Shared entity/relationship/store fixtures for engine-dependent graph tests.
use crate::knowledge::{Entity, Relationship};
use crate::knowledge_store::KnowledgeStore;

pub(super) fn test_store() -> std::sync::Arc<KnowledgeStore> {
    KnowledgeStore::open_mem().expect("open_mem")
}

pub(super) fn make_entity(id: &str, name: &str) -> Entity {
    Entity {
        id: crate::id::EntityId::new(id).expect("valid test id"),
        name: name.to_owned(),
        entity_type: "person".to_owned(),
        aliases: vec![],
        created_at: jiff::Timestamp::now(),
        updated_at: jiff::Timestamp::now(),
    }
}

pub(super) fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
    Relationship {
        src: crate::id::EntityId::new(src).expect("valid test id"),
        dst: crate::id::EntityId::new(dst).expect("valid test id"),
        relation: relation.to_owned(),
        weight,
        created_at: jiff::Timestamp::now(),
    }
}
