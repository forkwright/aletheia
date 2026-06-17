//! Theme-binding seam: tone tokens, color modes, contrast contract.
//!
//! The chart subsystem owns no tone palette; every tone reference in a
//! [`Chart`](crate::model::Chart) resolves against a [`ResolvedTheme`]
//! supplied by the caller. The `poiesis-theme` bridge is feature-gated
//! (`theme-bridge`), so this crate compiles and tests without a hard
//! theme-crate dependency edge.
//!
//! # Color modes
//!
//! - `themed` — fills emit as `var(--tone-N)`. The HTML deck has a CSS
//!   variable per palette slot, so the same SVG byte-stream recolors when
//!   the active theme switches.
//! - `resolved` — fills emit as literal hex (`#232E54`). PPTX bake and
//!   document figures must not depend on a CSS variable at run time.
//!
//! Geometry is identical between the two; only the fill attribute differs.

use crate::model::ToneRef;

/// Color mode for the SVG emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// Emit `var(--tone-N)` references. HTML target.
    Themed,
    /// Emit literal `#RRGGBB`. PPTX bake / document target.
    Resolved,
}

/// A resolved theme palette + minimal text style tokens for SVG emission.
///
/// The chart subsystem only consumes the palette + the `font-family` strings;
/// the broader theme tree (page layout, link colors, code blocks) belongs
/// to `poiesis-theme` and other backends.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ResolvedTheme {
    /// Ordered series palette. `ToneRef::Indexed(i)` resolves to
    /// `series[i]`; out-of-bounds is a parse-time error.
    pub series: Vec<Tone>,
    /// Named tones referenced by `ToneRef::Named(name)`.
    pub named: Vec<NamedTone>,
    /// Theme name (used for the `--tone-*` prefix in `Themed` mode).
    pub theme_name: String,
    /// Sans serif font family token.
    pub font_sans: String,
    /// Mono font family token.
    pub font_mono: String,
}

/// One palette slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tone {
    /// CSS variable name used in `Themed` mode (e.g. `series-0`).
    pub css_var: String,
    /// Resolved hex color (e.g. `#232E54`).
    pub hex: String,
}

/// A named (non-indexed) palette slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedTone {
    /// Tone name as referenced from a chart spec.
    pub name: String,
    /// Resolved hex color.
    pub hex: String,
}

impl ResolvedTheme {
    /// Convert a `poiesis-theme` resolved theme into the chart-local palette.
    #[cfg(feature = "theme-bridge")]
    #[must_use]
    pub fn from_poiesis_theme(t: &poiesis_theme::ResolvedTheme) -> Self {
        Self {
            series: t
                .chart
                .series
                .iter()
                .enumerate()
                .map(|(i, reference)| Tone {
                    css_var: format!("series-{i}"),
                    hex: t
                        .lookup_color(reference)
                        .map_or_else(|| "#000000".to_owned(), |color| color.as_str().to_owned()),
                })
                .collect(),
            named: Vec::new(),
            theme_name: t.id.as_str().to_owned(),
            font_sans: family_stack(t.lookup_family("sans"))
                .unwrap_or_else(|| "Inter, system-ui, sans-serif".to_owned()),
            font_mono: family_stack(t.lookup_family("mono"))
                .unwrap_or_else(|| "JetBrains Mono, monospace".to_owned()),
        }
    }

    /// Minimal `summus` theme stand-in.
    ///
    /// This mirrors the offsite-deck palette so the slide-3 golden can be
    /// exercised in unit tests without enabling the `theme-bridge` feature.
    ///
    /// Acceptance gate hook: the navy + teal pair below are the same colors
    /// the B-005 acceptance contract names (`#232E54`, `#318891`).
    #[must_use]
    pub fn summus_stub() -> Self {
        Self {
            series: vec![
                Tone {
                    css_var: "series-0".to_owned(),
                    hex: "#232E54".to_owned(),
                },
                Tone {
                    css_var: "series-1".to_owned(),
                    hex: "#318891".to_owned(),
                },
                Tone {
                    css_var: "series-2".to_owned(),
                    hex: "#A56A28".to_owned(),
                },
            ],
            named: Vec::new(),
            theme_name: "summus".to_owned(),
            font_sans: "Inter, system-ui, sans-serif".to_owned(),
            font_mono: "JetBrains Mono, monospace".to_owned(),
        }
    }

