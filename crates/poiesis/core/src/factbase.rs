//! The single home for every number cited by a deliverable.
//!
//! A [`Factbase`] is a collection of typed [`Fact`]s and [`Claim`]s. Every
//! numeric value a renderer emits is a `Cite(FactId)` reference into the
//! factbase — naked numbers are rejected by [`crate::components`] schema
//! validation and by the QA gate downstream. Facts may be `Manual` (operator
//! assertion), `File` (CSV cell / JSON path), `Sql` (resolved via a
//! [`DataSource`] adapter), `Derived` (an arithmetic expression over other
//! facts), or `Reference` (an alias for another fact).
//!
//! [`Factbase::resolve`] walks the dependency graph in declaration order,
//! detecting cycles and unsourced references before any data adapter is
//! invoked. Adapters are optional: a factbase with no `Sql` facts is
//! resolvable without configuring any [`DataSource`].

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{
    BadDerivedSnafu, CycleSnafu, DerivedTypeMismatchSnafu, FactInputsMissingSnafu, FactbaseError,
    MissingDataSourceSnafu, UnknownFactSnafu,
};
use crate::ids::{ClaimId, DataSourceId, FactId};
use crate::scalar::{Money, Scalar, Tolerance, Unit};

/// A typed, sourced value that may be cited by a deliverable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// The provenance of a [`Fact`].
// kanon:ignore RUST/non-exhaustive-enum — exhaustive match is part of the
// stable API; new sources are an explicit additive evolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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

/// An arithmetic expression in a `Derived` source.
///
/// Kept intentionally narrow at the v1.0.0 boundary: addition, subtraction,
/// multiplication, division, sum, mean. Sufficient for the offsite deck
/// claims and the workbook total rows; not a calculator.
// kanon:ignore RUST/non-exhaustive-enum — additive evolution is the intended
// migration path; keeping exhaustive matches keeps the gate honest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
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

/// A claim made by some location in the deliverable, asserting a fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

fn default_strict_tolerance() -> Tolerance {
    Tolerance::STRICT
}

/// A coarse pointer into the deliverable for surfacing where a claim lives.
///
/// Free-form by intent: render-side B-NNN will refine this to typed
/// references once their bodies stabilise. The QA gate echoes this verbatim
/// in its findings, so authors and reviewers can locate the claim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    /// Body coordinate (e.g. `"deck/slide/3"`, `"document/section/2"`,
    /// `"workbook/Receipts/B7"`).
    pub at: String,
}

/// The trait every data adapter implements.
///
/// Adapters live out of `poiesis-core`; consumers register them through a
/// [`DataSourceRegistry`] before calling [`Factbase::resolve`]. A factbase
/// with no `Source::Sql` facts needs no adapters.
// kanon:ignore RUST/pub-visibility — external adapters implement this trait outside poiesis-core
pub trait DataSource: Send + Sync {
    /// The id this adapter handles.
    fn id(&self) -> &DataSourceId;

    /// Execute `query` against the adapter and return a single scalar value.
    ///
    /// # Errors
    ///
    /// Implementations return a string error; the resolver wraps it in
    /// [`FactbaseError::BadDerived`] with the adapter id as context. (The
    /// dedicated `SqlAdapterError` variant is intentionally postponed to
    /// [[B-008]] when the first real adapter ships.)
    fn query(&self, query: &str, table: &str) -> Result<Scalar, String>;
}

/// Container for registered [`DataSource`] adapters.
///
/// Empty by default. A factbase with `Source::Sql` facts whose data-source
/// id is not registered fails resolution with
/// [`FactbaseError::MissingDataSource`].
// kanon:ignore RUST/pub-visibility — passed to Factbase::resolve by callers outside poiesis-core
#[derive(Default)]
pub struct DataSourceRegistry {
    adapters: HashMap<DataSourceId, Box<dyn DataSource>>,
}

impl std::fmt::Debug for DataSourceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataSourceRegistry")
            .field("adapter_ids", &self.adapters.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl DataSourceRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a [`DataSource`] adapter; replaces any prior adapter for the
    /// same id.
    pub fn register(&mut self, adapter: Box<dyn DataSource>) {
        let id = adapter.id().clone();
        self.adapters.insert(id, adapter);
    }

    /// Look up an adapter by id.
    #[must_use]
    pub fn get(&self, id: &DataSourceId) -> Option<&dyn DataSource> {
        self.adapters.get(id).map(std::convert::AsRef::as_ref)
    }
}

