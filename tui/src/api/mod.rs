pub mod client;
pub(crate) mod error;
pub mod sse;
pub mod streaming;
pub mod types;

#[expect(
    unused_imports,
    reason = "re-exports for crate-level access; modules import submodules directly"
)]
pub use client::ApiClient;
#[expect(
    unused_imports,
    reason = "re-exported so callers can name the error type via crate::api::ApiError"
)]
pub use error::ApiError;
#[expect(
    unused_imports,
    reason = "re-exported for crate-level glob access via crate::api::*"
)]
pub use types::*;
