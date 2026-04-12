use crate::block::Block;
use crate::metadata::Metadata;

/// A complete format-agnostic document ready for rendering.
///
/// `Document` is the single input type accepted by all [`Renderer`] impls.
/// It carries metadata (title, author, timestamps) and an ordered sequence of
/// block-level elements. Renderers traverse `content` top-to-bottom and map
/// each [`Block`] to the target format.
///
/// [`Renderer`]: crate::renderer::Renderer
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Document-level properties (title, author, creation time).
    pub metadata: Metadata,
    /// Ordered block-level content: headings, paragraphs, tables, lists, images, page breaks.
    pub content: Vec<Block>,
}

impl Document {
    /// Construct a document with a title and no content.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            metadata: Metadata::titled(title),
            content: Vec::new(),
        }
    }

    /// Append a block to the document.
    pub fn push(&mut self, block: Block) {
        self.content.push(block);
    }
}
