//! aletheia-taxis — configuration cascade and path resolution
//!
//! Taxis (τάξις) — "arrangement, ordering." Resolves configuration and files
//! through the three-tier oikos hierarchy:
//! `nous/{id}/` (most specific) → `shared/` → `theke/` (least specific).
//!
//! # Key types
//!
//! - [`oikos::Oikos`] — instance path resolver; construct via [`oikos::Oikos::discover`]
//! - [`config::AletheiaConfig`] — root configuration struct (YAML / env cascaded)
//! - [`loader::load_config`] — loads config with the three-source cascade
//! - [`cascade::Tier`] — identifies which tier a resolved file came from
//! - [`cascade::discover`] — walks all tiers to collect files by subdirectory
//! - [`cascade::resolve`] — finds the most-specific instance of a named file
//!
//! Depends only on `aletheia-koina`.

pub mod cascade;
pub mod config;
pub mod error;
pub mod loader;
pub mod oikos;