/// A collection of [`Fact`]s and [`Claim`]s forming the citation graph.
///
/// Order is preserved (`indexmap::IndexMap`) so resolution and serialisation
/// honour declaration order; this matches the source spec's "resolve in
/// declaration order" requirement.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Factbase {
    /// Facts in declaration order.
    pub facts: indexmap::IndexMap<FactId, Fact>,
    /// Claims in declaration order.
    pub claims: indexmap::IndexMap<ClaimId, Claim>,
}

/// The outcome of resolving a single fact.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFact {
    /// The fact id.
    pub id: FactId,
    /// The resolved value (for `Derived`/`Sql`, the computed result; for the
    /// other kinds, the authored value).
    pub value: Scalar,
    /// The presentation unit.
    pub unit: Unit,
}

impl ResolvedFact {
    /// Construct a resolved fact.
    pub fn new(id: FactId, value: Scalar, unit: Unit) -> Self {
        Self { id, value, unit }
    }
}

impl Factbase {
    /// Construct an empty factbase.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a fact; preserves declaration order.
    pub fn add_fact(&mut self, fact: Fact) {
        let id = fact.id.clone();
        self.facts.insert(id, fact);
    }

    /// Add a claim; preserves declaration order.
    pub fn add_claim(&mut self, claim: Claim) {
        let id = claim.id.clone();
        self.claims.insert(id, claim);
    }

    /// Validate the citation graph without resolving values.
    ///
    /// Checks every claim's `asserts` and every `Source::Reference`/`Derived`
    /// input resolves to a known fact id; checks the `Derived`/`Reference`
    /// dependency graph is acyclic. Returns the first error encountered.
    ///
    /// # Errors
    ///
    /// Returns [`FactbaseError::UnknownFact`] for dangling references,
    /// [`FactbaseError::FactInputsMissing`] when a `Derived` formula references
    /// a fact that exists but is omitted from `inputs`, and
    /// [`FactbaseError::Cycle`] for a cyclic dependency chain.
    pub fn validate(&self) -> Result<(), FactbaseError> {
        for claim in self.claims.values() {
            if !self.facts.contains_key(&claim.asserts) {
                return UnknownFactSnafu {
                    id: claim.asserts.as_str(),
                    referenced_by: format!("claim {}", claim.id),
                }
                .fail();
            }
        }
        for fact in self.facts.values() {
            for input in source_inputs(&fact.source) {
                if !self.facts.contains_key(input) {
                    return UnknownFactSnafu {
                        id: input.as_str(),
                        referenced_by: format!("source of fact {}", fact.id),
                    }
                    .fail();
                }
            }
            // WHY: The formula is the canonical dependency set for a Derived
            // fact. `source_inputs` returns the declared `inputs`, so a formula
            // reference that exists in the factbase but is omitted from `inputs`
            // would pass the existence check and then fail at resolve time with
            // an indistinguishable `UnknownFact`. Validate both existence and
            // subset here so callers get a precise error at build time.
            if let Source::Derived { formula, inputs } = &fact.source {
                let input_set: HashSet<&FactId> = inputs.iter().collect();
                for ref_id in expr_fact_ids(formula) {
                    if !self.facts.contains_key(ref_id) {
                        return UnknownFactSnafu {
                            id: ref_id.as_str(),
                            referenced_by: format!("formula of fact {}", fact.id),
                        }
                        .fail();
                    }
                    if !input_set.contains(ref_id) {
                        return FactInputsMissingSnafu {
                            id: ref_id.as_str(),
                            derived_fact: fact.id.as_str(),
                        }
                        .fail();
                    }
                }
            }
        }
        self.detect_cycle()?;
        Ok(())
    }

    /// Resolve every fact in declaration order.
    ///
    /// Calls `validate` first; on success, computes `Derived` values,
    /// follows `Reference` aliases, and dispatches `Sql` queries through
    /// `adapters`. `Manual` and `File` facts resolve to their authored value
    /// (file extraction is the caller's responsibility — this resolver does
    /// not read files).
    ///
    /// # Errors
    ///
    /// Returns the first [`FactbaseError`] encountered during validation or
    /// resolution.
    pub fn resolve(
        &self,
        adapters: &DataSourceRegistry,
    ) -> Result<BTreeMap<FactId, ResolvedFact>, FactbaseError> {
        self.validate()?;
        let order = self.topo_order();
        let known_ids: BTreeSet<FactId> = self.facts.keys().cloned().collect();
        let mut resolved: BTreeMap<FactId, ResolvedFact> = BTreeMap::new();
        for id in order {
            let Some(entry) = self.facts.get(&id) else {
                continue;
            };
            let value = match &entry.source {
                Source::Manual { .. } | Source::File { .. } => entry.value.clone(),
                Source::Reference { fact } => match resolved.get(fact) {
                    Some(r) => r.value.clone(),
                    None => entry.value.clone(),
                },
                Source::Derived { formula, .. } => {
                    evaluate_expr(formula, &resolved, &known_ids, &entry.id)?
                }
                Source::Sql {
                    data_source,
                    query,
                    table,
                } => match adapters.get(data_source) {
                    Some(adapter) => match adapter.query(query.as_str(), table.as_str()) {
                        Ok(v) => v,
                        Err(e) => {
                            return BadDerivedSnafu {
                                detail: format!("adapter {data_source} returned error: {e}"),
                            }
                            .fail();
                        }
                    },
                    None => {
                        return MissingDataSourceSnafu {
                            claim_id: entry.id.as_str(),
                            data_source: data_source.as_str(),
                        }
                        .fail();
                    }
                },
            };
            resolved.insert(
                entry.id.clone(),
                ResolvedFact {
                    id: entry.id.clone(),
                    value,
                    unit: entry.unit,
                },
            );
        }
        Ok(resolved)
    }

