//! HTML/CSS deck renderer — three-layer CSS compositor and minijinja slide engine.

pub mod error;

mod css;
mod layout;
mod render;

use poiesis_core::bodies::Deck;
use poiesis_core::components::ComponentRegistry;
use poiesis_core::envelope::Meta;
use poiesis_core::scalar::AspectRatio;

pub use error::DeckError;
/// Re-export of the layout types that appear in [`DeckRenderer`]'s public fields.
pub use layout::{Canvas, SlideLayout, Zone, ZoneName};
// WHY exported rather than suppressed: folding `poiesis-deck-layout` in made
// `zone_to_emu` unreachable, since `mod layout` is private -- so dead-code
// analysis flagged it and the two helpers it calls. It is a working, documented
// OOXML conversion awaiting the PPTX backend named in `layout`'s module docs;
// publishing it keeps the capability available to that consumer, where an
// `expect(dead_code)` would only hide the fact that nothing can reach it.
pub use layout::zone_to_emu;

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
        let layout = layout::resolve_layout(*aspect);
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
