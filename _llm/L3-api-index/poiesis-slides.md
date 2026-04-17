# L3 API Index: poiesis-slides

Crate path: `crates/poiesis/slides`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/pptx.rs`

```rust
pub enum PptxError {
    /// `ppt-rs` returned an error while generating the presentation.
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
