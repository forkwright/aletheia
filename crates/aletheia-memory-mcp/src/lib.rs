#![deny(missing_docs)]
//! aletheia-memory-mcp: standalone stdio MCP server for Aletheia's memory layer.
//!
//! External agents (Claude Code, Cursor, `OpenHands`, etc.) spawn this binary to
//! query the nous local knowledge graph directly over stdio JSON-RPC. It exposes
//! Aletheia's session-scoped knowledge-store surface: search, graph traversal,
//! topic enumeration, health stats, and token-gated writes.
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
//! - `nous_search` — BM25 text search over current facts.
//! - `nous_neighbors` — one-hop graph traversal from a fact's entities.
//! - `nous_list_topics` — enumerate fact-type buckets with counts.
//! - `nous_stats` — fact count, topic count, schema version, opaque store id.
//! - `nous_annotate` — token-gated annotation linked to a target fact.
//! - `nous_supersede` — token-gated supersession marker.
//! - `nous_forget` — token-gated soft deletion.
//!
//! This surface is distinct from kanon mnemosyne: it serves Aletheia nous local
//! knowledge with session-scoped semantics, not kanon's durable corpus store.
//! Read-tool recall scope is bound to the single `ALETHEIA_MEMORY_MCP_NOUS_ID`
//! identity configured at server startup; the model cannot supply a different
//! `nous_id` to access sibling memory.
//!
//! Write tools are registered only when `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` is set
//! at server startup; the capability token is never accepted as a tool argument.
//! Full local store paths are redacted from `nous_stats` unless
//! `ALETHEIA_MEMORY_MCP_ADMIN_DIAGNOSTICS` and the write token are both
//! configured, and the caller explicitly requests the path.
//!
//! # Feature gating
//!
//! Requires the `storage-fjall` feature on `mneme` (enabled by default here)
//! so the server can open an on-disk knowledge store at the oikos
//! `data/knowledge.fjall` path.

pub mod error;
pub mod server;
pub mod tools;
