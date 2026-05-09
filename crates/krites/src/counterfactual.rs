//! Counterfactual reasoning queries over causal edge graphs.
#![expect(
    clippy::indexing_slicing,
    reason = "row indices validated by length check above"
)]
#![expect(
    clippy::result_large_err,
    reason = "crate-wide Error type — boxing deferred to avoid API churn"
)]
//!
//! Provides three standard query patterns against a `causal_edges` relation:
//! - **Dependency analysis**: all transitive causes of a fact.
//! - **Impact analysis**: all transitive effects of a fact.
//! - **Minimal provenance**: the smallest subgraph justifying a conclusion.
//!
//! The `causal_edges` relation is expected to have the schema:
//! ```text
//! :create causal_edges {
//!     cause: String, effect: String =>
//!     ordering: String,
//!     relationship_type: String,
//!     confidence: Float,
//!     created_at: String
//! }
//! ```

use std::collections::BTreeMap;

use eidos::knowledge::CausalRelationType;

use snafu::OptionExt;

use crate::{DataValue, Db, NamedRows, Result};

/// A single causal edge returned by counterfactual queries.
#[derive(Debug, Clone, PartialEq)]
pub struct CausalEdgeRow {
    /// Cause fact ID.
    pub cause: String,
    /// Effect fact ID.
    pub effect: String,
    /// Relationship type (caused, enabled, prevented, correlated).
    pub relationship_type: CausalRelationType,
    /// Edge confidence in `[0.0, 1.0]`.
    pub confidence: f64,
}

/// Parse a [`NamedRows`] result into a vector of [`CausalEdgeRow`].
///
/// Expects columns in the order: `cause, effect, relationship_type, confidence`.
fn parse_edge_rows(rows: &NamedRows) -> Result<Vec<CausalEdgeRow>> {
    let mut edges = Vec::new();
    for row in &rows.rows {
        if row.len() < 4 {
            continue;
        }
        let cause = extract_str(&row[0])?;
        let effect = extract_str(&row[1])?;
        let rel_type_str = extract_str(&row[2])?;
        let confidence = extract_f64(&row[3])?;

        let relationship_type = rel_type_str.parse::<CausalRelationType>().ok().context(
            crate::error::UnknownCausalRelationSnafu {
                input: rel_type_str,
            },
        )?;

        edges.push(CausalEdgeRow {
            cause,
            effect,
            relationship_type,
            confidence,
        });
    }
    Ok(edges)
}

fn extract_str(v: &DataValue) -> Result<String> {
    match v {
        DataValue::Str(s) => Ok(s.to_string()),
        other => crate::error::EngineSnafu {
            message: format!("expected string value, got {other:?}"),
        }
        .fail(),
    }
}

fn extract_f64(v: &DataValue) -> Result<f64> {
    match v {
        DataValue::Num(crate::data::value::Num::Float(f)) => Ok(*f),
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "confidence values are small floats; integer cast is a safe fallback"
        )]
        DataValue::Num(crate::data::value::Num::Int(i)) => Ok(*i as f64),
        other => crate::error::EngineSnafu {
            message: format!("expected numeric value, got {other:?}"),
        }
        .fail(),
    }
}

/// Typed query builders for counterfactual reasoning over causal graphs.
pub struct Counterfactual;

impl Counterfactual {
    /// Dependency analysis — all facts that `fact_id` transitively depends on.
    ///
    /// Returns every edge in the transitive closure of causes for `fact_id`
    /// (direct and indirect).  The `effect` of each row is a node in the
    /// dependency chain and `cause` is its immediate predecessor.
    pub fn dependency_analysis(db: &Db, fact_id: impl AsRef<str>) -> Result<Vec<CausalEdgeRow>> {
        let mut params = BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            DataValue::Str(fact_id.as_ref().into()),
        );
        let rows = db.run_read_only(DEPENDENCY_ANALYSIS, params)?;
        parse_edge_rows(&rows)
    }

    /// Impact analysis — all facts that transitively depend on `fact_id`.
    ///
    /// Returns every edge in the transitive closure of effects for `fact_id`
    /// (direct and indirect).  The `cause` of each row is a node in the
    /// impact chain and `effect` is its immediate successor.
    pub fn impact_analysis(db: &Db, fact_id: impl AsRef<str>) -> Result<Vec<CausalEdgeRow>> {
        let mut params = BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            DataValue::Str(fact_id.as_ref().into()),
        );
        let rows = db.run_read_only(IMPACT_ANALYSIS, params)?;
        parse_edge_rows(&rows)
    }

    /// Minimal provenance — the subgraph justifying `conclusion_id`.
    ///
    /// Returns all causal edges that lie on a path from a root cause to
    /// `conclusion_id`.  This is the smallest subgraph that explains why the
    /// conclusion holds.
    pub fn minimal_provenance(
        db: &Db,
        conclusion_id: impl AsRef<str>,
    ) -> Result<Vec<CausalEdgeRow>> {
        let mut params = BTreeMap::new();
        params.insert(
            "conclusion_id".to_owned(),
            DataValue::Str(conclusion_id.as_ref().into()),
        );
        let rows = db.run_read_only(MINIMAL_PROVENANCE, params)?;
        parse_edge_rows(&rows)
    }
}

const DEPENDENCY_ANALYSIS: &str = r"
    target[] <- [[$fact_id]]

    dep_edge[effect, cause, rel_type, conf] :=
        *causal_edges[cause, effect, ordering, rel_type, conf, created_at]

    dep_reach[effect, cause] := dep_edge[effect, cause, _, _]
    dep_reach[effect, root] := dep_reach[effect, mid], dep_edge[mid, root, _, _]

    all_nodes[node] := target[node]
    all_nodes[node] := target[start], dep_reach[start, node]

    ?[cause, effect, relationship_type, confidence] :=
        all_nodes[effect],
        dep_edge[effect, cause, relationship_type, confidence]
";

const IMPACT_ANALYSIS: &str = r"
    target[] <- [[$fact_id]]

    imp_edge[cause, effect, rel_type, conf] :=
        *causal_edges[cause, effect, ordering, rel_type, conf, created_at]

    imp_reach[cause, effect] := imp_edge[cause, effect, _, _]
    imp_reach[cause, root] := imp_reach[cause, mid], imp_edge[mid, root, _, _]

    all_nodes[node] := target[node]
    all_nodes[node] := target[start], imp_reach[start, node]

    ?[cause, effect, relationship_type, confidence] :=
        all_nodes[cause],
        imp_edge[cause, effect, relationship_type, confidence]
";

const MINIMAL_PROVENANCE: &str = r"
    prov_edge[cause, effect, rel_type, conf] :=
        *causal_edges[cause, effect, ordering, rel_type, conf, created_at]

    prov_node[x] := x = $conclusion_id
    prov_node[c] := prov_node[e], prov_edge[c, e, _, _]

    ?[cause, effect, relationship_type, confidence] :=
        prov_node[effect],
        prov_edge[cause, effect, relationship_type, confidence],
        prov_node[cause]
";
