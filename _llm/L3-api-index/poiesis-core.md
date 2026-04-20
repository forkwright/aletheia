# L3 API Index: poiesis-core

Crate path: `crates/poiesis/core`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/block.rs`

```rust
pub enum Block {
    /// Section heading at a given depth (1 = h1, 6 = h6).
    Heading {
        /// Heading depth: 1 through 6 inclusive.
        level: u8,
        /// Heading text, may be styled.
        text: RichText,
    },
    /// A body paragraph of rich text.
    Paragraph(RichText),
    /// A data table with a header row and data rows.
    Table(Table),
    /// A bulleted or numbered list.
    List {
        /// `true` for numbered list, `false` for bulleted.
        ordered: bool,
        /// The list items in display order.
        items: Vec<ListItem>,
    },
    /// An embedded image.
    Image(Image),
    /// A forced page break for paginated formats (PDF, ODT, PPTX).
    /// Ignored by spreadsheet backends.
    PageBreak,
}
```

```rust
pub struct Table {
    /// Column header labels.
    pub headers: Vec<String>,
    /// Data rows. Each inner `Vec` must have the same length as `headers`.
    pub rows: Vec<Vec<RichText>>,
}
```

```rust
pub struct ListItem {
    /// The item text, may be styled.
    pub content: RichText,
}
```

```rust
pub struct Image {
    /// Raw image bytes (PNG, JPEG, etc.).
    pub data: Vec<u8>,
    /// MIME type, e.g. `"image/png"`.
    pub mime: String,
    /// Alt text for accessibility and plain-text fallback.
    pub alt: String,
}
```

## `src/document.rs`

```rust
pub struct Document {
    /// Document-level properties (title, author, creation time).
    pub metadata: Metadata,
    /// Ordered block-level content: headings, paragraphs, tables, lists, images, page breaks.
    pub content: Vec<Block>,
}
```

```rust
impl Document {
    pub fn new (title: impl Into<String>) -> Self;
    pub fn push (&mut self, block: Block);
}
```

## `src/metadata.rs`

```rust
pub struct Metadata {
    /// Document title shown in viewer toolbars and export filenames.
    pub title: String,
    /// Optional author name embedded in document properties.
    pub author: Option<String>,
    /// Optional creation timestamp embedded in document properties.
    pub created: Option<Timestamp>,
}
```

```rust
impl Metadata {
    pub fn titled (title: impl Into<String>) -> Self;
}
```

## `src/renderer.rs`

> A format backend that renders a [`Document`] into raw bytes.
> 
> Implementors produce a self-contained byte payload for their target format
> (e.g. PDF, ODT, XLSX). The caller writes those bytes to disk or streams
> them over the network.
```rust
pub trait Renderer {
    fn render (&self, doc: &Document) -> Result<Vec<u8>, Self::Error>;
    fn format (&self) -> &'static str;
}
```

## `src/rich_text.rs`

```rust
pub struct RichText {
    /// Ordered list of styled inline spans.
    pub spans: Vec<Span>,
}
```

```rust
impl RichText {
    pub fn plain_text (&self) -> String;
}
```

```rust
pub enum Span {
    /// Unstyled text.
    Plain(String),
    /// Bold text.
    Bold(String),
    /// Italic text.
    Italic(String),
    /// Inline code with monospace rendering.
    Code(String),
    /// Hyperlink with visible label and target URL.
    Link {
        /// Display text shown to the reader.
        text: String,
        /// Destination URL.
        url: String,
    },
}
```
