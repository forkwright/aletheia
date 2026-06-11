//! Chromium CDP wrapper for HTML-to-PDF conversion.
//!
//! # Soft dependency
//!
//! The `chromium` feature (enabled by default) requires a system Chromium browser.
//!
//! **Install:**
//! - Debian/Ubuntu: `apt install chromium`
//! - macOS: `brew install --cask chromium`
//!
//! **Override binary path:** set the `CHROMIUM_PATH` environment variable.

use std::time::Duration;

pub mod error;
pub use error::PrinterError;

/// Default deadline for an HTML-to-PDF print operation.
pub const DEFAULT_PRINT_TIMEOUT: Duration = Duration::from_secs(60);

/// PDF print options controlling paper size and margins.
#[derive(Debug, Clone)]
pub struct PrintOptions {
    /// Paper width in mm.
    pub paper_width_mm: f64,
    /// Paper height in mm.
    pub paper_height_mm: f64,
    /// Top margin in mm (0.0 = no margin).
    pub margin_top_mm: f64,
    /// Bottom margin in mm.
    pub margin_bottom_mm: f64,
    /// Left margin in mm.
    pub margin_left_mm: f64,
    /// Right margin in mm.
    pub margin_right_mm: f64,
    /// Page scale factor (1.0 = 100%).
    pub scale: f64,
    /// Overall deadline for browser launch, page setup, and PDF generation.
    pub timeout: Duration,
    /// Explicitly disable the Chromium sandbox.
    pub disable_sandbox: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self::widescreen_16_9()
    }
}

impl PrintOptions {
    /// 16:9 widescreen — 254 mm × 142.875 mm, no margins.
    #[must_use]
    pub fn widescreen_16_9() -> Self {
        Self {
            paper_width_mm: 254.0,
            paper_height_mm: 142.875,
            margin_top_mm: 0.0,
            margin_bottom_mm: 0.0,
            margin_left_mm: 0.0,
            margin_right_mm: 0.0,
            scale: 1.0,
            timeout: DEFAULT_PRINT_TIMEOUT,
            disable_sandbox: false,
        }
    }

    /// 4:3 standard — 254 mm × 190.5 mm, no margins.
    #[must_use]
    pub fn standard_4_3() -> Self {
        Self {
            paper_width_mm: 254.0,
            paper_height_mm: 190.5,
            margin_top_mm: 0.0,
            margin_bottom_mm: 0.0,
            margin_left_mm: 0.0,
            margin_right_mm: 0.0,
            scale: 1.0,
            timeout: DEFAULT_PRINT_TIMEOUT,
            disable_sandbox: false,
        }
    }

    /// Select options based on a `poiesis_core` aspect ratio.
    #[must_use]
    pub fn from_aspect(aspect: &poiesis_core::scalar::AspectRatio) -> Self {
        if aspect == &poiesis_core::scalar::AspectRatio::WIDESCREEN_16_9 {
            Self::widescreen_16_9()
        } else {
            Self::standard_4_3()
        }
    }
}

#[cfg(feature = "chromium")]
mod chromium_impl;

/// Convert an HTML string to PDF bytes using a headless Chromium browser.
///
/// # Errors
///
/// - [`PrinterError::ChromiumNotFound`] — no Chromium binary is available
/// - [`PrinterError::BrowserLaunch`] — CDP browser failed to start
/// - [`PrinterError::PageError`] — page navigation or content error
/// - [`PrinterError::PdfError`] — PDF generation failed
#[cfg(feature = "chromium")]
pub async fn print_to_pdf(html: &str, opts: &PrintOptions) -> Result<Vec<u8>, PrinterError> {
    chromium_impl::print_to_pdf_inner(html, opts).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_options_default_keeps_sandbox_enabled() {
        let opts = PrintOptions::default();

        assert!(!opts.disable_sandbox);
        assert_eq!(opts.timeout, DEFAULT_PRINT_TIMEOUT);
    }
}
