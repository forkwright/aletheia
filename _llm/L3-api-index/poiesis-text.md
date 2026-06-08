# L3 API Index: poiesis-text

Crate path: `crates/poiesis/text`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/odt.rs`

```rust
pub enum OdtError {
    /// A ZIP I/O error occurred while building the ODT package.
    #[snafu(display("ODT zip error: {message}"))]
    Zip {
        /// Human-readable ZIP error description.
        message: String,
    },
}
```

> Renders a [`Document`] to an ODT byte vector.
```rust
pub struct OdtRenderer;
```

```rust
impl OdtRenderer {
    pub fn new () -> Self;
}
```

## `src/pdf.rs`

```rust
pub enum PdfError {
    /// No usable system font was found. Install a font (e.g. `liberation-sans-fonts`
    /// or `google-noto-sans-fonts`) and verify it is readable.
    #[snafu(display("no usable system font found for PDF rendering"))]
    NoFont,

    /// `krilla` rejected the page dimensions. A4 constants are compile-time
    /// checked, so this variant is not reachable in practice.
    #[snafu(display("invalid page dimensions: {width}x{height}"))]
    InvalidPageSize {
        /// Attempted width in PDF points.
        width: f32,
        /// Attempted height in PDF points.
        height: f32,
    },

    /// `krilla` returned an error while producing the PDF.
    #[snafu(display("krilla PDF error: {message}"))]
    Krilla {
        /// Human-readable krilla error description.
        message: String,
    },
}
```

> Renders a [`Document`] to a PDF byte vector using the `krilla` crate.
> 
> Requires at least one OpenType/TrueType font to be available at the paths
> checked by [`PdfRenderer::font_data`]. On Fedora the `liberation-sans-fonts`
> or `google-noto-sans-fonts` package satisfies this requirement.
```rust
pub struct PdfRenderer {
    /// Raw font bytes. Loaded once at construction time.
    font_data: Vec<u8>,
}
```

```rust
impl PdfRenderer {
    pub fn new () -> Result<Self, PdfError>;
    pub fn with_font_data (data: Vec<u8>) -> Self;
}
```