    /// Returns all transitive source inputs of `root` in post-order (leaves first),
    /// deduplicated. Returns an empty vec if `root` is not in the factbase or has no inputs.
    pub fn walk_citation_chain(&self, root: &FactId) -> Vec<FactId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        if let Some(fact) = self.facts.get(root) {
            for input in source_inputs(&fact.source) {
                self.dfs_walk(input, &mut visited, &mut result);
            }
        }
        result
    }

    fn dfs_walk(&self, node: &FactId, visited: &mut HashSet<FactId>, result: &mut Vec<FactId>) {
        if visited.contains(node) {
            return;
        }
        if let Some(fact) = self.facts.get(node) {
            for input in source_inputs(&fact.source) {
                self.dfs_walk(input, visited, result);
            }
            visited.insert(node.clone());
            result.push(node.clone());
        }
    }

    /// Returns the citation chain for the fact asserted by `claim_id`,
    /// or `None` if the claim is not in the factbase.
    pub fn claim_citation_chain(&self, claim_id: &ClaimId) -> Option<Vec<FactId>> {
        let claim = self.claims.get(claim_id)?;
        Some(self.walk_citation_chain(&claim.asserts))
    }

    /// Detect cycles in the `Derived`/`Reference` dependency graph.
    fn detect_cycle(&self) -> Result<(), FactbaseError> {
        let mut marks: HashMap<&FactId, Mark> =
            self.facts.keys().map(|id| (id, Mark::None)).collect();
        let mut stack: Vec<&FactId> = Vec::new();
        for root in self.facts.keys() {
            if matches!(marks.get(root), Some(Mark::Done)) {
                continue;
            }
            self.dfs_cycle(root, &mut marks, &mut stack)?;
        }
        Ok(())
    }

    fn dfs_cycle<'a>(
        &'a self,
        node: &'a FactId,
        marks: &mut HashMap<&'a FactId, Mark>,
        stack: &mut Vec<&'a FactId>,
    ) -> Result<(), FactbaseError> {
        match marks.get(node) {
            Some(Mark::Done) => return Ok(()),
            Some(Mark::InProgress) => {
                let mut path: Vec<String> = stack.iter().map(|f| f.as_str().to_owned()).collect();
                path.push(node.as_str().to_owned());
                if let Some(first) = path.iter().position(|p| p == node.as_str()) {
                    let cycle: Vec<String> = path.into_iter().skip(first).collect();
                    return CycleSnafu { path: cycle }.fail();
                }
                return CycleSnafu { path }.fail();
            }
            Some(Mark::None) | None => { /* not yet visited — proceed to mark InProgress */ }
        }
        marks.insert(node, Mark::InProgress);
        stack.push(node);
        if let Some(fact) = self.facts.get(node) {
            for input in source_inputs(&fact.source) {
                if let Some((k, _)) = self.facts.get_key_value(input) {
                    self.dfs_cycle(k, marks, stack)?;
                }
            }
        }
        stack.pop();
        marks.insert(node, Mark::Done);
        Ok(())
    }

    /// Topological order in declaration-priority: facts with no dependencies
    /// first, then facts depending on already-emitted facts. Within an
    /// independent set, the original declaration order is preserved.
    fn topo_order(&self) -> Vec<FactId> {
        let mut visited: BTreeSet<FactId> = BTreeSet::new();
        let mut order: Vec<FactId> = Vec::with_capacity(self.facts.len());
        for id in self.facts.keys() {
            self.dfs_topo(id, &mut visited, &mut order);
        }
        order
    }

    fn dfs_topo(&self, node: &FactId, visited: &mut BTreeSet<FactId>, order: &mut Vec<FactId>) {
        if visited.contains(node) {
            return;
        }
        if let Some(fact) = self.facts.get(node) {
            for input in source_inputs(&fact.source) {
                self.dfs_topo(input, visited, order);
            }
        }
        visited.insert(node.clone());
        if self.facts.contains_key(node) {
            order.push(node.clone());
        }
    }
}

