use crate::rich_text::RichText;

/// A block-level element in the document tree.
// kanon:ignore RUST/non-exhaustive-enum — public enum is part of stable crate API; exhaustive matching is intentionally supported
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
    /// A typed admonition block with a semantic kind and rich-text body.
    Note(Note),
    /// A block-level display math expression.
    DisplayMath(String),
    /// A raw block payload with a format tag and content string.
    RawBlock {
        /// Raw content format, such as `latex` or `html`.
        format: String,
        /// Raw content payload.
        content: String,
    },
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

/// A typed admonition block.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// The semantic admonition kind.
    pub kind: NoteKind,
    /// The note body as rich text.
    pub body: RichText,
}

/// Semantic kind for a [`Note`] block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NoteKind {
    /// A plain note.
    Note,
    /// A caution or warning.
    Warning,
    /// A helpful tip.
    Tip,
    /// A high-priority note.
    Important,
}

impl NoteKind {
    /// Lowercase tag used in serialised output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Warning => "warning",
            Self::Tip => "tip",
            Self::Important => "important",
        }
    }

    /// Human-readable label for plain-text fallback.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Warning => "Warning",
            Self::Tip => "Tip",
            Self::Important => "Important",
        }
    }
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
