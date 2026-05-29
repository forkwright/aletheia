# L3 API Index: poiesis-deck

Crate path: `crates/poiesis/deck`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum DeckError {
    /// Component not found in registry.
    #[snafu(display("component '{id}' not found in registry"))]
    ComponentNotFound {
        /// The missing component id.
        id: String,
    },

    /// Failed to load template file.
    #[snafu(display("failed to load template for '{id}': {source}"))]
    TemplateLoad {
        /// The component id.
        id: String,
        /// The underlying IO error.
        source: std::io::Error,
    },

    /// Template render error.
    #[snafu(display("template render error for '{id}': {source}"))]
    TemplateRender {
        /// The component id.
        id: String,
        /// The underlying minijinja error.
        source: minijinja::Error,
    },
}
```

## `src/lib.rs`

```rust
pub struct DeckRenderer {
    /// Component registry providing templates and schemas.
    pub registry: ComponentRegistry,
    /// Resolved slide layout.
    pub layout: SlideLayout,
}
```

```rust
impl DeckRenderer {
    pub fn new (registry: ComponentRegistry, aspect: &AspectRatio) -> Self;
    pub fn render (&self, deck: &Deck, meta: &Meta) -> Result<String, DeckError>;
}
```
