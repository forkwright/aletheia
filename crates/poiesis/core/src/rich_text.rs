/// A sequence of inline spans forming a run of styled text.
#[derive(Debug, Clone, PartialEq)]
pub struct RichText {
    /// Ordered list of styled inline spans.
    pub spans: Vec<Span>,
}

impl RichText {
    /// Concatenate all span text content into a single unstyled string.
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(Span::text).collect()
    }
}

impl From<&str> for RichText {
    fn from(s: &str) -> Self {
        Self {
            spans: vec![Span::Plain(s.to_owned())],
        }
    }
}

impl From<String> for RichText {
    fn from(s: String) -> Self {
        Self {
            spans: vec![Span::Plain(s)],
        }
    }
}

/// A single styled inline run of text.
#[derive(Debug, Clone, PartialEq)]
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

impl Span {
    /// The text content of the span, regardless of styling.
    pub fn text(&self) -> &str {
        match self {
            Self::Plain(s) | Self::Bold(s) | Self::Italic(s) | Self::Code(s) => s.as_str(),
            Self::Link { text, .. } => text.as_str(),
        }
    }
}
