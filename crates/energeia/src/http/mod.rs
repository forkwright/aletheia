//! HTTP/SSE dispatch engine module.
//!
//! Implements [`DispatchEngine`](crate::engine::DispatchEngine) targeting the
//! Anthropic Agent SDK. The current transport is a subprocess wrapper around
//! the Claude CLI (`claude --output-format stream-json`), parsing NDJSON from
//! stdout. The trait boundary insulates callers from this detail; when the
//! Agent SDK HTTP endpoints are publicly available, only this module changes.
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