/// Borrow the fact ids that a `Source` depends on.
fn source_inputs(source: &Source) -> Vec<&FactId> {
    match source {
        Source::Reference { fact } => vec![fact],
        Source::Derived { inputs, .. } => inputs.iter().collect(),
        Source::Sql { .. } | Source::Manual { .. } | Source::File { .. } => Vec::new(),
    }
}

/// Borrow every fact id referenced by a `Derived` expression.
//
// WHY: The formula is the canonical dependency set for a Derived fact; this
// helper lets validation compare it against the factbase.
fn expr_fact_ids(expr: &Expr) -> Vec<&FactId> {
    match expr {
        Expr::Add { a, b } | Expr::Sub { a, b } | Expr::Mul { a, b } | Expr::Div { a, b } => {
            vec![a, b]
        }
        Expr::Sum { terms } => terms.iter().collect(),
    }
}

#[derive(Clone, Copy)]
enum Mark {
    None,
    InProgress,
    Done,
}

fn evaluate_expr(
    expr: &Expr,
    resolved: &BTreeMap<FactId, ResolvedFact>,
    known_ids: &BTreeSet<FactId>,
    derived_fact_id: &FactId,
) -> Result<Scalar, FactbaseError> {
    match expr {
        Expr::Add { a, b } => binary_op(
            a,
            b,
            resolved,
            known_ids,
            derived_fact_id,
            "+",
            |x, y| x + y,
            i64::checked_add,
        ),
        Expr::Sub { a, b } => binary_op(
            a,
            b,
            resolved,
            known_ids,
            derived_fact_id,
            "-",
            |x, y| x - y,
            i64::checked_sub,
        ),
        Expr::Mul { a, b } => binary_op(
            a,
            b,
            resolved,
            known_ids,
            derived_fact_id,
            "*",
            |x, y| x * y,
            i64::checked_mul,
        ),
        Expr::Div { a, b } => {
            let left = lookup(a, resolved, known_ids, derived_fact_id)?;
            let right = lookup(b, resolved, known_ids, derived_fact_id)?;
            let lf = scalar_to_f64(&left.value)?;
            let rf = scalar_to_f64(&right.value)?;
            if rf == 0.0 {
                return BadDerivedSnafu {
                    detail: format!("division by zero: {a} / {b}"),
                }
                .fail();
            }
            Scalar::new_ratio(lf / rf).map_err(|e| FactbaseError::BadDerived {
                detail: e.to_string(),
            })
        }
        Expr::Sum { terms } => {
            let mut iter = terms.iter();
            let Some(head_id) = iter.next() else {
                return Ok(Scalar::Count { value: 0 });
            };
            let head = lookup(head_id, resolved, known_ids, derived_fact_id)?;
            let mut acc = head.value.clone();
            for term_id in iter {
                let next = lookup(term_id, resolved, known_ids, derived_fact_id)?;
                acc = add_scalars(&acc, &next.value)?;
            }
            Ok(acc)
        }
    }
}

fn lookup<'a>(
    id: &FactId,
    resolved: &'a BTreeMap<FactId, ResolvedFact>,
    known_ids: &BTreeSet<FactId>,
    derived_fact_id: &FactId,
) -> Result<&'a ResolvedFact, FactbaseError> {
    if let Some(fact) = resolved.get(id) {
        return Ok(fact);
    }
    // WHY: A fact id can be absent from `resolved` for two reasons: it was
    // never declared (unknown), or it exists in the factbase but was omitted
    // from the derived fact's `inputs` and therefore not resolved before this
    // expression. `validate()` normally catches the latter, but this branch
    // keeps the runtime error precise if resolution is ever invoked without
    // validation.
    if known_ids.contains(id) {
        return FactInputsMissingSnafu {
            id: id.as_str(),
            derived_fact: derived_fact_id.as_str(),
        }
        .fail();
    }
    UnknownFactSnafu {
        id: id.as_str(),
        referenced_by: "derived expression".to_owned(),
    }
    .fail()
}

