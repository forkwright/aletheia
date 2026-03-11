//! Error types for the Datalog query engine.
use snafu::Snafu;

/// Errors from the query engine (compilation, evaluation, stratification, graph traversal).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub(crate) enum QueryError {
    /// Query compilation failed (binding indices, arity checks, etc.).
    #[snafu(display("compilation failed: {message}"))]
    CompilationFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Reference to a rule or relation that doesn't exist.
    #[snafu(display("relation not found: '{name}' — {reason}"))]
    InvalidRelation {
        name: String,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Negation stratification is impossible (cyclic negation).
    #[snafu(display("stratification failed: {message}"))]
    StratificationFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Type error in a query expression.
    #[snafu(display("type error: expected {expected}, got {got} in {context}"))]
    TypeError {
        expected: String,
        got: String,
        context: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Graph traversal algorithm error.
    #[snafu(display("graph traversal '{algorithm}' failed: {message}"))]
    GraphTraversal {
        algorithm: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Runtime evaluation error.
    #[snafu(display("evaluation error: {message}"))]
    EvalFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Arity mismatch between rule/relation and its application.
    #[snafu(display("arity mismatch: {message}"))]
    ArityMismatch {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Unsafe negation or unbound variable in rule.
    #[snafu(display("unsafe rule: {message}"))]
    UnsafeRule {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Access level insufficient for the requested operation.
    #[snafu(display("insufficient access: {message}"))]
    InsufficientAccess {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid time-travel scanning configuration.
    #[snafu(display("invalid time travel: {message}"))]
    InvalidTimeTravel {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A named field does not exist on a stored relation.
    #[snafu(display("stored relation '{relation}' does not have field '{field}'"))]
    FieldNotFound {
        relation: String,
        field: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// An unbound variable was found in a rule head or expression.
    #[snafu(display("unbound variable: {message}"))]
    UnboundVariable {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Stored relation operation error (create, replace, put, etc.).
    #[snafu(display("stored relation error: {message}"))]
    StoredRelation {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
