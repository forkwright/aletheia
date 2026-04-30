//! API-layer error type — re-exported from `keryx`.
//!
//! The canonical type lives in `keryx::error`. This module is a thin
//! re-export so existing `skene::api::error` paths keep working;
//! consumers may switch to `keryx::error` directly at any time.

pub use keryx::error::{ApiError, AuthSnafu, HttpSnafu, InvalidTokenSnafu, Result, ServerSnafu};
