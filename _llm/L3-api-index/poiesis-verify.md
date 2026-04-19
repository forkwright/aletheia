# L3 API Index: poiesis-verify

Crate path: `crates/poiesis/verify`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/arithmetic.rs`

> Evaluate an arithmetic formula string and return the f64 result.
> 
> # Errors
> 
> Returns `VerifyError::Eval` if the formula contains unknown characters,
> unmatched parentheses, or a division-by-zero.
```rust
pub fn eval (formula: &str) -> Result<f64, VerifyError>
```

## `src/error.rs`

```rust
pub enum VerifyError {
    /// Failed to evaluate an arithmetic formula.
    #[snafu(display("arithmetic evaluation failed for formula {formula:?}: {detail}"))]
    Eval {
        /// The formula that could not be evaluated.
        formula: String,
        /// Human-readable description of the parse/eval error.
        detail: String,
    },
    /// Failed to read the manifest file.
    #[snafu(display("failed to read manifest {path:?}: {source}"))]
    ReadManifest {
        /// Path that could not be read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to parse the manifest JSON.
    #[snafu(display("failed to parse manifest {path:?}: {detail}"))]
    ParseManifest {
        /// Path whose contents could not be parsed.
        path: String,
        /// JSON parse error description.
        detail: String,
    },
}
```

## `src/lib.rs`

> Stateless claim verifier.
```rust
pub struct Verifier;
```

```rust
impl Verifier {
    pub fn new () -> Self;
    pub fn verify (&self, manifest: &VerifyManifest) -> Vec<ClaimResult>;
    pub fn verify_file (&self, path: &std::path::Path) -> Result<Vec<ClaimResult>, VerifyError>;
}
```

```rust
pub struct ClaimResult {
    /// Claim identifier.
    pub id: String,
    /// Verbatim claim text.
    pub text: String,
    /// The numeric value asserted.
    pub claimed: f64,
    /// The value resolved from sources, if any source was resolvable.
    pub actual: Option<f64>,
    /// Absolute difference between `actual` and `claimed`, if `actual` is set.
    pub diff: Option<f64>,
    /// Tolerance used for this claim.
    pub tolerance: f64,
    /// Human-readable unit.
    pub unit: String,
    /// True iff the claim passes all checks.
    pub pass: bool,
    /// Result of the arithmetic sub-check, if an `arithmetic` formula was provided.
    pub arith_check: Option<ArithCheck>,
}
```

```rust
pub struct ArithCheck {
    /// The formula evaluated.
    pub formula: String,
    /// Expected result from the manifest.
    pub expected: f64,
    /// Actual evaluated result.
    pub actual: f64,
    /// Absolute difference.
    pub diff: f64,
    /// True iff diff <= tolerance.
    pub pass: bool,
}
```

```rust
pub struct VerifyResult {
    /// Per-claim results.
    pub claims: Vec<ClaimResult>,
    /// Total number of claims.
    pub total: usize,
    /// Number of passing claims.
    pub passed: usize,
    /// Number of failing claims.
    pub failed: usize,
}
```

```rust
impl VerifyResult {
    pub fn from_claims (claims: Vec<ClaimResult>) -> Self;
    pub fn any_failed (&self) -> bool;
}
```

## `src/manifest.rs`

```rust
pub struct VerifyManifest {
    /// Path or name of the report this manifest covers.
    pub report: String,
    /// Every numeric (or categorical) claim made in the report.
    pub claims: Vec<Claim>,
}
```

```rust
pub struct Claim {
    /// Unique, stable identifier used by reference sources.
    pub id: String,
    /// Verbatim text of the claim as it appears in the report.
    pub text: String,
    /// The numeric value asserted by the claim.
    pub value: f64,
    /// Human-readable unit (e.g. "dollars", "percent", "count").
    pub unit: String,
    /// Location in the report source (e.g. "line 104, h2 heading").
    pub location: String,
    /// One or more sources that back the claim.
    pub sources: Vec<Source>,
    /// Optional arithmetic formula that produces `value` from its components.
    pub arithmetic: Option<Arithmetic>,
    /// Maximum acceptable absolute difference between `value` and the resolved
    /// source value for the claim to PASS.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    /// Last known status from a previous verification run.
    pub status: Option<String>,
}
```

```rust
pub enum Source {
    /// A SQL query (stored for record-keeping; execution not performed by this crate).
    Sql {
        /// Primary table queried (for display purposes).
        table: String,
        /// Full SQL query text.
        query: String,
        /// Last observed result (populated on verify, null in authored manifest).
        result: Option<f64>,
        /// ISO-8601 date the query was last run.
        queried: String,
    },
    /// An arithmetic expression derived from other values.
    Derived {
        /// Formula string (e.g. "106365 / 107784 * 100").
        formula: String,
        /// Last observed result.
        result: Option<f64>,
    },
    /// Pointer to another claim's validated value.
    #[serde(rename = "reference")]
    Ref {
        /// The `id` of the referenced claim.
        #[serde(rename = "ref")]
        ref_id: String,
    },
}
```

```rust
pub struct Arithmetic {
    /// Formula string (e.g. "78187 + 26558 + 1620").
    pub formula: String,
    /// Expected result of evaluating the formula.
    pub result: f64,
}
```
