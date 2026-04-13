use jiff::Timestamp;

/// Document-level metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    /// Document title shown in viewer toolbars and export filenames.
    pub title: String,
    /// Optional author name embedded in document properties.
    pub author: Option<String>,
    /// Optional creation timestamp embedded in document properties.
    pub created: Option<Timestamp>,
}

impl Metadata {
    /// Construct minimal metadata with only a title.
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            author: None,
            created: None,
        }
    }
}
