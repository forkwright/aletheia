# L3 API Index: poiesis-printer-chromium

Crate path: `crates/poiesis/printer-chromium`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum PrinterError {
    #[snafu(display("Chromium binary not found; install chromium or set CHROMIUM_PATH env var"))]
    ChromiumNotFound,
    #[snafu(display("Browser launch failed: {source}"))]
    BrowserLaunch {
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, |e| e)))]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("Page error: {source}"))]
    PageError {
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, |e| e)))]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("PDF generation failed: {source}"))]
    PdfError {
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, |e| e)))]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("{operation} timed out after {timeout_secs}s"))]
    Timeout {
        operation: &'static str,
        timeout_secs: u64,
    },
    #[snafu(display("Browser cleanup failed: {source}"))]
    Cleanup {
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, |e| e)))]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
```

## `src/lib.rs`

> Default deadline for an HTML-to-PDF print operation.
```rust
pub const DEFAULT_PRINT_TIMEOUT: Duration = Duration::from_secs(60);
```

```rust
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
```

```rust
impl PrintOptions {
    pub fn widescreen_16_9 () -> Self;
    pub fn standard_4_3 () -> Self;
    pub fn from_aspect (aspect: &poiesis_core::scalar::AspectRatio) -> Self;
}
```

```rust
pub async fn print_to_pdf (html: &str, opts: &PrintOptions) -> Result<Vec<u8>, PrinterError>
```