    /// Resolve a [`ToneRef`] to the fill string for the chosen color mode.
    ///
    /// Returns [`crate::Error::UnresolvedTone`] if the reference does not
    /// exist in this palette. Series index is forwarded into the error so
    /// callers can report the offending slot.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnresolvedTone`] when `tone` is
    /// [`ToneRef::Indexed`] beyond `series.len()` or [`ToneRef::Named`]
    /// without a matching named-tone entry.
    pub fn fill_for(
        &self,
        tone: &ToneRef,
        mode: ColorMode,
        series_index: usize,
    ) -> crate::Result<String> {
        match tone {
            ToneRef::Indexed(i) => {
                let slot = self
                    .series
                    .get(*i)
                    .ok_or_else(|| crate::Error::UnresolvedTone {
                        tone: format!("indexed({i})"),
                        series_index,
                    })?;
                Ok(match mode {
                    ColorMode::Themed => format!("var(--tone-{})", slot.css_var),
                    ColorMode::Resolved => slot.hex.clone(),
                })
            }
            ToneRef::Named(name) => {
                let slot = self.named.iter().find(|t| &t.name == name).ok_or_else(|| {
                    crate::Error::UnresolvedTone {
                        tone: format!("named({name})"),
                        series_index,
                    }
                })?;
                Ok(match mode {
                    ColorMode::Themed => format!("var(--tone-{name})"),
                    ColorMode::Resolved => slot.hex.clone(),
                })
            }
        }
    }

    /// Resolve a per-slice fill for pie/doughnut charts.
    ///
    /// The series' declared [`ToneRef`] seeds the slice palette; each
    /// subsequent slice cycles forward through the theme palette so charts
    /// with more slices than palette slots still render deterministically.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnresolvedTone`] when the series base tone
    /// does not resolve.
    pub fn fill_for_slice(
        &self,
        base: &ToneRef,
        mode: ColorMode,
        series_index: usize,
        slice_index: usize,
    ) -> crate::Result<String> {
        let base_idx = match base {
            ToneRef::Indexed(i) => self.series.get(*i).map_or_else(
                || {
                    Err(crate::Error::UnresolvedTone {
                        tone: format!("indexed({i})"),
                        series_index,
                    })
                },
                |_| Ok(*i),
            )?,
            ToneRef::Named(name) => self
                .named
                .iter()
                .position(|t| &t.name == name)
                .ok_or_else(|| crate::Error::UnresolvedTone {
                    tone: format!("named({name})"),
                    series_index,
                })?,
        };
        let palette_len = self.series.len().max(1);
        let cycled = (base_idx + slice_index) % palette_len;
        self.fill_for(&ToneRef::Indexed(cycled), mode, series_index)
    }
}

#[cfg(feature = "theme-bridge")]
fn family_stack(family: Option<&[String]>) -> Option<String> {
    family.map(|stack| stack.join(", "))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn summus_stub_has_navy_and_teal() {
        let t = ResolvedTheme::summus_stub();
        assert_eq!(t.series[0].hex, "#232E54");
        assert_eq!(t.series[1].hex, "#318891");
    }

    #[test]
    fn fill_for_themed_emits_css_var() {
        let t = ResolvedTheme::summus_stub();
        let f = t
            .fill_for(&ToneRef::Indexed(0), ColorMode::Themed, 0)
            .expect("indexed 0 resolves");
        assert_eq!(f, "var(--tone-series-0)");
    }

    #[test]
    fn fill_for_resolved_emits_hex() {
        let t = ResolvedTheme::summus_stub();
        let f = t
            .fill_for(&ToneRef::Indexed(1), ColorMode::Resolved, 0)
            .expect("indexed 1 resolves");
        assert_eq!(f, "#318891");
    }

    #[test]
    fn out_of_bounds_tone_index_errors() {
        let t = ResolvedTheme::summus_stub();
        let r = t.fill_for(&ToneRef::Indexed(99), ColorMode::Resolved, 4);
        assert!(matches!(r, Err(crate::Error::UnresolvedTone { .. })));
    }

    #[test]
    fn fill_for_slice_cycles_past_palette_end() {
        let t = ResolvedTheme::summus_stub();
        // Palette has 3 slots; 5 slices should cycle through 0..2 repeatedly.
        let mut last = String::new();
        for j in 0..5 {
            let f = t
                .fill_for_slice(&ToneRef::Indexed(0), ColorMode::Resolved, 0, j)
                .expect("slice resolves");
            assert!(!f.is_empty());
            if j % 3 == 0 {
                last = f;
            } else {
                assert_ne!(f, last, "cycle should advance");
            }
        }
    }

    #[test]
    fn fill_for_slice_honors_base_tone() {
        let t = ResolvedTheme::summus_stub();
        let base_0 = t
            .fill_for_slice(&ToneRef::Indexed(0), ColorMode::Resolved, 0, 0)
            .expect("base 0");
        let base_1 = t
            .fill_for_slice(&ToneRef::Indexed(1), ColorMode::Resolved, 0, 0)
            .expect("base 1");
        assert_ne!(base_0, base_1);
    }
}
