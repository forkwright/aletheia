//! Per-kind emitter arms.
//!
//! One module per chart kind. Each arm consumes the validated [`Chart`],
//! the [`ResolvedTheme`], the [`Canvas`] geometry, and the [`ColorMode`];
//! each arm emits its primitive group in a fixed source order, so the
//! whole-chart SVG is byte-deterministic.
//!
//! The `combo` arm is the only one with a working emitter today (the B-005
//! acceptance gate is a combo chart); the rest return
//! [`Error::EmitterStub`](crate::Error::EmitterStub). Per-kind designs are
//! fanned out as separate follow-up work — the design notes for each kind
//! live in this module's source comments and in the PR body.
//!
//! [`Chart`]: crate::model::Chart
//! [`ResolvedTheme`]: crate::theme::ResolvedTheme
//! [`Canvas`]: super::canvas::Canvas
//! [`ColorMode`]: crate::theme::ColorMode

/// Combo: columns + line on dual y-axes. The B-005 acceptance gate.
pub mod combo;
