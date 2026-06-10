//! HTTP request handlers.
//!
//! # Examples
//!
//! New handlers are async functions that take [`axum::extract::State`] and
//! return an Axum [`IntoResponse`](axum::response::IntoResponse):
//!
//! ```no_run
//! use axum::{Json, extract::State, response::IntoResponse};
//! use std::sync::Arc;
//!
//! struct AppState;
//!
//! async fn example(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
//!     Json("ok")
//! }
//! ```

/// Runtime configuration read/write.
pub mod config;
/// Domain event subscription and discovery.
pub mod events;
/// System health and readiness check.
pub mod health;
/// Meta-insights: agent performance, quality metrics, system journal.
pub mod insights;
/// Knowledge graph browsing and management.
pub mod knowledge;
/// Prometheus metrics exposition endpoint.
pub mod metrics;
/// Nous agent listing and status inspection.
pub mod nous;
/// Planning project verification endpoints.
pub(crate) mod planning;
/// Session lifecycle, history retrieval, and SSE message streaming.
pub mod sessions;
/// Workspace file-browser and git diff endpoints.
pub mod workspace;