fn binary_op(
    lhs_id: &FactId,
    rhs_id: &FactId,
    resolved: &BTreeMap<FactId, ResolvedFact>,
    known_ids: &BTreeSet<FactId>,
    derived_fact_id: &FactId,
    op_name: &str,
    f_op: fn(f64, f64) -> f64,
    i_op: fn(i64, i64) -> Option<i64>,
) -> Result<Scalar, FactbaseError> {
    let lhs = lookup(lhs_id, resolved, known_ids, derived_fact_id)?;
    let rhs = lookup(rhs_id, resolved, known_ids, derived_fact_id)?;
    match (&lhs.value, &rhs.value) {
        (Scalar::Count { value: lv }, Scalar::Count { value: rv }) => {
            let summed = i_op(*lv, *rv).ok_or_else(|| FactbaseError::BadDerived {
                detail: format!("integer overflow in {lhs_id} {op_name} {rhs_id}"),
            })?;
            Ok(Scalar::Count { value: summed })
        }
        (Scalar::Money { value: lv }, Scalar::Money { value: rv }) => match op_name {
            "+" => {
                let summed = lv.micros().checked_add(rv.micros()).ok_or_else(|| {
                    FactbaseError::BadDerived {
                        detail: format!("integer overflow in money {lhs_id} {op_name} {rhs_id}"),
                    }
                })?;
                Ok(Scalar::Money {
                    value: Money::from_micros(summed),
                })
            }
            "-" => {
                let diff = lv.micros().checked_sub(rv.micros()).ok_or_else(|| {
                    FactbaseError::BadDerived {
                        detail: format!("integer overflow in money {lhs_id} {op_name} {rhs_id}"),
                    }
                })?;
                Ok(Scalar::Money {
                    value: Money::from_micros(diff),
                })
            }
            _ => BadDerivedSnafu {
                detail: format!("operator {op_name} not defined on money pair {lhs_id}, {rhs_id}"),
            }
            .fail(),
        },
        (Scalar::Ratio { value: lv }, Scalar::Ratio { value: rv }) => {
            Scalar::new_ratio(f_op(*lv, *rv)).map_err(|e| FactbaseError::BadDerived {
                detail: e.to_string(),
            })
        }
        _ => DerivedTypeMismatchSnafu {
            detail: format!(
                "{op_name} requires same-typed operands; got {} and {}",
                lhs.value.kind(),
                rhs.value.kind()
            ),
        }
        .fail(),
    }
}

fn add_scalars(lhs: &Scalar, rhs: &Scalar) -> Result<Scalar, FactbaseError> {
    match (lhs, rhs) {
        (Scalar::Count { value: lv }, Scalar::Count { value: rv }) => {
            let summed = lv
                .checked_add(*rv)
                .ok_or_else(|| FactbaseError::BadDerived {
                    detail: "integer overflow in sum".to_owned(),
                })?;
            Ok(Scalar::Count { value: summed })
        }
        (Scalar::Money { value: lv }, Scalar::Money { value: rv }) => {
            let summed =
                lv.micros()
                    .checked_add(rv.micros())
                    .ok_or_else(|| FactbaseError::BadDerived {
                        detail: "integer overflow in money sum".to_owned(),
                    })?;
            Ok(Scalar::Money {
                value: Money::from_micros(summed),
            })
        }
        (Scalar::Ratio { value: lv }, Scalar::Ratio { value: rv }) => Scalar::new_ratio(lv + rv)
            .map_err(|e| FactbaseError::BadDerived {
                detail: e.to_string(),
            }),
        _ => DerivedTypeMismatchSnafu {
            detail: format!(
                "sum requires same-typed operands; got {} and {}",
                lhs.kind(),
                rhs.kind()
            ),
        }
        .fail(),
    }
}

/// Lossy `i64`-to-`f64` conversion is acceptable here: the divide-then-
/// compare in [`Expr::Div`] only needs to land within tolerance and the QA
/// gate compares with [`Tolerance`], not raw equality. The casts cannot
/// truncate (i64 → f64 is widening), only lose mantissa precision for
/// magnitudes above `2^53`, which is well outside the deliverable range.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "ratio output is a precision-tolerant aggregate"
)]
fn scalar_to_f64(s: &Scalar) -> Result<f64, FactbaseError> {
    match s {
        Scalar::Count { value } => Ok(*value as f64),
        Scalar::Money { value } => Ok((value.micros() as f64) / 1_000_000.0),
        Scalar::Ratio { value } => Ok(*value),
        Scalar::Text { .. } | Scalar::Date { .. } => DerivedTypeMismatchSnafu {
            detail: format!("cannot convert {} to numeric", s.kind()),
        }
        .fail(),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, clippy::unwrap_used, reason = "test assertions")]
#[path = "factbase_tests.rs"]
mod tests;
