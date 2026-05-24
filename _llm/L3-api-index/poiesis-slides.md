# L3 API Index: poiesis-slides

Crate path: `crates/poiesis/slides`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// The supplied JSON does not match the expected slide schema.
    #[snafu(display("invalid slide JSON: {message}"))]
    InvalidJson {
        /// Human-readable description of the schema violation.
        message: String,
    },

    /// An error occurred while reading the PPTX ZIP archive.
    #[snafu(display("ZIP read error: {message}"))]
    ZipRead {
        /// Human-readable error description.
        message: String,
    },

    /// An error occurred while parsing OOXML XML inside the PPTX archive.
    #[snafu(display("XML parse error: {message}"))]
    XmlParse {
        /// Human-readable error description.
        message: String,
    },

    /// An error occurred during PPTX generation via the hand-rolled emitter.
    #[snafu(display("PPTX render error: {source}"))]
    Render {
        /// Underlying emitter error.
        source: crate::pptx::PptxError,
    },
}
```

## `src/lib.rs`

> Result alias for poiesis-slides operations.
```rust
pub type Result<T, E = Error> = std::result::Result<T, E>;
```

```rust
pub struct SlideSummary {
    /// Slide title (first text run in the slide, typically the title placeholder).
    pub title: String,
    /// Bullet text extracted from the slide body.
    pub bullets: Vec<String>,
}
```

```rust
pub struct PresentationSummary {
    /// Per-slide summaries, ordered by slide position.
    pub slides: Vec<SlideSummary>,
    /// Total number of slides.
    pub slide_count: usize,
}
```

```rust
pub fn render_pptx (data: &Value) -> Result<Vec<u8>>
```

```rust
pub fn inspect_pptx (bytes: &[u8]) -> Result<PresentationSummary>
```

## `src/pptx.rs`

```rust
pub enum PptxError {
    /// An error occurred while generating the presentation.
    #[snafu(display("PPTX error: {message}"))]
    Pptx {
        /// Human-readable error description.
        message: String,
    },
}
```

> Renders a [`Document`] to a PPTX byte vector.
> 
> Each top-level [`Block::Heading`] starts a new slide. Other blocks
> are appended to the current slide as bullet points.
```rust
pub struct PptxRenderer;
```

```rust
impl PptxRenderer {
    pub fn new () -> Self;
}
```
