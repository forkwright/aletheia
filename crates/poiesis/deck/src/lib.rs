//! HTML/CSS deck renderer — three-layer CSS compositor and minijinja slide engine.

pub mod error;

mod css;
mod render;

use poiesis_core::bodies::Deck;
use poiesis_core::components::ComponentRegistry;
use poiesis_core::envelope::Meta;
use poiesis_core::scalar::AspectRatio;
use poiesis_deck_layout::SlideLayout;

pub use error::DeckError;

/// The deck renderer.
#[derive(Debug, Clone)]
pub struct DeckRenderer {
    /// Component registry providing templates and schemas.
    pub registry: ComponentRegistry,
    /// Resolved slide layout.
    pub layout: SlideLayout,
}

impl DeckRenderer {
    /// Create a new renderer from a registry and aspect ratio.
    #[must_use]
    pub fn new(registry: ComponentRegistry, aspect: &AspectRatio) -> Self {
        let layout = poiesis_deck_layout::resolve_layout(aspect);
        Self { registry, layout }
    }

    /// Render a deck to a standalone HTML string.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError`] if a component is missing, a template fails to load,
    /// or minijinja reports a render error.
    pub fn render(&self, deck: &Deck, meta: &Meta) -> Result<String, DeckError> {
        render::render_deck(&self.registry, &self.layout, deck, meta)
    }
}
