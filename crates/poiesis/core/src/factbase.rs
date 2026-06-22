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
    BadDerivedSnafu, CycleSnafu, DerivedTypeMismatchSnafu, FactbaseError, MissingDataSourceSnafu,
    UnknownFactSnafu,
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
    /// Returns [`FactbaseError::UnknownFact`] for dangling references and
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
            // fact; validating only `inputs` lets references that the evaluator
            // will actually use slip through as `UnknownFact` at resolve time.
            if let Source::Derived { formula, .. } = &fact.source {
                for ref_id in expr_fact_ids(formula) {
                    if !self.facts.contains_key(ref_id) {
                        return UnknownFactSnafu {
                            id: ref_id.as_str(),
                            referenced_by: format!("formula of fact {}", fact.id),
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
                Source::Derived { formula, .. } => evaluate_expr(formula, &resolved)?,
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
            _ => {}
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
) -> Result<Scalar, FactbaseError> {
    match expr {
        Expr::Add { a, b } => binary_op(a, b, resolved, "+", |x, y| x + y, i64::checked_add),
        Expr::Sub { a, b } => binary_op(a, b, resolved, "-", |x, y| x - y, i64::checked_sub),
        Expr::Mul { a, b } => binary_op(a, b, resolved, "*", |x, y| x * y, i64::checked_mul),
        Expr::Div { a, b } => {
            let left = lookup(a, resolved)?;
            let right = lookup(b, resolved)?;
            let lf = scalar_to_f64(&left.value)?;
            let rf = scalar_to_f64(&right.value)?;
            if rf == 0.0 {
                return BadDerivedSnafu {
                    detail: format!("division by zero: {a} / {b}"),
                }
                .fail();
            }
            Ok(Scalar::Ratio { value: lf / rf })
        }
        Expr::Sum { terms } => {
            let mut iter = terms.iter();
            let Some(head_id) = iter.next() else {
                return Ok(Scalar::Count { value: 0 });
            };
            let head = lookup(head_id, resolved)?;
            let mut acc = head.value.clone();
            for term_id in iter {
                let next = lookup(term_id, resolved)?;
                acc = add_scalars(&acc, &next.value)?;
            }
            Ok(acc)
        }
    }
}

fn lookup<'a>(
    id: &FactId,
    resolved: &'a BTreeMap<FactId, ResolvedFact>,
) -> Result<&'a ResolvedFact, FactbaseError> {
    resolved.get(id).ok_or_else(|| FactbaseError::UnknownFact {
        id: id.as_str().to_owned(),
        referenced_by: "derived expression".to_owned(),
    })
}

fn binary_op(
    lhs_id: &FactId,
    rhs_id: &FactId,
    resolved: &BTreeMap<FactId, ResolvedFact>,
    op_name: &str,
    f_op: fn(f64, f64) -> f64,
    i_op: fn(i64, i64) -> Option<i64>,
) -> Result<Scalar, FactbaseError> {
    let lhs = lookup(lhs_id, resolved)?;
    let rhs = lookup(rhs_id, resolved)?;
    match (&lhs.value, &rhs.value) {
        (Scalar::Count { value: lv }, Scalar::Count { value: rv }) => {
            let summed = i_op(*lv, *rv).ok_or_else(|| FactbaseError::BadDerived {
                detail: format!("integer overflow in {lhs_id} {op_name} {rhs_id}"),
            })?;
            Ok(Scalar::Count { value: summed })
        }
        (Scalar::Money { value: lv }, Scalar::Money { value: rv }) => match op_name {
            "+" => Ok(Scalar::Money {
                value: Money::from_micros(lv.micros() + rv.micros()),
            }),
            "-" => Ok(Scalar::Money {
                value: Money::from_micros(lv.micros() - rv.micros()),
            }),
            _ => BadDerivedSnafu {
                detail: format!("operator {op_name} not defined on money pair {lhs_id}, {rhs_id}"),
            }
            .fail(),
        },
        (Scalar::Ratio { value: lv }, Scalar::Ratio { value: rv }) => Ok(Scalar::Ratio {
            value: f_op(*lv, *rv),
        }),
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
        (Scalar::Money { value: lv }, Scalar::Money { value: rv }) => Ok(Scalar::Money {
            value: Money::from_micros(lv.micros() + rv.micros()),
        }),
        (Scalar::Ratio { value: lv }, Scalar::Ratio { value: rv }) => {
            Ok(Scalar::Ratio { value: lv + rv })
        }
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
mod tests {
    use super::*;
    use jiff::Timestamp;

    fn ts() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn manual_fact(id: &str, value: Scalar, unit: Unit) -> Fact {
        Fact {
            id: FactId::new(id).unwrap(),
            value,
            unit,
            source: Source::Manual {
                note: "test".to_owned(),
                captured_by: "tester".to_owned(),
            },
            captured: ts(),
        }
    }

    #[test]
    fn empty_factbase_validates() {
        let fb = Factbase::new();
        assert!(fb.validate().is_ok());
        let resolved = fb.resolve(&DataSourceRegistry::new()).unwrap();
        assert!(resolved.is_empty());
    }

    #[test]
    fn manual_facts_resolve_to_authored_value() {
        let mut fb = Factbase::new();
        fb.add_fact(manual_fact("a", Scalar::Count { value: 7 }, Unit::Count));
        let resolved = fb.resolve(&DataSourceRegistry::new()).unwrap();
        let a = resolved.get(&FactId::new("a").unwrap()).unwrap();
        assert_eq!(a.value, Scalar::Count { value: 7 });
    }

    #[test]
    fn derived_sum_resolves() {
        let mut fb = Factbase::new();
        fb.add_fact(manual_fact("a", Scalar::Count { value: 3 }, Unit::Count));
        fb.add_fact(manual_fact("b", Scalar::Count { value: 4 }, Unit::Count));
        fb.add_fact(Fact {
            id: FactId::new("total").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Derived {
                formula: Expr::Add {
                    a: FactId::new("a").unwrap(),
                    b: FactId::new("b").unwrap(),
                },
                inputs: vec![FactId::new("a").unwrap(), FactId::new("b").unwrap()],
            },
            captured: ts(),
        });
        let resolved = fb.resolve(&DataSourceRegistry::new()).unwrap();
        let total = resolved.get(&FactId::new("total").unwrap()).unwrap();
        assert_eq!(total.value, Scalar::Count { value: 7 });
    }

    #[test]
    fn reference_resolves_to_target_value() {
        let mut fb = Factbase::new();
        fb.add_fact(manual_fact(
            "source",
            Scalar::Count { value: 42 },
            Unit::Count,
        ));
        fb.add_fact(Fact {
            id: FactId::new("alias").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Reference {
                fact: FactId::new("source").unwrap(),
            },
            captured: ts(),
        });
        let resolved = fb.resolve(&DataSourceRegistry::new()).unwrap();
        let alias = resolved.get(&FactId::new("alias").unwrap()).unwrap();
        assert_eq!(alias.value, Scalar::Count { value: 42 });
    }

    #[test]
    fn cycle_is_detected_with_path_in_error() {
        let mut fb = Factbase::new();
        fb.add_fact(Fact {
            id: FactId::new("a").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Reference {
                fact: FactId::new("b").unwrap(),
            },
            captured: ts(),
        });
        fb.add_fact(Fact {
            id: FactId::new("b").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Reference {
                fact: FactId::new("a").unwrap(),
            },
            captured: ts(),
        });
        let err = fb.validate().expect_err("cycle must be detected");
        let path = match err {
            FactbaseError::Cycle { path } => path,
            other => panic!("expected Cycle, got {other:?}"),
        };
        assert!(path.contains(&"a".to_owned()));
        assert!(path.contains(&"b".to_owned()));
    }

    #[test]
    fn unknown_reference_rejects_with_named_id() {
        let mut fb = Factbase::new();
        fb.add_fact(Fact {
            id: FactId::new("orphan").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Reference {
                fact: FactId::new("missing").unwrap(),
            },
            captured: ts(),
        });
        let err = fb.validate().expect_err("dangling reference must reject");
        assert!(matches!(err, FactbaseError::UnknownFact { id, .. } if id == "missing"));
    }

    #[test]
    fn unknown_claim_target_rejects() {
        let mut fb = Factbase::new();
        fb.add_claim(Claim {
            id: ClaimId::new("c1").unwrap(),
            text: "x is 1".to_owned(),
            asserts: FactId::new("absent").unwrap(),
            location: Location {
                at: "deck/slide/1".to_owned(),
            },
            tolerance: Tolerance::STRICT,
        });
        let err = fb.validate().expect_err("claim of absent fact rejects");
        assert!(matches!(err, FactbaseError::UnknownFact { id, .. } if id == "absent"));
    }

    #[test]
    fn sql_without_adapter_rejects_with_named_data_source() {
        let mut fb = Factbase::new();
        fb.add_fact(Fact {
            id: FactId::new("from_db").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Sql {
                data_source: DataSourceId::new("redshift_prod").unwrap(),
                query: "SELECT 1".to_owned(),
                table: "totals".to_owned(),
            },
            captured: ts(),
        });
        let err = fb
            .resolve(&DataSourceRegistry::new())
            .expect_err("missing adapter");
        match err {
            FactbaseError::MissingDataSource { data_source, .. } => {
                assert_eq!(data_source, "redshift_prod");
            }
            other => panic!("expected MissingDataSource, got {other:?}"),
        }
    }

    #[test]
    fn type_mismatch_in_derived_rejects() {
        let mut fb = Factbase::new();
        fb.add_fact(manual_fact(
            "count",
            Scalar::Count { value: 3 },
            Unit::Count,
        ));
        fb.add_fact(manual_fact(
            "money",
            Scalar::Money {
                value: Money::from_units(7).expect("in range"),
            },
            Unit::Usd,
        ));
        fb.add_fact(Fact {
            id: FactId::new("mix").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Derived {
                formula: Expr::Add {
                    a: FactId::new("count").unwrap(),
                    b: FactId::new("money").unwrap(),
                },
                inputs: vec![FactId::new("count").unwrap(), FactId::new("money").unwrap()],
            },
            captured: ts(),
        });
        let err = fb
            .resolve(&DataSourceRegistry::new())
            .expect_err("type mismatch");
        assert!(matches!(err, FactbaseError::DerivedTypeMismatch { .. }));
    }

    #[test]
    fn walk_chain_leaf_fact() {
        let mut fb = Factbase::new();
        fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
        let chain = fb.walk_citation_chain(&FactId::new("a").unwrap());
        assert!(chain.is_empty());
    }

    #[test]
    fn walk_chain_derived_returns_leaves_first() {
        let mut fb = Factbase::new();
        let id_a = FactId::new("a").unwrap();
        let id_b = FactId::new("b").unwrap();
        fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
        fb.add_fact(Fact {
            id: id_b.clone(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Derived {
                formula: Expr::Add {
                    a: id_a.clone(),
                    b: id_a.clone(),
                },
                inputs: vec![id_a.clone()],
            },
            captured: ts(),
        });
        let chain = fb.walk_citation_chain(&id_b);
        assert_eq!(chain, vec![id_a]);
    }

    #[test]
    fn walk_chain_diamond() {
        let mut fb = Factbase::new();
        let id_a = FactId::new("a").unwrap();
        let id_b = FactId::new("b").unwrap();
        let id_c = FactId::new("c").unwrap();
        let id_d = FactId::new("d").unwrap();
        fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
        fb.add_fact(Fact {
            id: id_b.clone(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Derived {
                formula: Expr::Add {
                    a: id_a.clone(),
                    b: id_a.clone(),
                },
                inputs: vec![id_a.clone()],
            },
            captured: ts(),
        });
        fb.add_fact(Fact {
            id: id_c.clone(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Derived {
                formula: Expr::Add {
                    a: id_a.clone(),
                    b: id_a.clone(),
                },
                inputs: vec![id_a.clone()],
            },
            captured: ts(),
        });
        fb.add_fact(Fact {
            id: id_d.clone(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Derived {
                formula: Expr::Add {
                    a: id_b.clone(),
                    b: id_c.clone(),
                },
                inputs: vec![id_b.clone(), id_c.clone()],
            },
            captured: ts(),
        });
        let chain = fb.walk_citation_chain(&id_d);
        assert_eq!(chain, vec![id_a, id_b, id_c]);
    }

    #[test]
    fn walk_chain_unknown_root() {
        let fb = Factbase::new();
        let chain = fb.walk_citation_chain(&FactId::new("ghost").unwrap());
        assert!(chain.is_empty());
    }

    #[test]
    fn claim_citation_chain_returns_fact_chain() {
        let mut fb = Factbase::new();
        let id_a = FactId::new("a").unwrap();
        let id_b = FactId::new("b").unwrap();
        fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
        fb.add_fact(Fact {
            id: id_b.clone(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Derived {
                formula: Expr::Add {
                    a: id_a.clone(),
                    b: id_a.clone(),
                },
                inputs: vec![id_a.clone()],
            },
            captured: ts(),
        });
        fb.add_claim(Claim {
            id: ClaimId::new("c1").unwrap(),
            text: "b is 1".to_owned(),
            asserts: id_b.clone(),
            location: Location {
                at: "deck/slide/1".to_owned(),
            },
            tolerance: Tolerance::STRICT,
        });
        let chain = fb.claim_citation_chain(&ClaimId::new("c1").unwrap());
        assert_eq!(chain, Some(vec![id_a]));
    }

    #[test]
    fn claim_citation_chain_missing_claim() {
        let fb = Factbase::new();
        let chain = fb.claim_citation_chain(&ClaimId::new("ghost").unwrap());
        assert_eq!(chain, None);
    }

    #[test]
    fn derived_formula_unknown_fact_rejects_even_when_inputs_valid() {
        let mut fb = Factbase::new();
        fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
        fb.add_fact(Fact {
            id: FactId::new("d").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Derived {
                formula: Expr::Add {
                    a: FactId::new("a").unwrap(),
                    b: FactId::new("b").unwrap(),
                },
                inputs: vec![FactId::new("a").unwrap()],
            },
            captured: ts(),
        });
        let err = fb
            .validate()
            .expect_err("formula ref outside inputs must reject");
        assert!(matches!(err, FactbaseError::UnknownFact { id, .. } if id == "b"));
    }
}
