//! aletheia-taxis — configuration cascade and path resolution
//!
//! Taxis (τάξις) — "arrangement, ordering." Resolves configuration and files
//! through the oikos three-tier hierarchy: nous/{id}/ → shared/ → theke/.
//!
//! Depends only on `aletheia-koina`.

pub mod cascade;
pub mod config;
pub mod error;
pub mod loader;
pub mod oikos;
