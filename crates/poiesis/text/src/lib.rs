#![deny(missing_docs)]
//! poiesis-text: PDF and ODT document rendering backends.
//!
//! Feature flags:
//! - `pdf` (default): PDF output via the `krilla` crate.
//! - `odt` (default): `OpenDocument` Text output via clean-room ZIP/XML.

#[cfg(feature = "pdf")]
pub mod pdf;

#[cfg(feature = "odt")]
pub mod odt;

#[cfg(feature = "pdf")]
pub use pdf::PdfRenderer;

#[cfg(feature = "odt")]
pub use odt::OdtRenderer;
