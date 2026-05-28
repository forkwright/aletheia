//! Vega-Lite shell-out emitter (feature `charts-vega`).
//!
//! Handles kinds the pure-Rust emitter does not own: `heatmap`, `boxplot`,
//! `sankey`, `candlestick`, plus any axis with `Scale::Log` or `Scale::Time`.
//!
//! # Strategy
//!
//! Subprocess shell-out to `npx -y vega-lite@<pin> vl2svg in.vl.json > out.svg`.
//! Embedding a JS engine (`deno_core`/`quickjs`) was rejected because it
//! bloats the binary and couples the deck-render critical path to a JS
//! runtime; the CLI shell-out is the isolation.
//!
//! # Determinism
//!
//! - `vega-lite` + `vega` versions are pinned; the pin is part of the
//!   golden-snapshot contract.
//! - The Vega config block is theme-derived (palette + fonts) so theme
//!   swaps recolor here the same way they do on the Rust path.
//!
//! # Current status
//!
//! Scaffold only. The shell-out wiring + the Vega spec compiler land in a
//! follow-up PR.

use crate::Result;
use crate::model::Chart;
use crate::render::canvas::Canvas;
use crate::theme::{ColorMode, ResolvedTheme};

/// Emit a Vega-Lite-rendered chart.
///
/// # Errors
///
/// Returns [`crate::Error::EmitterStub`] until the shell-out path lands.
pub fn emit(
    chart: &Chart,
    _theme: &ResolvedTheme,
    _canvas: &Canvas,
    _mode: ColorMode,
) -> Result<String> {
    Err(crate::Error::EmitterStub {
        kind: chart.kind.name().to_owned(),
    })
}
