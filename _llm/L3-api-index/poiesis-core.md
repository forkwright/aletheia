# L3 API Index: poiesis-core

Crate path: `crates/poiesis/core`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/block.rs`

```rust
pub enum Block {
    /// Section heading at a given depth (1 = h1, 6 = h6).
    Heading {
        /// Heading depth: 1 through 6 inclusive.
        level: u8,
        /// Heading text, may be styled.
        text: RichText,
    },
    /// A body paragraph of rich text.
    Paragraph(RichText),
    /// A typed admonition block with a semantic kind and rich-text body.
    Note(Note),
    /// A block-level display math expression.
    DisplayMath(String),
    /// A raw block payload with a format tag and content string.
    RawBlock {
        /// Raw content format, such as `latex` or `html`.
        format: String,
        /// Raw content payload.
        content: String,
    },
    /// A data table with a header row and data rows.
    Table(Table),
    /// A bulleted or numbered list.
    List {
        /// `true` for numbered list, `false` for bulleted.
        ordered: bool,
        /// The list items in display order.
        items: Vec<ListItem>,
    },
    /// An embedded image.
    Image(Image),
    /// A forced page break for paginated formats (PDF, ODT, PPTX).
    /// Ignored by spreadsheet backends.
    PageBreak,
}
```

```rust
pub struct Note {
    /// The semantic admonition kind.
    pub kind: NoteKind,
    /// The note body as rich text.
    pub body: RichText,
}
```

```rust
pub enum NoteKind {
    /// A plain note.
    Note,
    /// A caution or warning.
    Warning,
    /// A helpful tip.
    Tip,
    /// A high-priority note.
    Important,
}
```

```rust
impl NoteKind {
    pub fn as_str (self) -> &'static str;
    pub fn label (self) -> &'static str;
}
```

```rust
pub struct Table {
    /// Column header labels.
    pub headers: Vec<String>,
    /// Data rows. Each inner `Vec` must have the same length as `headers`.
    pub rows: Vec<Vec<RichText>>,
}
```

```rust
pub struct ListItem {
    /// The item text, may be styled.
    pub content: RichText,
}
```

```rust
pub struct Image {
    /// Raw image bytes (PNG, JPEG, etc.).
    pub data: Vec<u8>,
    /// MIME type, e.g. `"image/png"`.
    pub mime: String,
    /// Alt text for accessibility and plain-text fallback.
    pub alt: String,
}
```

## `src/bodies.rs`

```rust
pub struct Deck {
    /// The deck-wide aspect ratio (e.g. [`AspectRatio::WIDESCREEN_16_9`]).
    pub aspect: AspectRatio,
    /// Slides in display order.
    pub slides: Vec<Slide>,
}
```

```rust
pub struct Slide {
    /// The component pack this slide instantiates.
    pub component: ComponentId,
    /// The slide field payload; opaque to this crate aside from schema
    /// validation.
    pub fields: serde_json::Value,
    /// Optional speaker notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}
```

```rust
pub struct Workbook {
    /// Sheets in tab order.
    pub sheets: Vec<Sheet>,
}
```

```rust
pub struct Sheet {
    /// Sheet display name.
    pub name: SheetName,
    /// The header row.
    pub headers: Vec<String>,
    /// The data rows; each row must have `headers.len()` cells.
    pub rows: Vec<Vec<WorkbookCell>>,
    /// Per-column scalar kind; drives format-at-the-boundary in the
    /// rendering crate.
    pub column_types: Vec<ScalarKind>,
}
```

```rust
pub enum WorkbookCell {
    /// A typed literal value (typically `Scalar::Text` for labels).
    Lit {
        /// The literal.
        value: Scalar,
    },
    /// A reference into [`crate::factbase::Factbase`] for any numeric value.
    Cite {
        /// The cited fact id.
        fact: FactId,
    },
}
```

```rust
pub struct DocumentBody {
    /// The wrapped legacy document.
    pub document: Document,
}
```

```rust
impl DocumentBody {
    pub fn new (document: Document) -> Self;
}
```

## `src/components.rs`

```rust
pub struct TokenRef(pub String);
```

```rust
impl TokenRef {
    pub fn new (s: impl Into<String>) -> Self;
}
```

```rust
pub struct ComponentDef {
    /// The component id; matches the pack directory name.
    pub id: ComponentId,
    /// The JSON-schema document validating `Slide.fields` payloads.
    pub schema: Value,
    /// Default values merged into `Slide.fields` before validation.
    pub defaults: Value,
    /// Filesystem path of the HTML template (consumed by [[B-003]]).
    pub html: PathBuf,
    /// Filesystem path of the OOXML recipe (consumed by [[B-004]]).
    pub ooxml: PathBuf,
    /// Theme tokens this component reads.
    pub tokens: Vec<TokenRef>,
}
```

```rust
pub struct ComponentRegistry {
    by_id: BTreeMap<ComponentId, ComponentDef>,
}
```

```rust
impl ComponentRegistry {
    pub fn new () -> Self;
    pub fn insert (&mut self, def: ComponentDef);
    pub fn get (&self, id: &ComponentId) -> Option<&ComponentDef>;
    pub fn iter (&self) -> impl Iterator<Item = &ComponentDef>;
    pub fn list_components (&self) -> Vec<ComponentId>;
    pub fn len (&self) -> usize;
    pub fn is_empty (&self) -> bool;
    pub fn discover (&mut self, root: &Path) -> Result<usize, RegistryError>;
    pub fn validate_fields (
        &self,
        id: &ComponentId,
        fields: &Value,
    ) -> Result<Value, RegistryError>;
}
```

## `src/document.rs`

```rust
pub struct Document {
    /// Document-level properties (title, author, creation time).
    pub metadata: Metadata,
    /// Ordered block-level content: headings, paragraphs, notes, tables,
    /// lists, images, display math, raw blocks, and page breaks.
    pub content: Vec<Block>,
}
```

```rust
impl Document {
    pub fn new (title: impl Into<String>) -> Self;
    pub fn push (&mut self, block: Block);
}
```

## `src/envelope.rs`

```rust
pub struct Meta {
    /// Deliverable title; required.
    pub title: String,
    /// Optional author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Optional creation timestamp; renderers may stamp document properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Timestamp>,
    /// Optional subject; threaded into PDF/DOCX/PPTX core properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Optional list of keywords / tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
}
```

```rust
impl Meta {
    pub fn new (title: impl Into<String>) -> Result<Self, SpecError>;
    pub fn validate (&self) -> Result<(), SpecError>;
}
```

```rust
pub enum Body {
    /// A deck of slides.
    Deck(Deck),
    /// A prose document (wraps the pre-envelope
    /// [`crate::document::Document`]).
    Document(DocumentBodyRepr),
    /// A workbook of sheets.
    Workbook(Workbook),
}
```

```rust
pub struct DocumentBodyRepr {
    /// The document title — duplicated from [`Meta`] for self-containment
    /// at the body level (renderers that only see `Body` still have a
    /// title to thread through).
    pub title: String,
}
```

```rust
impl Body {
    pub fn kind (&self) -> BodyKind;
}
```

```rust
pub struct DeliverableSpec {
    /// Typed metadata.
    pub meta: Meta,
    /// Theme reference; resolved against the theme registry ([[B-002]]).
    pub theme: ThemeId,
    /// The factbase carrying every cited number.
    pub facts: Factbase,
    /// The body — deck, document, or workbook.
    pub body: Body,
}
```

```rust
impl DeliverableSpec {
    pub fn validate (
        &self,
        components: &ComponentRegistry,
        known_themes: &[ThemeId],
    ) -> Result<(), crate::error::PoiesisError>;
    pub fn body_kind (&self) -> BodyKind;
    pub fn referenced_components (&self) -> Vec<ComponentId>;
}
```

## `src/error.rs`

```rust
pub enum IdError {
    /// The input string was empty.
    #[snafu(display("{kind} identifier cannot be empty"))]
    Empty {
        /// Human-readable identifier kind, e.g. `"component"`.
        kind: &'static str,
    },
    /// The input contained a character that is not allowed for this kind.
    #[snafu(display(
        "{kind} identifier {input:?} contains invalid character {ch:?} (allowed: {allowed})"
    ))]
    InvalidChar {
        /// Human-readable identifier kind.
        kind: &'static str,
        /// The offending input string.
        input: String,
        /// The first invalid character encountered.
        ch: char,
        /// Description of the allowed character set.
        allowed: &'static str,
    },
    /// The input exceeded the maximum allowed length.
    #[snafu(display("{kind} identifier {input:?} length {got} exceeds maximum {max}"))]
    TooLong {
        /// Human-readable identifier kind.
        kind: &'static str,
        /// The offending input string.
        input: String,
        /// The actual length.
        got: usize,
        /// The maximum allowed length.
        max: usize,
    },
}
```

```rust
pub enum ScalarError {
    /// Unknown unit name.
    #[snafu(display("unknown unit {input:?} (known: count, usd, percent, ratio, date, text)"))]
    UnknownUnit {
        /// The offending input string.
        input: String,
    },
    /// Aspect ratio could not be parsed in `"W:H"` form.
    #[snafu(display("aspect ratio {input:?} is not in W:H form with positive integers"))]
    BadAspect {
        /// The offending input string.
        input: String,
    },
    /// Tolerance must be in the closed unit interval `[0.0, 1.0]`.
    #[snafu(display("tolerance {value} is outside [0.0, 1.0]"))]
    BadTolerance {
        /// The offending value.
        value: f64,
    },
    /// A monetary amount was outside the representable range.
    #[snafu(display("monetary amount {input:?} is malformed or exceeds representable range"))]
    BadMoney {
        /// The offending input string.
        input: String,
    },
}
```

```rust
pub enum FactbaseError {
    /// A `FactId` referenced from a `Claim` or `Derived`/`Reference` source
    /// is not present in the factbase.
    #[snafu(display("unknown fact reference {id:?} from {referenced_by}"))]
    UnknownFact {
        /// The unresolved fact identifier (as a string for error surfacing).
        id: String,
        /// What referenced it (claim id, derived source, etc.).
        referenced_by: String,
    },
    /// A cycle was detected in the `Derived`/`Reference` dependency graph.
    /// `path` is the cycle path in declaration order.
    #[snafu(display("cycle in factbase: {}", path.join(" -> ")))]
    Cycle {
        /// The fact ids forming the cycle, head repeated at the tail.
        path: Vec<String>,
    },
    /// A claim references a fact whose `Source` requires a data adapter that
    /// is not configured.
    #[snafu(display(
        "claim {claim_id:?} requires data source {data_source:?} but no adapter is configured"
    ))]
    MissingDataSource {
        /// The claim that triggered the error.
        claim_id: String,
        /// The data source id named by the claim's underlying fact.
        data_source: String,
    },
    /// A `Derived` expression named an arithmetic operator the evaluator does
    /// not implement.
    #[snafu(display("unsupported derived expression: {detail}"))]
    BadDerived {
        /// Free-form description of why the expression cannot be evaluated.
        detail: String,
    },
    /// A `Derived` expression's operand types are incompatible.
    #[snafu(display("type mismatch in derived expression: {detail}"))]
    DerivedTypeMismatch {
        /// Free-form description of the mismatch.
        detail: String,
    },
}
```

```rust
pub enum RegistryError {
    /// A pack directory was missing a required artifact.
    #[snafu(display("component pack {component:?} missing required file {file}"))]
    MissingPackFile {
        /// The component id whose pack is incomplete.
        component: String,
        /// The file path expected within the pack directory.
        file: String,
    },
    /// A pack's `schema.json` was unreadable or not valid JSON.
    #[snafu(display("component pack {component:?} has malformed schema.json: {detail}"))]
    MalformedSchema {
        /// The component id whose schema failed to parse.
        component: String,
        /// Parser-emitted detail.
        detail: String,
    },
    /// A pack's `recipe.toml` was unreadable or not valid TOML.
    #[snafu(display("component pack {component:?} has malformed recipe.toml: {detail}"))]
    MalformedRecipe {
        /// The component id whose recipe failed to parse.
        component: String,
        /// Parser-emitted detail.
        detail: String,
    },
    /// I/O error while discovering or reading a pack.
    #[snafu(display("component pack discovery I/O failure at {path}: {detail}"))]
    Io {
        /// The filesystem path where I/O failed.
        path: String,
        /// OS-emitted detail.
        detail: String,
    },
    /// A `Slide.fields` payload failed schema validation.
    /// `pointer` is a JSON-pointer (RFC 6901) naming the path inside the payload.
    #[snafu(display("slot validation failed at {pointer}: {detail}"))]
    SlotValidation {
        /// JSON-pointer into the offending payload.
        pointer: String,
        /// Free-form description of the rule that rejected the value.
        detail: String,
    },
    /// A `Slide` referenced a component id that is not registered.
    #[snafu(display("slide references unknown component {component:?}"))]
    UnknownComponent {
        /// The unresolved component id.
        component: String,
    },
}
```

```rust
pub enum SpecError {
    /// A required `Meta` field was missing.
    #[snafu(display("meta field {field:?} is required but missing"))]
    MissingMetaField {
        /// The field name.
        field: &'static str,
    },
    /// A `ThemeId` referenced by the envelope is not known to the theme
    /// registry. The theme registry itself lives in `poiesis-theme`; this
    /// error is surfaced when an envelope is checked against a registry that
    /// does not contain its theme.
    #[snafu(display("envelope references unknown theme {theme:?}"))]
    UnknownTheme {
        /// The unresolved theme id.
        theme: String,
    },
    /// The body kind in the spec did not match the body kind expected by the
    /// renderer or theme.
    #[snafu(display("body kind mismatch: spec carries {got}, expected {expected}"))]
    BodyKindMismatch {
        /// The body kind in the spec.
        got: &'static str,
        /// The body kind expected.
        expected: &'static str,
    },
}
```

```rust
pub enum PoiesisError {
    /// Identifier construction failed.
    #[snafu(display("{source}"))]
    Id {
        /// The wrapped identifier error.
        source: IdError,
    },
    /// Scalar/unit/aspect/tolerance parsing failed.
    #[snafu(display("{source}"))]
    Scalar {
        /// The wrapped scalar error.
        source: ScalarError,
    },
    /// Factbase resolution failed.
    #[snafu(display("{source}"))]
    Factbase {
        /// The wrapped factbase error.
        source: FactbaseError,
    },
    /// Component registry operation failed.
    #[snafu(display("{source}"))]
    Registry {
        /// The wrapped registry error.
        source: RegistryError,
    },
    /// Spec parsing or validation failed.
    #[snafu(display("{source}"))]
    Spec {
        /// The wrapped spec error.
        source: SpecError,
    },
}
```

## `src/factbase.rs`

```rust
pub struct Fact {
    /// Identifier referenced by `Cite`/`Reference`/`Derived`.
    pub id: FactId,
    /// The typed value. For `Sql`/`Derived` facts the value is the cached or
    /// computed result; for `Manual`/`File` it is the authored value.
    pub value: Scalar,
    /// Presentation unit (drives formatting and dimensional checks).
    pub unit: Unit,
    /// Where this fact comes from.
    pub source: Source,
    /// When the fact was last captured / asserted.
    pub captured: Timestamp,
}
```

```rust
pub enum Source {
    /// Resolved via the named data adapter (e.g. a CSV reader, SQL driver).
    Sql {
        /// Adapter id; must resolve in the configured [`DataSourceRegistry`].
        data_source: DataSourceId,
        /// The query (SQL string, or whatever the adapter accepts).
        query: String,
        /// A friendly name for the table/view used in error messages.
        table: String,
    },
    /// An arithmetic expression over other facts.
    Derived {
        /// The expression.
        formula: Expr,
        /// The fact ids consumed by `formula`, in dependency order.
        inputs: Vec<FactId>,
    },
    /// An alias for another fact.
    Reference {
        /// The aliased fact id.
        fact: FactId,
    },
    /// An operator-asserted value with no programmatic provenance.
    Manual {
        /// Free-form note.
        note: String,
        /// Who captured the value.
        captured_by: String,
    },
    /// A value extracted from a file at a locator.
    File {
        /// Filesystem path.
        path: PathBuf,
        /// In-file locator (CSV cell `A1`, JSON pointer `/totals/0`, etc.).
        locator: String,
    },
}
```

```rust
pub enum Expr {
    /// `a + b`.
    Add {
        /// First operand fact id.
        a: FactId,
        /// Second operand fact id.
        b: FactId,
    },
    /// `a - b`.
    Sub {
        /// Minuend fact id.
        a: FactId,
        /// Subtrahend fact id.
        b: FactId,
    },
    /// `a * b`.
    Mul {
        /// First operand fact id.
        a: FactId,
        /// Second operand fact id.
        b: FactId,
    },
    /// `a / b` evaluated as a ratio (`f64`).
    Div {
        /// Numerator fact id.
        a: FactId,
        /// Denominator fact id.
        b: FactId,
    },
    /// Sum over a list of facts (each must have a compatible unit).
    Sum {
        /// The fact ids to sum.
        terms: Vec<FactId>,
    },
}
```

```rust
pub struct Claim {
    /// Identifier of this claim.
    pub id: ClaimId,
    /// Human-readable form of the claim as it appears in prose.
    pub text: String,
    /// The fact asserted.
    pub asserts: FactId,
    /// Where in the deliverable the claim lives (slide, paragraph, sheet cell).
    pub location: Location,
    /// Numeric tolerance used by the QA gate when comparing fact vs. claim.
    #[serde(default = "default_strict_tolerance")]
    pub tolerance: Tolerance,
}
```

```rust
pub struct Location {
    /// Body coordinate (e.g. `"deck/slide/3"`, `"document/section/2"`,
    /// `"workbook/Receipts/B7"`).
    pub at: String,
}
```

> The trait every data adapter implements.
> 
> Adapters live out of `poiesis-core`; consumers register them through a
> [`DataSourceRegistry`] before calling [`Factbase::resolve`]. A factbase
> with no `Source::Sql` facts needs no adapters.
```rust
pub trait DataSource : Send + Sync {
    fn id (&self) -> &DataSourceId;
    fn query (&self, query: &str, table: &str) -> Result<Scalar, String>;
}
```

```rust
pub struct DataSourceRegistry {
    adapters: HashMap<DataSourceId, Box<dyn DataSource>>,
}
```

```rust
impl DataSourceRegistry {
    pub fn new () -> Self;
    pub fn register (&mut self, adapter: Box<dyn DataSource>);
    pub fn get (&self, id: &DataSourceId) -> Option<&dyn DataSource>;
}
```

```rust
pub struct Factbase {
    /// Facts in declaration order.
    pub facts: indexmap::IndexMap<FactId, Fact>,
    /// Claims in declaration order.
    pub claims: indexmap::IndexMap<ClaimId, Claim>,
}
```

```rust
pub struct ResolvedFact {
    /// The fact id.
    pub id: FactId,
    /// The resolved value (for `Derived`/`Sql`, the computed result; for the
    /// other kinds, the authored value).
    pub value: Scalar,
    /// The presentation unit.
    pub unit: Unit,
}
```

```rust
impl Factbase {
    pub fn new () -> Self;
    pub fn add_fact (&mut self, fact: Fact);
    pub fn add_claim (&mut self, claim: Claim);
    pub fn validate (&self) -> Result<(), FactbaseError>;
    pub fn resolve (
        &self,
        adapters: &DataSourceRegistry,
    ) -> Result<BTreeMap<FactId, ResolvedFact>, FactbaseError>;
    pub fn walk_citation_chain (&self, root: &FactId) -> Vec<FactId>;
    pub fn claim_citation_chain (&self, claim_id: &ClaimId) -> Option<Vec<FactId>>;
}
```

## `src/lib.rs`

```rust
pub struct Artifact {
    /// The format identifier, e.g. `"pdf"`, `"pptx"`, `"docx"`, `"xlsx"`,
    /// `"html"`.
    pub format: String,
    /// The body kind the artifact represents.
    pub body_kind: BodyKind,
    /// The complete byte payload.
    pub bytes: Vec<u8>,
}
```

```rust
pub enum BodyKind {
    /// A deck composed of [`bodies::Slide`]s.
    Deck,
    /// A prose document (pre-envelope [`document::Document`]).
    Document,
    /// A workbook of named [`bodies::Sheet`]s.
    Workbook,
}
```

```rust
impl BodyKind {
    pub fn as_str (self) -> &'static str;
}
```

## `src/metadata.rs`

```rust
pub struct Metadata {
    /// Document title shown in viewer toolbars and export filenames.
    pub title: String,
    /// Optional author name embedded in document properties.
    pub author: Option<String>,
    /// Optional creation timestamp embedded in document properties.
    pub created: Option<Timestamp>,
}
```

```rust
impl Metadata {
    pub fn titled (title: impl Into<String>) -> Self;
}
```

## `src/qa.rs`

```rust
pub enum QaIssueKind {
    /// A citation could not be resolved to a fact.
    CitationUnresolvable,
    /// A claim does not match the fact it cites.
    ClaimMismatch,
    /// Prose violated a style or structural rule.
    ProseViolation,
    /// A required section is absent from the document.
    MissingSection,
}
```

```rust
pub struct QaIssue {
    /// The classification of the issue.
    pub kind: QaIssueKind,
    /// Optional source location (e.g. a JSON pointer or line reference).
    pub location: Option<String>,
    /// Human-readable description of the issue.
    pub message: String,
}
```

```rust
pub struct QaReport {
    /// Whether any issues were found.
    pub has_issues: bool,
    /// Number of issues in this report.
    pub issue_count: usize,
    /// The individual issues comprising this report.
    pub issues: Vec<QaIssue>,
}
```

```rust
impl QaReport {
    pub fn pass () -> Self;
    pub fn new (issues: Vec<QaIssue>) -> Self;
    pub fn merge (reports: impl IntoIterator<Item = QaReport>) -> Self;
    pub fn is_clean (&self) -> bool;
    pub fn to_json (&self) -> Result<String, serde_json::Error>;
}
```

## `src/renderer.rs`

> A format backend that renders a [`Document`] into raw bytes.
> 
> Implementors produce a self-contained byte payload for their target format
> (e.g. PDF, ODT, XLSX). The caller writes those bytes to disk or streams
> them over the network.
```rust
pub trait Renderer {
    fn render (&self, doc: &Document) -> Result<Vec<u8>, Self::Error>;
    fn format (&self) -> &'static str;
}
```

## `src/rich_text.rs`

```rust
pub struct RichText {
    /// Ordered list of styled inline spans.
    pub spans: Vec<Span>,
}
```

```rust
impl RichText {
    pub fn plain_text (&self) -> String;
}
```

```rust
pub enum Span {
    /// Unstyled text.
    Plain(String),
    /// Bold text.
    Bold(String),
    /// Italic text.
    Italic(String),
    /// Inline code with monospace rendering.
    Code(String),
    /// Citation placeholder carrying a fact id.
    Cite(String),
    /// Hyperlink with visible label and target URL.
    Link {
        /// Display text shown to the reader.
        text: String,
        /// Destination URL.
        url: String,
    },
}
```

## `src/scalar.rs`

```rust
pub enum ScalarKind {
    /// Whole-number counts (`Count(i64)`).
    Count,
    /// Monetary amounts (`Money` — `i64` micro-units).
    Money,
    /// Dimensionless ratios (`Ratio(f64)`); use [`Unit::Percent`] when display
    /// should multiply by 100.
    Ratio,
    /// Free-form text values.
    Text,
    /// Calendar dates.
    Date,
}
```

```rust
impl ScalarKind {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub struct Money(i64);
```

```rust
impl Money {
    pub const fn from_micros (micros: i64) -> Self;
    pub const fn micros (self) -> i64;
    pub fn from_units (units: i64) -> Result<Self, ScalarError>;
    pub fn from_units_and_fraction (units: i64, fraction: u32) -> Result<Self, ScalarError>;
}
```

```rust
pub enum Scalar {
    /// Whole-number count.
    Count {
        /// The count.
        value: i64,
    },
    /// Monetary amount (see [`Money`]).
    Money {
        /// The amount.
        value: Money,
    },
    /// Dimensionless ratio (`0.5` = 50%); pair with [`Unit::Percent`] for display.
    Ratio {
        /// The ratio.
        value: f64,
    },
    /// Free-form text.
    Text {
        /// The text.
        value: String,
    },
    /// Calendar date.
    Date {
        /// The date.
        value: Date,
    },
}
```

```rust
impl Scalar {
    pub fn kind (&self) -> ScalarKind;
}
```

```rust
pub enum Unit {
    /// A dimensionless count (people, sessions, line items).
    Count,
    /// US dollars (presentation currency; see [`Money`] for value shape).
    Usd,
    /// Dimensionless percentage; display multiplies the underlying ratio by 100.
    Percent,
    /// Dimensionless ratio; display leaves the underlying ratio as-is.
    Ratio,
    /// A calendar date.
    Date,
    /// Free-form text (label, slug, name).
    Text,
}
```

```rust
impl Unit {
    pub fn as_str (self) -> &'static str;
    pub fn compatible_with (self, kind: ScalarKind) -> bool;
}
```

```rust
pub struct AspectRatio {
    width: u16,
    height: u16,
}
```

```rust
impl AspectRatio {
    pub fn new (width: u16, height: u16) -> Result<Self, ScalarError>;
    pub fn width (self) -> u16;
    pub fn height (self) -> u16;
}
```

```rust
pub struct Tolerance(f64);
```

```rust
impl Tolerance {
    pub fn new (value: f64) -> Result<Self, ScalarError>;
    pub fn as_f64 (self) -> f64;
}
```
