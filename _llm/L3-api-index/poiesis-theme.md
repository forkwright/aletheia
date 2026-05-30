# L3 API Index: poiesis-theme

Crate path: `crates/poiesis/theme`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum ThemeError {
    /// A theme name failed [`ThemeId`](crate::ThemeId) parsing.
    #[snafu(display("invalid theme id {candidate:?}: {source}"))]
    InvalidId {
        /// The string presented at the boundary.
        candidate: String,
        /// The parse failure reason.
        source: InvalidThemeId,
    },

    /// A `themes/<name>.toml` file could not be read.
    #[snafu(display("failed to read theme file {path}"))]
    ReadTheme {
        /// The path that failed to read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// A `themes/<name>.toml` file failed TOML parsing.
    #[snafu(display("failed to parse theme TOML at {path}"))]
    ParseToml {
        /// The path that failed to parse.
        path: String,
        /// Underlying toml deserialization error.
        source: toml::de::Error,
    },

    /// A `themes/` discovery directory could not be enumerated.
    #[snafu(display("failed to enumerate themes directory {path}"))]
    Discovery {
        /// The directory that failed enumeration.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// The on-disk filename did not match the parsed `[meta].id`.
    #[snafu(display(
        "theme file {path:?} declares id {declared:?}, expected {expected:?} from filename"
    ))]
    IdMismatch {
        /// Source file path on disk.
        path: PathBuf,
        /// `id` field declared in the TOML.
        declared: String,
        /// Id derived from the filename stem.
        expected: String,
    },

    /// A token reference pointed at a node the theme does not define.
    ///
    /// This is the runtime sibling of the [`crate::lint::UnknownTokenRule`]
    /// shape: the lint rule rejects unknown tokens at the spec boundary; this
    /// variant catches the same condition during resolution if a renderer
    /// somehow asks for a token that survived the spec gate.
    #[snafu(display(
        "theme {theme_id} does not define token {reference}; available tokens in this namespace: {available}"
    ))]
    UnknownToken {
        /// The theme that was queried.
        theme_id: String,
        /// The token reference that missed.
        reference: String,
        /// Comma-joined list of tokens defined in the same namespace.
        available: String,
    },

    /// A `[color.tone]` entry pointed at a role that does not exist.
    #[snafu(display(
        "tone {tone_name:?} references unknown color role {role:?} in theme {theme_id}"
    ))]
    UnknownRole {
        /// The theme that was being resolved.
        theme_id: String,
        /// The tone whose target failed lookup.
        tone_name: String,
        /// The missing role name.
        role: String,
    },

    /// The registry was asked for a theme it does not carry.
    #[snafu(display("theme {theme_id} not found in registry; available: {available}"))]
    NotFound {
        /// The theme that was requested.
        theme_id: String,
        /// Comma-joined list of registry entries.
        available: String,
    },

    /// A sink could not write to the supplied buffer.
    #[snafu(display("sink {sink:?} failed to emit"))]
    Sink {
        /// The sink that failed (e.g. `"css"`, `"ooxml"`).
        sink: String,
        /// Underlying formatter error.
        source: std::fmt::Error,
    },

    /// A ZIP-based sink (PPTX, DOCX, ODT) failed while writing an entry.
    #[snafu(display("zip sink {sink:?} failed while writing entry {entry:?}: {message}"))]
    ZipWrite {
        /// The sink that failed (e.g. `"pptx"`, `"reference_docx"`).
        sink: String,
        /// The ZIP entry path that failed.
        entry: String,
        /// Error message from the zip writer.
        message: String,
    },
}
```

## `src/id.rs`

```rust
pub struct ThemeId(String);
```

```rust
impl ThemeId {
    pub fn parse (candidate: &str) -> Result<Self, InvalidThemeId>;
    pub fn as_str (&self) -> &str;
    pub fn into_inner (self) -> String;
}
```

```rust
pub enum InvalidThemeId {
    /// The candidate was empty.
    #[snafu(display("theme id is empty"))]
    Empty,
    /// The candidate length was outside [[`ThemeId::MIN_LEN`], [`ThemeId::MAX_LEN`]].
    #[snafu(display("theme id length {len} is outside [{min}, {max}]"))]
    Length {
        /// Length of the rejected candidate.
        len: usize,
        /// Minimum permitted length.
        min: usize,
        /// Maximum permitted length.
        max: usize,
    },
    /// The first character was not `[a-z]`.
    #[snafu(display("theme id must begin with [a-z]; found {found:?}"))]
    Leading {
        /// The disallowed leading character.
        found: char,
    },
    /// A character outside `[a-z0-9_-]` appeared after position 0.
    #[snafu(display("theme id may only contain [a-z0-9_-]; found {found:?}"))]
    Character {
        /// The disallowed character.
        found: char,
    },
}
```

## `src/lint.rs`

> Identifier for the `THEME/raw-color-literal` rule. The QA gate ([B-008])
> will register this rule against the basanos engine using this string.
```rust
pub const RAW_COLOR_LITERAL_RULE_ID: &str = "THEME/raw-color-literal";
```

> Identifier for the `THEME/raw-font-literal` rule.
```rust
pub const RAW_FONT_LITERAL_RULE_ID: &str = "THEME/raw-font-literal";
```

> Identifier for the `THEME/unknown-token` rule.
```rust
pub const UNKNOWN_TOKEN_RULE_ID: &str = "THEME/unknown-token";
```

```rust
pub struct Violation {
    /// The stable rule id (e.g. `"THEME/raw-color-literal"`).
    pub rule_id: String,
    /// JSON Pointer into the spec document.
    pub pointer: String,
    /// The offending substring as it appears in the spec.
    pub value: String,
    /// Human-readable diagnostic.
    pub message: String,
}
```

```rust
pub struct RawColorLiteralRule;
```

```rust
impl RawColorLiteralRule {
    pub fn scan (self, pointer: &str, value: &str) -> Vec<Violation>;
}
```

```rust
pub struct RawFontLiteralRule;
```

```rust
impl RawFontLiteralRule {
    pub fn scan (self, pointer: &str, value: &str) -> Vec<Violation>;
}
```

```rust
pub struct UnknownTokenRule<'theme> {
    /// The theme to check the reference against.
    pub theme: &'theme ResolvedTheme,
}
```

```rust
impl <'theme> UnknownTokenRule<'theme> {
    pub fn new (theme: &'theme ResolvedTheme) -> Self;
    pub fn check (&self, pointer: &str, token_ref: &str) -> Option<Violation>;
}
```

## `src/registry.rs`

```rust
pub struct Registry {
    themes: BTreeMap<ThemeId, Theme>,
}
```

```rust
impl Registry {
    pub fn new () -> Self;
    pub fn load_dir (dir: &Path) -> Result<Self, ThemeError>;
    pub fn insert (&mut self, theme: Theme);
    pub fn len (&self) -> usize;
    pub fn is_empty (&self) -> bool;
    pub fn ids (&self) -> Vec<&ThemeId>;
    pub fn get (&self, id: &ThemeId) -> Result<&Theme, ThemeError>;
    pub fn resolve (&self, id: &ThemeId) -> Result<ResolvedTheme, ThemeError>;
}
```

> Parse a candidate string into a [`ThemeId`], lifting the parse error into
> the crate's top-level [`ThemeError`].
> 
> WHY: thin delegation  -  callers at the registry boundary need a single
> error type; `ThemeId::parse` returns the narrower `InvalidThemeId`. This
> helper performs the one-line lift so the call sites don't all need to
> import [`InvalidIdSnafu`].
> 
> # Errors
> 
> Returns [`ThemeError::InvalidId`] when the candidate fails
> [`ThemeId::parse`]. The candidate is preserved verbatim on the error so a
> caller can echo it back in a diagnostic.
```rust
pub fn parse_theme_id (candidate: &str) -> Result<ThemeId, ThemeError>
```

## `src/resolved.rs`

```rust
pub struct ResolvedTheme {
    /// Stable identifier of the theme this resolution belongs to.
    pub id: ThemeId,
    /// Optional human label, carried through from `[meta].title`.
    pub title: Option<String>,
    /// Optional description, carried through from `[meta].description`.
    pub description: Option<String>,
    /// Brand colors, ordered. The map is the source of truth for tone / surface
    /// lookups elsewhere in the resolution.
    pub role: IndexMap<String, HexColor>,
    /// Tone → resolved hex value.
    pub tone: IndexMap<String, HexColor>,
    /// Surface → resolved hex value.
    pub surface: IndexMap<String, HexColor>,
    /// Typography (families, scale, roles) — carried through unchanged.
    pub r#type: TypeTokens,
    /// Spacing — carried through unchanged.
    pub space: SpaceTokens,
    /// Grid — carried through unchanged.
    pub grid: GridTokens,
    /// Table chrome — carried through unchanged.
    pub table: TableTokens,
    /// Chart palette — carried through unchanged.
    pub chart: ChartTokens,
}
```

```rust
impl ResolvedTheme {
    pub fn from_theme (theme: Theme) -> Result<Self, ThemeError>;
    pub fn lookup_color (&self, reference: &str) -> Option<&HexColor>;
    pub fn lookup_type_role (&self, name: &str) -> Option<&TypeRole>;
    pub fn lookup_scale (&self, name: &str) -> Option<u32>;
    pub fn lookup_family (&self, name: &str) -> Option<&[String]>;
}
```

## `src/sinks/css.rs`

> Emit the theme as CSS custom properties on `:root`.
> 
> Variable names follow the convention surfaced in B-002:
> 
> ```text
> --color-<role>   : #RRGGBB                       (one per [color.role])
> --tone-<name>    : var(--color-<role>) or #hex   (one per [color.tone])
> --surface-<name> : #RRGGBB                       (one per [color.surface])
> --type-<role>-size, --type-<role>-weight, ...     (one per [type.role] slot)
> --space-<name>   : <px>px                        (one per [space] slot)
> ```
> 
> The output is deterministic: every map is emitted in declaration order
> (preserved by [`indexmap::IndexMap`]); CSS values use the canonical
> uppercase `#RRGGBB` form; integer values carry no fractional digits. The
> same [`ResolvedTheme`] always produces byte-identical output so callers
> can fingerprint or diff brand assets without normalization.
> 
> # Errors
> 
> Returns [`ThemeError::Sink`] only if the underlying [`std::fmt::Write`]
> implementation fails. For `String` this is structurally unreachable; the
> variant exists for composition with non-allocating sinks.
```rust
pub fn emit_css (theme: &ResolvedTheme) -> Result<String, ThemeError>
```

## `src/sinks/docvars.rs`

> Emit the theme as a flat doc-vars JSON object, suitable for piping into
> Pandoc as `-M name=value` or for embedding in a `reference.docx`/`reference.odt`
> generation step.
> 
> Key naming follows the same convention as the CSS sink (`color.<role>`,
> `tone.<name>`, `surface.<name>`, `type.<role>.<slot>`, `space.<name>`,
> `grid.<slot>`, `table.<slot>`, `chart.series.<n>`), so a downstream
> template can reference the same logical name regardless of which sink it
> reads from.
> 
> The output is deterministic: every map is emitted in declaration order.
> Numeric values become JSON numbers (`64`, not `"64"`); colors become
> uppercased `#RRGGBB` strings; family stacks become arrays of strings.
> 
> # Errors
> 
> Returns [`ThemeError::Sink`] only if JSON serialization fails (structurally
> impossible for the value shapes this function produces  -  the variant
> exists for API symmetry with the other sinks).
```rust
pub fn emit_docvars_json (theme: &ResolvedTheme) -> Result<String, ThemeError>
```

> Emit the doc-vars map as a flat YAML metadata block. The shape matches the
> JSON sink; the format is what Pandoc's `--metadata-file` expects.
> 
> # Errors
> 
> Returns [`ThemeError::Sink`] if `std::fmt::Write` fails (unreachable for
> `String`).
```rust
pub fn emit_docvars_yaml (theme: &ResolvedTheme) -> Result<String, ThemeError>
```

## `src/sinks/ooxml.rs`

> Emit the OOXML `theme1.xml` body  -  the `<a:clrScheme>` + `<a:fontScheme>`
> that `PowerPoint` and `LibreOffice` read at file open to populate accent
> swatches and the theme font picker.
> 
> The schema slot mapping follows the convention every Office consumer
> expects (dk1=text/dark, lt1=background/light, dk2/lt2=secondary,
> accent1..6 = the brand palette in order):
> 
> | OOXML slot | Source token        | Why                              |
> |------------|---------------------|----------------------------------|
> | `dk1`      | `surface.ink`       | primary text color               |
> | `lt1`      | `surface.page`      | primary background               |
> | `dk2`      | `tone.neutral` role | brand-dark accent (B-002 names) |
> | `lt2`      | `surface.page_alt`  | brand-light fallback             |
> | `accent1`  | `tone.accent` role  | primary brand accent             |
> | `accent2`  | `tone.before` role  | secondary brand accent           |
> | `accent3`  | first `color.role`  | fallback to first role           |
> | `accent4..6` | next roles in order | populate from `color.role` order |
> 
> Defaults are conservative: if a slot's source token is absent, the slot
> emits the canonical Office default (black/white). Native `PowerPoint`
> chart series bind to `accent1..3`, so recoloring the theme recolors
> charts as B-002 names.
> 
> # Errors
> 
> Returns [`ThemeError::Sink`] only if the underlying [`std::fmt::Write`]
> fails. For `String` this is structurally unreachable.
```rust
pub fn emit_theme_xml (theme: &ResolvedTheme) -> Result<String, ThemeError>
```

## `src/sinks/pptx.rs`

> Emit a minimal, valid PPTX ZIP byte vector  -  a "base template" file  - 
> with the [`ResolvedTheme`]'s color and font scheme baked into
> `ppt/theme/theme1.xml`.
> 
> # Errors
> 
> Returns [`ThemeError::ZipWrite`] if any ZIP entry fails to write.
```rust
pub fn emit_base_pptx (theme: &ResolvedTheme) -> Result<Vec<u8>, ThemeError>
```

## `src/sinks/typst.rs`

> Emit the theme as Typst `#let` variable declarations.
> 
> Downstream Typst templates `#import` or `#include` this file to access
> brand colors, typography, spacing, grid, and chart palette.
> 
> Variable naming:
> 
> ```text
> color-<role>    rgb("#RRGGBB")          (one per [color.role])
> tone-<name>     rgb("#RRGGBB")          (one per [color.tone])
> surface-<name>  rgb("#RRGGBB")          (one per [color.surface])
> type-family-<name>  ("Geist", ...)      (one per [type.family])
> type-scale-<name>   <px>                (one per [type.scale])
> space-<name>    <px>                    (one per [space])
> grid-<slot>     <value>                 (one per present [grid] field)
> chart-series    (rgb("#..."), ...)      (resolved palette tuple)
> ```
> 
> The output is deterministic: every map is emitted in declaration order
> (preserved by [`indexmap::IndexMap`]); colors use the canonical uppercase
> `#RRGGBB` form; integer values carry no fractional digits. The same
> [`ResolvedTheme`] always produces byte-identical output.
> 
> # Errors
> 
> Returns [`ThemeError::Sink`] only if the underlying [`std::fmt::Write`]
> implementation fails. For `String` this is structurally unreachable; the
> variant exists for composition with non-allocating sinks.
```rust
pub fn emit_typst_template (theme: &ResolvedTheme) -> Result<String, ThemeError>
```

## `src/tokens.rs`

```rust
pub struct Theme {
    /// Theme-level metadata: id, optional human title and description.
    pub meta: Meta,
    /// `[color.*]` token namespace.
    pub color: ColorTokens,
    /// `[type.*]` token namespace.
    pub r#type: TypeTokens,
    /// `[space]` table.
    #[serde(default)]
    pub space: SpaceTokens,
    /// `[grid]` table.
    #[serde(default)]
    pub grid: GridTokens,
    /// `[table]` table.
    #[serde(default)]
    pub table: TableTokens,
    /// `[chart]` table.
    #[serde(default)]
    pub chart: ChartTokens,
}
```

```rust
pub struct Meta {
    /// Stable identifier (filesystem-safe, registry key).
    pub id: ThemeId,
    /// Human-readable label.
    #[serde(default)]
    pub title: Option<String>,
    /// One-line description of the brand or design intent.
    #[serde(default)]
    pub description: Option<String>,
}
```

```rust
pub struct ColorTokens {
    /// Named brand colors. Values are concrete [`HexColor`]s.
    #[serde(default)]
    pub role: IndexMap<String, HexColor>,
    /// Semantic tones. Each entry is a role *name*; resolution turns it into a hex.
    #[serde(default)]
    pub tone: IndexMap<String, String>,
    /// Surface slots. Each entry is a role *name*; resolution turns it into a hex.
    #[serde(default)]
    pub surface: IndexMap<String, String>,
}
```

```rust
pub struct TypeTokens {
    /// `[type.family]` — typeface family stacks. Each entry is a fallback list.
    #[serde(default)]
    pub family: IndexMap<String, Vec<String>>,
    /// `[type.scale]` — pixel sizes at `[grid].base_canvas`.
    #[serde(default)]
    pub scale: IndexMap<String, u32>,
    /// `[type.role]` — composite text roles (title, `hero_number`, eyebrow, …).
    #[serde(default)]
    pub role: IndexMap<String, TypeRole>,
}
```

```rust
pub struct TypeRole {
    /// Reference into [`TypeTokens::family`] (e.g. `"sans"`).
    #[serde(default)]
    pub family: Option<String>,
    /// Numeric weight (100..=900).
    #[serde(default)]
    pub weight: Option<u16>,
    /// Reference into [`TypeTokens::scale`] (e.g. `"title"`).
    #[serde(default)]
    pub size: Option<String>,
    /// Tracking in `em`.
    #[serde(default)]
    pub tracking: Option<f32>,
    /// Leading (line-height) as a unitless multiplier.
    #[serde(default)]
    pub leading: Option<f32>,
    /// Reference into [`ColorTokens::role`] / [`ColorTokens::tone`] /
    /// [`ColorTokens::surface`]; the resolver disambiguates.
    #[serde(default)]
    pub color: Option<String>,
}
```

```rust
pub struct SpaceTokens {
    /// Named spacing slots.
    #[serde(flatten, default)]
    pub slots: IndexMap<String, u32>,
}
```

```rust
pub struct GridTokens {
    /// `base_canvas = [width, height]` in pixels. Drives `type.scale` units.
    #[serde(default)]
    pub base_canvas: Option<[u32; 2]>,
    /// Aspect ratio token (e.g. `"16:9"`).
    #[serde(default)]
    pub aspect: Option<String>,
    /// Number of columns in the grid.
    #[serde(default)]
    pub columns: Option<u32>,
    /// Gutter width in canvas pixels.
    #[serde(default)]
    pub gutter: Option<u32>,
    /// Outer margin in canvas pixels.
    #[serde(default)]
    pub margin: Option<u32>,
}
```

```rust
pub struct TableTokens {
    /// Header fill (color reference).
    #[serde(default)]
    pub header_fill: Option<String>,
    /// Header ink (color reference).
    #[serde(default)]
    pub header_ink: Option<String>,
    /// Zebra-stripe fill (color reference).
    #[serde(default)]
    pub zebra: Option<String>,
    /// Border color (color reference).
    #[serde(default)]
    pub border: Option<String>,
    /// Whether to suppress vertical borders.
    #[serde(default)]
    pub no_vertical_borders: bool,
}
```

```rust
pub struct ChartTokens {
    /// Ordered series palette (color references).
    #[serde(default)]
    pub series: Vec<String>,
    /// Gridline color reference.
    #[serde(default)]
    pub gridline: Option<String>,
    /// Label color reference.
    #[serde(default)]
    pub label: Option<String>,
}
```

```rust
pub struct HexColor(String);
```

```rust
impl HexColor {
    pub fn parse (candidate: &str) -> Result<Self, InvalidHexColor>;
    pub fn as_str (&self) -> &str;
    pub fn body (&self) -> &str;
}
```

```rust
pub struct InvalidHexColor {
    /// The verbatim input that was rejected.
    pub candidate: String,
}
```
