use crate::rich_text::RichText;

/// A block-level element in the document tree.
#[derive(Debug, Clone, PartialEq)]
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

/// A data table.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Column header labels.
    pub headers: Vec<String>,
    /// Data rows. Each inner `Vec` must have the same length as `headers`.
    pub rows: Vec<Vec<RichText>>,
}

/// A single list item.
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// The item text, may be styled.
    pub content: RichText,
}

/// An embedded raster image.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// Raw image bytes (PNG, JPEG, etc.).
    pub data: Vec<u8>,
    /// MIME type, e.g. `"image/png"`.
    pub mime: String,
    /// Alt text for accessibility and plain-text fallback.
    pub alt: String,
}
