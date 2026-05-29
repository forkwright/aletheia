//! Slide layout solver for deck rendering — shared by HTML/CSS and PPTX backends.

pub mod css;
pub mod emu;
pub mod solver;
pub mod zone;

pub use css::zone_to_css;
pub use emu::zone_to_emu;
pub use solver::resolve_layout;
pub use zone::{Canvas, SlideLayout, Zone, ZoneName};
