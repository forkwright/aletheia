//! aletheia-mneme — session store and memory engine
//!
//! Mneme (Μνήμη) — "memory." Manages sessions, messages, usage tracking, and
//! structured knowledge. Two embedded storage backends:
//! - `SQLite` (`store::SessionStore`) — conversation history, distillation metadata, agent notes
//! - `CozoDB` (`knowledge_store::KnowledgeStore`) — fact graph, entity relationships, HNSW vector index
//!
//! ## Key types
//!
//! - [`store::SessionStore`] — open/create sessions, append messages, record distillation
//! - [`types::Session`] / [`types::Message`] — session and message records from `SQLite`
//! - [`types::SessionStatus`] / [`types::SessionType`] / [`types::Role`] — enum domain types
//! - [`knowledge::Fact`] / [`knowledge::Entity`] / [`knowledge::Relationship`] — `CozoDB` domain types
//! - `extract::ExtractionEngine` — LLM-driven fact/entity extraction from conversations
//! - [`embedding::EmbeddingProvider`] — text-to-vector interface for semantic recall
//! - `recall::RecallQuery` — structured semantic + graph recall query
//!
//! Depends on `aletheia-koina` for shared types.

pub mod embedding;
pub mod error;
#[cfg_attr(not(test), expect(dead_code, reason = "used in module tests only"))]
pub(crate) mod extract;
pub mod knowledge;
pub mod knowledge_store;
#[cfg(feature = "mneme-engine")]
pub(crate) mod query;
pub mod recall;
#[cfg(feature = "sqlite")]
pub mod schema;
#[cfg(feature = "sqlite")]
pub mod store;
pub mod types;

#[cfg(all(test, feature = "sqlite"))]
mod assertions {
    use super::store::SessionStore;
    use static_assertions::assert_impl_all;

    assert_impl_all!(SessionStore: Send);
}
