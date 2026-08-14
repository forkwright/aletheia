//! Slide layout solver — normalized zone coordinates, pixel CSS, and OOXML
//! EMU output.
//!
//! `zone_to_css` backs the HTML/CSS renderer this crate implements.
//! `zone_to_emu` targets a PPTX backend; `poiesis-slides` (the current PPTX
//! renderer) does not consume it yet.

mod css;
mod emu;
mod solver;
mod zone;

pub use css::zone_to_css;
pub use emu::zone_to_emu;
pub use solver::resolve_layout;
pub use zone::{Canvas, SlideLayout, Zone, ZoneName};
