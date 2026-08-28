//! Canonical `KnowledgeConfig` derivation for the CLI's direct-store commands.
//!
//! This crate ships no `lib.rs` (three independent `[[bin]]` targets), so
//! this file is shared by `mod` inclusion rather than an ordinary `use`:
//! `main.rs` declares it at its default path, and `bin/seed_psyche_facts.rs`
//! / `bin/golden_set_harness.rs` reach it via `#[path = "../knowledge_config.rs"]`
//! — one source file, three build inputs, instead of the six-line
//! `KnowledgeConfig` construction restated at each call site (#7023). Every
//! function here is infallible: a caller that needs the stricter
//! hard-error-on-unloadable-config resolution (`golden_set_harness`, which
//! also needs the raw embedding config alongside) composes it locally from
//! [`derive_knowledge_config`]/[`DerivedKnowledgeConfig::to_knowledge_config`]
//! rather than this file carrying a variant only one of the three binaries
//! calls — an unused `pub fn` here is dead code in the *other* two binaries'
//! separate compilations of this same source.

/// The two `KnowledgeConfig` fields an instance config actually determines,
/// kept as a small `Clone` DTO because `KnowledgeConfig` itself holds a
/// `Box<dyn AdmissionPolicy>` and is therefore not `Clone` — a caller that
/// opens more than one store from one derivation (`memory reembed`/`gc`
/// iterate every cohort store on disk) builds a fresh `KnowledgeConfig` per
/// store from this rather than re-deriving from the loaded config each time.
#[derive(Debug, Clone)]
pub struct DerivedKnowledgeConfig {
    pub dim: usize,
    pub embedding_model: String,
}

impl DerivedKnowledgeConfig {
    /// Build a full `KnowledgeConfig`. `allow_assumed_embedding_meta` stays a
    /// call-site parameter rather than part of this DTO: it varies by
    /// operation, not by config (recovery tooling like `reembed`/`gc` sets it
    /// `true`; ordinary store access leaves it `false`).
    #[must_use]
    pub fn to_knowledge_config(
        &self,
        allow_assumed_embedding_meta: bool,
    ) -> mneme::knowledge_store::KnowledgeConfig {
        mneme::knowledge_store::KnowledgeConfig {
            dim: self.dim,
            embedding_model: self.embedding_model.clone(),
            allow_assumed_embedding_meta,
            ..Default::default()
        }
    }
}

/// Derive the `KnowledgeConfig` fields from an already-loaded instance
/// config, or `None` to apply `KnowledgeConfig::default()`'s fields.
///
/// Takes the loaded config directly (rather than an `Oikos` to load it from)
/// so a caller that already has an `Option<&AletheiaConfig>` for another
/// reason — e.g. `import_knowledge`'s network-fetch gate (#4741) — never
/// loads it twice.
pub fn derive_knowledge_config(
    loaded: Option<&taxis::config::AletheiaConfig>,
) -> DerivedKnowledgeConfig {
    loaded.map_or_else(
        || {
            let default = mneme::knowledge_store::KnowledgeConfig::default();
            DerivedKnowledgeConfig {
                dim: default.dim,
                embedding_model: default.embedding_model,
            }
        },
        |config| {
            let embedding = config.embedding.to_embedding_config();
            DerivedKnowledgeConfig {
                dim: config.embedding.dimension,
                embedding_model: embedding.effective_model_name(),
            }
        },
    )
}

/// Derive the `KnowledgeConfig` used to open an agent's shared knowledge
/// store from an already-loaded instance config, or `None` to apply
/// `KnowledgeConfig::default()`. Convenience wrapper over
/// [`derive_knowledge_config`] for the common case of opening exactly one
/// store with a fixed `allow_assumed_embedding_meta`.
pub fn knowledge_config_from_loaded(
    loaded: Option<&taxis::config::AletheiaConfig>,
    allow_assumed_embedding_meta: bool,
) -> mneme::knowledge_store::KnowledgeConfig {
    derive_knowledge_config(loaded).to_knowledge_config(allow_assumed_embedding_meta)
}
