#![deny(missing_docs)]
//! poiesis-slides: PPTX presentation rendering backend.
//!
//! Feature flags:
//! - `pptx` (default): `PowerPoint` PPTX output via `ppt-rs`.

#[cfg(feature = "pptx")]
pub mod pptx;

#[cfg(feature = "pptx")]
pub use pptx::PptxRenderer;
