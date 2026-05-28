#![deny(missing_docs)]
//! poiesis-theme: theme registry, token model, and brand-asset sinks.
//!
//! Every poiesis render path consumes a [`ResolvedTheme`]; the `theme` crate is
//! the single home for the tokens that every backend reads from and for the
//! sinks that pre-bake brand assets for the three rendering families:
//!
//! - CSS custom properties for the HTML/PDF deck path (see [`sinks::css`]),
//! - OOXML `clrScheme` / `fontScheme` for the PPTX path (see [`sinks::ooxml`]),
//! - flat doc-vars map for the Pandoc document path (see [`sinks::docvars`]).
//!
//! Components reference tokens (e.g. `color.tone.positive`, `type.role.title`),
//! never literal hex or typeface strings. Swapping `theme: summus → ardent`
//! therefore restyles every component with zero spec edits. The
//! [`THEME/raw-color-literal`](lint::RAW_COLOR_LITERAL_RULE_ID),
//! [`THEME/raw-font-literal`](lint::RAW_FONT_LITERAL_RULE_ID), and
//! [`THEME/unknown-token`](lint::UNKNOWN_TOKEN_RULE_ID) rules mechanically
//! enforce that constraint at the spec boundary.
//!
//! ## Status
//!
//! This crate ships the locked surface from `planning/poiesis-evolution/B-002`:
//! [`ThemeId`], the [`Theme`] TOML shape, [`ResolvedTheme`], a discoverable
//! [`Registry`], the seed `summus` brand, the CSS sink, the OOXML `theme1.xml`
//! emitter, the doc-vars emitter, and the three `THEME/*` lint rule shapes.
//! Per-crate downstream wiring (a packed `<name>-base.pptx` for B-004, the
//! [B-001] `Renderer` parameter, the [B-008] lint-engine registration, and the
//! `theme` CLI verbs from [B-010]) is the next phase; the per-sink modules
//! document the explicit follow-up surface.
//!
//! [B-001]: https://github.com/forkwright/aletheia
//! [B-008]: https://github.com/forkwright/aletheia
//! [B-010]: https://github.com/forkwright/aletheia

/// Typed error surface for parse, registry, resolution, and sink failures.
pub mod error;
/// [`ThemeId`] newtype — the parse-don't-validate boundary for theme names.
pub mod id;
/// The `THEME/raw-color-literal`, `THEME/raw-font-literal`, and `THEME/unknown-token`
/// rule shapes consumed by the QA gate.
pub mod lint;
/// [`Registry`] of named themes, discovered from a `themes/` directory.
pub mod registry;
/// [`ResolvedTheme`] — tone references resolved to concrete role values.
pub mod resolved;
/// Brand-asset sinks: CSS custom properties, OOXML `theme1.xml`, Pandoc doc-vars.
pub mod sinks;
/// The TOML-shape [`Theme`] and the token model it carries.
pub mod tokens;

pub use error::ThemeError;
pub use id::ThemeId;
pub use registry::Registry;
pub use resolved::ResolvedTheme;
pub use tokens::Theme;
