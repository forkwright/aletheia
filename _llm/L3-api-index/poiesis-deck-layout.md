# L3 API Index: poiesis-deck-layout

Crate path: `crates/poiesis/deck-layout`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/css.rs`

```rust
pub fn zone_to_css (zone: &Zone, canvas: &Canvas) -> String
```

## `src/emu.rs`

```rust
pub fn zone_to_emu (zone: &Zone, canvas: &Canvas) -> (i64, i64, i64, i64)
```

## `src/solver.rs`

```rust
pub fn resolve_layout (aspect: &AspectRatio) -> SlideLayout
```

## `src/zone.rs`

```rust
pub struct Zone {
    /// Normalized x position.
    pub x: f64,
    /// Normalized y position.
    pub y: f64,
    /// Normalized width.
    pub w: f64,
    /// Normalized height.
    pub h: f64,
}
```

```rust
pub enum ZoneName {
    /// Full slide.
    Full,
    /// Top header area.
    Header,
    /// Sub-header below header.
    SubHeader,
    /// Main body area.
    Body,
    /// Content area.
    Content,
    /// Bottom footer.
    Footer,
    /// Left half split.
    LeftHalf,
    /// Right half split.
    RightHalf,
    /// Center KPI card.
    CenterKpi,
}
```

```rust
impl ZoneName {
    pub fn css_class (self) -> &'static str;
}
```

```rust
pub struct Canvas {
    /// Width in pixels.
    pub width_px: u32,
    /// Height in pixels.
    pub height_px: u32,
}
```

```rust
impl Canvas {
    pub fn from_aspect (aspect: &AspectRatio) -> Self;
}
```

```rust
pub struct SlideLayout {
    /// Canvas dimensions.
    pub canvas: Canvas,
    /// Named zones in display order.
    pub zones: BTreeMap<ZoneName, Zone>,
}
```
