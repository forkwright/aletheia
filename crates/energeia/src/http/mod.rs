//! HTTP/SSE dispatch engine module.
//!
//! WHY the name doesn't match the transport: [`HttpEngine`] is a subprocess
//! wrapper around the Claude CLI (`claude --output-format stream-json`),
//! parsing NDJSON from stdout — not an HTTP/SSE client. There is no
//! Anthropic-hosted "Agent SDK" HTTP/SSE endpoint to migrate to: the Claude
//! Agent SDK is Claude Code packaged as a library, itself a locally-run
//! harness, not a server product (see `crate::agent_sdk` for the full
//! reasoning). The [`DispatchEngine`](crate::engine::DispatchEngine) trait
//! boundary still insulates callers from this detail, so a genuinely
//! different transport — should one exist someday — would still only
//! change this module.
//!
//! # Module layout
//!
//! - [`client`] — `HttpEngine` implementing `DispatchEngine`
//! - [`session`] — `ProcessSessionHandle` implementing `SessionHandle`
//! - [`stream`] — NDJSON wire types and event stream parser
//! - [`mock`] — `MockEngine` for tests

mod client;
pub mod mock;
pub(crate) mod session;
pub(crate) mod stream;

pub use client::HttpEngine;
pub use mock::{MockEngine, MockOutcome};
