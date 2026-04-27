//! API-layer error type — re-exported from theatron-net.
//!
//! Extracted to `theatron_net::error` (W4 chalkeion plan). This module
//! is a thin re-export so existing `skene::api::error` paths continue
//! to work; consumers may switch to `theatron_net::error` directly at
//! any time.

pub use theatron_net::error::{
    ApiError, AuthSnafu, HttpSnafu, InvalidTokenSnafu, Result, ServerSnafu,
};
