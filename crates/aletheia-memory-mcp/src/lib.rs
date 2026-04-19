#![deny(missing_docs)]
//! aletheia-memory-mcp: standalone stdio MCP server for Aletheia's memory layer.
//!
//! External agents (Claude Code, Cursor, `OpenHands`, etc.) spawn this binary to
//! query the knowledge graph directly over stdio JSON-RPC. It exposes a
//! read-only subset of the memory API — search, graph traversal, topic
//! enumeration, and health stats — with no session, tool-execution, or write
//! surface.
//!
//! # Why a separate crate from diaporeia
//!
//! `diaporeia` is the in-process MCP server that bundles session management,
//! nous agent control, and memory into one authenticated HTTP/stdio surface.
//! It is meant for operator use against a running Aletheia instance.
//!
//! `aletheia-memory-mcp` is a leaf binary that opens the knowledge store
//! directly, without the rest of the runtime. It is scoped to the memory-as-
//! service use case: any agent that speaks MCP can treat Aletheia's KG as a
//! drop-in memory provider by spawning this binary.
//!
//! # Tools exposed
//!
//! - `memory_search` — BM25 text search over current facts.
//! - `memory_neighbors` — one-hop graph traversal from a fact's entities.
//! - `memory_list_topics` — enumerate fact-type buckets with counts.
//! - `memory_stats` — fact count, topic count, schema version, open path.
//!
//! All tools are read-only. Writes (annotate, supersede, forget) are deferred
//! behind an auth model review; see issue tracker for the follow-up.
//!
//! # Feature gating
//!
//! Requires the `storage-fjall` feature on `mneme` (enabled by default here)
//! so the server can open an on-disk knowledge store at the oikos
//! `data/knowledge.fjall` path.

pub mod error;
pub mod server;
pub mod tools;
