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

```rust
pub enum WorkbookError {
    /// A [`WorkbookCell::Cite`](crate::workbook::WorkbookCell) references a fact id not present in the resolved factbase.
    #[snafu(display("unknown fact id: {id}"))]
    UnknownFact {
        /// The fact id that was not found.
        id: String,
    },
    /// `rust_xlsxwriter` returned an error.
    #[snafu(display("XLSX write error: {message}"))]
    XlsxWrite {
        /// Human-readable error description.
        message: String,
    },
}
```

## `src/format.rs`

> Returns the data-cell format for a column given its kind, unit, and theme.
```rust
pub fn cell_format (kind: ScalarKind, unit: Unit, _theme: &ResolvedTheme) -> Format
```

> Returns the header-row format: bold + optional theme colours.
```rust
pub fn header_format (theme: &ResolvedTheme) -> Format
```

> Returns the totals-row format: same number format as [`cell_format`] but bold.
```rust
pub fn totals_format (kind: ScalarKind, unit: Unit, theme: &ResolvedTheme) -> Format
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

## `src/totals.rs`

> Compute per-column totals for a sheet.
> 
> Returns one `Option<Scalar>` per column header. Numeric columns
> (`Count`, `Money`, `Ratio`) are summed across all data rows; `Text` and
> `Date` columns always return `None`. Columns with no resolvable numeric
> values also return `None`.
```rust
pub fn compute_totals (
    sheet: &Sheet,
    facts: &BTreeMap<FactId, ResolvedFact>,
) -> Vec<Option<Scalar>>
```

## `src/workbook.rs`

> Renders a [`Workbook`] to an XLSX byte vector.
> 
> `facts` is the pre-resolved factbase (call `factbase.resolve(&registry)` before
> passing here). Every [`WorkbookCell::Cite`] must resolve in `facts`; an unknown
> fact id returns [`WorkbookError::UnknownFact`].
> 
> `theme` drives header formatting via [`crate::format::header_format`] and
> cell number formats via [`crate::format::cell_format`].
```rust
pub fn render_workbook (
    wb: &Workbook,
    facts: &BTreeMap<FactId, ResolvedFact>,
    theme: &ResolvedTheme,
) -> std::result::Result<Vec<u8>, crate::error::WorkbookError>
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
