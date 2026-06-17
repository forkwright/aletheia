//! Per-kind emitter arms.
//!
//! One module per chart kind. Each arm consumes the validated [`Chart`],
//! the [`ResolvedTheme`], the [`Canvas`] geometry, and the [`ColorMode`];
//! each arm emits its primitive group in a fixed source order, so the
//! whole-chart SVG is byte-deterministic.
//!
//! The pure-Rust render path is wired here for `bar`, `column`, `line`,
//! `area`, `combo`, `scatter`, `pie`, `doughnut`, and `stat`. The remaining
//! chart kinds continue to route to Vega-Lite behind `charts-vega`.
//!
//! [`Chart`]: crate::model::Chart
//! [`ResolvedTheme`]: crate::theme::ResolvedTheme
//! [`Canvas`]: super::canvas::Canvas
//! [`ColorMode`]: crate::theme::ColorMode

pub mod area;
pub mod bar;
pub mod column;
pub mod combo;
pub mod doughnut;
pub mod line;
pub mod pie;
pub mod scatter;
pub mod shared;
pub mod stat;
