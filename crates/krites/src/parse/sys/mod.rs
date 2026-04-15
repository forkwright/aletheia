//! System command parsing.
//!
//! Handles `:compact`, `:explain`, relation/index management, triggers,
//! access levels, and all three index types (standard, FTS, HNSW, LSH).

use compact_str::CompactString;
use ordered_float::OrderedFloat;

use crate::data::program::InputProgram;
use crate::data::relation::VecElementType;
use crate::data::symb::Symbol;
use crate::fts::TokenizerConfig;
use crate::runtime::relation::AccessLevel;

/// A parsed system command (prefixed with `::` in Datalog source).
#[derive(Debug)]
#[non_exhaustive]
pub enum SysOp {
    /// Trigger storage compaction.
    Compact,
    /// List columns of a relation.
    ListColumns(Symbol),
    /// List indices on a relation.
    ListIndices(Symbol),
    /// List all relations.
    ListRelations,
    /// List running queries.
    ListRunning,
    /// List registered fixed rules.
    ListFixedRules,
    /// Kill a running query by process ID.
    KillRunning(u64),
    /// Explain the query plan for a program.
    Explain(Box<InputProgram>),
    /// Remove one or more relations.
    RemoveRelation(Vec<Symbol>),
    /// Rename relations: `(old_name, new_name)` pairs.
    RenameRelation(Vec<(Symbol, Symbol)>),
    /// Show triggers on a relation.
    ShowTrigger(Symbol),
    /// Set triggers on a relation: `(put_scripts, rm_scripts, replace_scripts)`.
    SetTriggers(Symbol, Vec<String>, Vec<String>, Vec<String>),
    /// Set access level on one or more relations.
    SetAccessLevel(Vec<Symbol>, AccessLevel),
    /// Create a standard (B-tree) index.
    CreateIndex(Symbol, Symbol, Vec<Symbol>),
    /// Create an HNSW vector similarity index.
    CreateVectorIndex(HnswIndexConfig),
    /// Create a full-text search index.
    CreateFtsIndex(FtsIndexConfig),
    /// Create a MinHash LSH index.
    CreateMinHashLshIndex(MinHashLshConfig),
    /// Remove an index.
    RemoveIndex(Symbol, Symbol),
    /// Set a description on a relation.
    DescribeRelation(Symbol, CompactString),
}

/// Configuration for a full-text search index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FtsIndexConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub extractor: String,
    pub tokenizer: TokenizerConfig,
    pub filters: Vec<TokenizerConfig>,
}

/// Configuration for a MinHash LSH (locality-sensitive hashing) index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MinHashLshConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub extractor: String,
    pub tokenizer: TokenizerConfig,
    pub filters: Vec<TokenizerConfig>,
    pub n_gram: usize,
    pub n_perm: usize,
    pub false_positive_weight: OrderedFloat<f64>,
    pub false_negative_weight: OrderedFloat<f64>,
    pub target_threshold: OrderedFloat<f64>,
}

/// Configuration for an HNSW (Hierarchical Navigable Small World) vector index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HnswIndexConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub vec_dim: usize,
    pub dtype: VecElementType,
    pub vec_fields: Vec<CompactString>,
    pub distance: HnswDistance,
    pub ef_construction: usize,
    pub m_neighbours: usize,
    pub index_filter: Option<String>,
    pub extend_candidates: bool,
    pub keep_pruned_connections: bool,
}

/// Distance metric for HNSW vector search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum HnswDistance {
    /// Euclidean (L2) distance.
    L2,
    /// Inner product distance.
    InnerProduct,
    /// Cosine distance.
    Cosine,
}

mod index;
mod parse;

pub(crate) use parse::parse_sys;
