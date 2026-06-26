//! Error types for parse-don't-validate boundaries.
//!
//! Every typed constructor in [`crate::ids`], [`crate::scalar`], [`crate::factbase`],
//! [`crate::components`], and [`crate::envelope`] returns one of these on failure.
//! Errors carry the offending input plus, where applicable, a JSON-pointer path
//! into the source document so the caller can surface a precise rejection.

use snafu::Snafu;

/// Errors raised when constructing a typed identifier (`ComponentId`,
/// `ThemeId`, `FactId`, `ClaimId`, `SheetName`, `DataSourceId`).
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
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

/// Errors raised when parsing a typed value (`Scalar`, `Unit`, `AspectRatio`,
/// `Tolerance`).
#[derive(Debug, Clone, PartialEq, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
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
    /// A ratio value was not a finite `f64`.
    #[snafu(display("ratio {value} is not finite"))]
    BadRatio {
        /// The offending value.
        value: f64,
    },
}

/// Errors raised when validating or resolving a `Factbase`.
#[derive(Debug, Clone, PartialEq, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
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
    /// A `Derived` fact's formula references a fact that exists in the
    /// factbase but is missing from the derived fact's `inputs` list.
    #[snafu(display(
        "derived fact {derived_fact:?} references {id:?} in its formula but not in inputs"
    ))]
    FactInputsMissing {
        /// The referenced fact id that is absent from `inputs`.
        id: String,
        /// The derived fact whose `inputs` list is incomplete.
        derived_fact: String,
    },
}

/// Errors raised by the component registry: discovery, schema parse, slot
/// validation.
#[derive(Debug, Clone, PartialEq, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
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

/// Errors raised when parsing or validating a [`crate::envelope::DeliverableSpec`].
#[derive(Debug, Clone, PartialEq, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
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
    /// A [`crate::bodies::Sheet`] violates a structural invariant: either
    /// `column_types.len()` does not equal `headers.len()`, or a row does not
    /// contain exactly `headers.len()` cells.
    #[snafu(display(
        "sheet {sheet:?} shape mismatch: expected {expected} columns, got {got} at {location}",
        location = row.map_or_else(|| "column_types".to_string(), |r| format!("row {r}"))
    ))]
    SheetShapeMismatch {
        /// The sheet display name that failed validation.
        sheet: String,
        /// `None` for a `column_types` length mismatch; `Some(index)` for the
        /// offending row.
        row: Option<usize>,
        /// The expected column count (`headers.len()`).
        expected: usize,
        /// The actual column count that was found.
        got: usize,
    },
}

/// Umbrella error covering every parse-don't-validate boundary in this crate.
#[derive(Debug, Clone, PartialEq, Snafu)]
#[non_exhaustive]
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

impl From<IdError> for PoiesisError {
    fn from(source: IdError) -> Self {
        Self::Id { source }
    }
}

impl From<ScalarError> for PoiesisError {
    fn from(source: ScalarError) -> Self {
        Self::Scalar { source }
    }
}

impl From<FactbaseError> for PoiesisError {
    fn from(source: FactbaseError) -> Self {
        Self::Factbase { source }
    }
}

impl From<RegistryError> for PoiesisError {
    fn from(source: RegistryError) -> Self {
        Self::Registry { source }
    }
}

impl From<SpecError> for PoiesisError {
    fn from(source: SpecError) -> Self {
        Self::Spec { source }
    }
}
