use crate::document::Document;

/// A format backend that renders a [`Document`] into raw bytes.
///
/// Implementors produce a self-contained byte payload for their target format
/// (e.g. PDF, ODT, XLSX). The caller writes those bytes to disk or streams
/// them over the network.
pub trait Renderer {
    /// The error type returned on rendering failure.
    type Error: std::error::Error;

    /// Render `doc` into a complete byte payload for this format.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the backend cannot produce a valid output, for example
    /// because a required font resource is missing or a table row is malformed.
    fn render(&self, doc: &Document) -> Result<Vec<u8>, Self::Error>;

    /// Short lowercase identifier for the output format, e.g. `"pdf"`, `"odt"`.
    fn format(&self) -> &'static str;
}
