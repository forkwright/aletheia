//! Abstract syntax tree for the krites v2 Datalog dialect.
//!
//! This AST captures the full surface syntax of queries, writes, and DDL
//! statements. It is produced by the recursive-descent parser and consumed
//! by the query planner.

use crate::v2::schema::ColumnType;
use crate::v2::value::Value;

// ---------------------------------------------------------------------------
// Statement
// ---------------------------------------------------------------------------

/// A top-level Datalog statement.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Statement {
    /// Query (read) statement.
    Query(Query),
    /// Insert or update tuples.
    Put {
        /// Target relation name.
        relation: String,
        /// Rows to insert, each a list of (column, expression) pairs.
        rows: Vec<Vec<(String, Expr)>>,
    },
    /// Create a stored relation with a schema.
    Create {
        /// Relation name.
        relation: String,
        /// Parsed schema specification.
        schema: SchemaSpec,
    },
    /// Replace (drop and recreate) a stored relation.
    Replace {
        /// Relation name.
        relation: String,
        /// Parsed schema specification.
        schema: SchemaSpec,
    },
    /// Remove a stored relation.
    Remove {
        /// Relation name.
        relation: String,
    },
    /// Create a full-text search index.
    FtsCreate {
        /// Relation name.
        relation: String,
        /// FTS configuration.
        config: FtsConfig,
    },
    /// Create an HNSW vector index.
    HnswCreate {
        /// Relation name.
        relation: String,
        /// HNSW configuration.
        config: HnswConfig,
    },
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// A Datalog query: outputs, rule body, ordering, and limit.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Query {
    /// Output columns (projection / aggregation).
    pub outputs: Vec<OutputCol>,
    /// Rule bodies (disjunction — at least one).
    pub rules: Vec<Rule>,
    /// Result ordering specifications.
    pub ordering: Vec<OrderSpec>,
    /// Optional result limit.
    pub limit: Option<Expr>,
}

/// A single output column, optionally aggregated.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OutputCol {
    /// Column or variable name.
    pub name: String,
    /// Optional aggregation function.
    pub aggregation: Option<Aggregation>,
}

/// Aggregation functions supported in output columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Aggregation {
    /// Count of non-null values.
    Count,
    /// Sum of numeric values.
    Sum,
    /// Maximum value.
    Max,
    /// Minimum value.
    Min,
    /// Arithmetic mean.
    Mean,
}

/// Sort specification.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OrderSpec {
    /// Column or variable name to sort by.
    pub column: String,
    /// Whether to sort descending.
    pub descending: bool,
}

// ---------------------------------------------------------------------------
// Rule
// ---------------------------------------------------------------------------

/// A rule body: atoms joined by `,` and filtered by boolean expressions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Rule {
    /// Atoms (relation references) in the rule body.
    pub atoms: Vec<Atom>,
    /// Boolean filter expressions.
    pub filters: Vec<Filter>,
}

/// A filter is a boolean expression that constrains the rule.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Filter {
    /// The boolean expression.
    pub expr: Expr,
}

// ---------------------------------------------------------------------------
// Atom
// ---------------------------------------------------------------------------

/// An atom in a rule body.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Atom {
    /// Dereference a stored relation: `*relation{bindings}`.
    Stored {
        /// Relation name.
        relation: String,
        /// Variable bindings.
        bindings: Vec<Binding>,
    },
    /// Index lookup: `~relation:index{bindings | params}`.
    Index {
        /// Relation name.
        relation: String,
        /// Index name.
        index: String,
        /// Variable bindings.
        bindings: Vec<Binding>,
        /// Index-specific parameters.
        params: Vec<(String, Expr)>,
    },
    /// Fixed-rule (graph algorithm) invocation: `<~AlgorithmName{inputs | options}`.
    FixedRule {
        /// Algorithm name.
        name: String,
        /// Input relations.
        inputs: Vec<InputRelation>,
        /// Algorithm-specific options.
        options: Vec<(String, Expr)>,
    },
    /// Temporary (headless) relation: `name{bindings}`.
    Temp {
        /// Relation name.
        name: String,
        /// Variable bindings.
        bindings: Vec<Binding>,
    },
}

/// A variable binding inside an atom's braces.
///
/// Named bindings: `col: var`  → `column = Some("col")`, `variable = "var"`
/// Positional bindings: `var` → `column = None`, `variable = "var"`
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Binding {
    /// Column name for named bindings; `None` for positional.
    pub column: Option<String>,
    /// Bound variable name.
    pub variable: String,
}

/// An input relation for a fixed-rule atom.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InputRelation {
    /// Relation name.
    pub name: String,
    /// Variable bindings.
    pub bindings: Vec<Binding>,
}

// ---------------------------------------------------------------------------
// Expression
// ---------------------------------------------------------------------------

/// An expression: variables, parameters, literals, function calls, and ops.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Expr {
    /// A Datalog variable.
    Var(String),
    /// A runtime parameter: `$name`.
    Param(String),
    /// A literal value.
    Literal(Value),
    /// A function call: `name(args)`.
    FnCall {
        /// Function name.
        name: String,
        /// Arguments.
        args: Vec<Expr>,
    },
    /// Binary operation.
    BinOp {
        /// Operator.
        op: BinOp,
        /// Left operand.
        left: Box<Expr>,
        /// Right operand.
        right: Box<Expr>,
    },
    /// Unary operation.
    UnaryOp {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        operand: Box<Expr>,
    },
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BinOp {
    /// Equal: `=`.
    Eq,
    /// Not equal: `!=`.
    Neq,
    /// Less than: `<`.
    Lt,
    /// Greater than: `>`.
    Gt,
    /// Less than or equal: `<=`.
    Lte,
    /// Greater than or equal: `>=`.
    Gte,
    /// Logical and: `&&`.
    And,
    /// Logical or: `||`.
    Or,
    /// Add: `+`.
    Add,
    /// Subtract: `-`.
    Sub,
    /// Multiply: `*`.
    Mul,
    /// Divide: `/`.
    Div,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnaryOp {
    /// Logical not: `!`.
    Not,
    /// Arithmetic negation: `-`.
    Neg,
}

// ---------------------------------------------------------------------------
// Schema specification (parsed before conversion to RelationSchema)
// ---------------------------------------------------------------------------

/// Parsed schema from a `:create` or `:replace` statement.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SchemaSpec {
    /// Key column names (before `=>`).
    pub key_columns: Vec<String>,
    /// Value column specifications (after `=>`).
    pub value_columns: Vec<ValueColumnSpec>,
}

/// A value column in a schema specification.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ValueColumnSpec {
    /// Column name.
    pub name: String,
    /// Data type.
    pub column_type: ColumnType,
    /// Optional default expression.
    pub default: Option<Expr>,
}

// ---------------------------------------------------------------------------
// Index configs
// ---------------------------------------------------------------------------

/// Full-text search index configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FtsConfig {
    /// Column names to index.
    pub columns: Vec<String>,
    /// Index options.
    pub options: Vec<(String, Expr)>,
}

/// HNSW vector index configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HnswConfig {
    /// Vector column name.
    pub column: String,
    /// Index options.
    pub options: Vec<(String, Expr)>,
}
