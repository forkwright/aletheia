# L3 API Index: poiesis-sheet

Crate path: `crates/poiesis/sheet`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/ods.rs`

```rust
pub enum OdsRendererError {
    /// `spreadsheet-ods` returned an error while writing the ODS file.
    #[snafu(display("ODS error: {message}"))]
    Ods {
        /// Human-readable error description.
        message: String,
    },
}
```

> Renders a [`Document`] to an ODS byte vector.
> 
> The document title becomes the first sheet name. Tables map directly to
> sheets. Non-table blocks are rendered as plain-text rows.
```rust
pub struct OdsRenderer;
```

```rust
impl OdsRenderer {
    pub fn new () -> Self;
}
```

## `src/xlsx.rs`

```rust
pub enum XlsxRendererError {
    /// `rust_xlsxwriter` returned an error.
    #[snafu(display("XLSX error: {message}"))]
    Xlsx {
        /// Human-readable error description.
        message: String,
    },
}
```

> Renders a [`Document`] to an Excel XLSX byte vector.
> 
> The document title becomes the first sheet name. Each [`Block::Table`]
> produces one worksheet. Non-table blocks are written as plain-text rows
> at the top of the current sheet.
```rust
pub struct XlsxRenderer;
```

```rust
impl XlsxRenderer {
    pub fn new () -> Self;
}
```
