# L3 API Index: poiesis-sheet

Crate path: `crates/poiesis/sheet`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// The input JSON does not conform to the expected workbook schema.
    #[snafu(display("invalid schema: {detail}"))]
    InvalidSchema {
        /// Human-readable description of the schema violation.
        detail: String,
    },

    /// `rust_xlsxwriter` returned an error while writing the workbook.
    #[snafu(display("XLSX write error: {message}"))]
    XlsxWrite {
        /// Human-readable error description.
        message: String,
    },

    /// `calamine` returned an error while reading the workbook.
    #[snafu(display("XLSX read error: {message}"))]
    XlsxRead {
        /// Human-readable error description.
        message: String,
    },
}
```

## `src/lib.rs`

```rust
pub struct WorkbookSummary {
    /// Per-sheet summaries.
    pub sheets: Vec<SheetSummary>,
    /// Total number of non-empty cells across all sheets.
    pub cell_count: usize,
}
```

```rust
pub struct SheetSummary {
    /// Sheet name.
    pub name: String,
    /// Number of rows with data.
    pub row_count: usize,
    /// Number of columns with data.
    pub column_count: usize,
}
```

```rust
pub fn render_xlsx (data: &serde_json::Value) -> Result<Vec<u8>, Error>
```

```rust
pub fn inspect_xlsx (bytes: &[u8]) -> Result<WorkbookSummary, Error>
```

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
