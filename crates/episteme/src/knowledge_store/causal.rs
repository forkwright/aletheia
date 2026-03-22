//! Causal edge operations for the knowledge graph.
//!
//! Supports inserting causal edges between fact nodes, querying direct
//! effects/causes, and propagating confidence through causal chains.

#[cfg(feature = "mneme-engine")]
use snafu::ResultExt;
use tracing::instrument;

use super::{KnowledgeStore, queries};

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Insert a causal edge between two fact nodes.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidConfidence`](crate::error::Error::InvalidConfidence) if
    /// confidence is outside `[0.0, 1.0]`.
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the write fails.
    #[instrument(skip(self, edge), fields(cause = %edge.cause, effect = %edge.effect))]
    pub fn insert_causal_edge(
        &self,
        edge: &crate::knowledge::CausalEdge,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use snafu::ensure;

        use crate::engine::DataValue;

        ensure!(
            (0.0..=1.0).contains(&edge.confidence),
            crate::error::InvalidConfidenceSnafu {
                value: edge.confidence
            }
        );

        let now = crate::knowledge::format_timestamp(&edge.created_at);
        let mut params = BTreeMap::new();
        params.insert(
            "cause".to_owned(),
            DataValue::Str(edge.cause.as_str().into()),
        );
        params.insert(
            "effect".to_owned(),
            DataValue::Str(edge.effect.as_str().into()),
        );
        params.insert(
            "ordering".to_owned(),
            DataValue::Str(edge.ordering.as_str().into()),
        );
        params.insert("confidence".to_owned(), DataValue::from(edge.confidence));
        params.insert("created_at".to_owned(), DataValue::Str(now.into()));
        self.run_mut(&queries::upsert_causal_edge(), params)
    }

    /// Remove a causal edge.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the deletion fails.
    #[instrument(skip(self))]
    pub fn remove_causal_edge(
        &self,
        cause: &crate::id::FactId,
        effect: &crate::id::FactId,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("cause".to_owned(), DataValue::Str(cause.as_str().into()));
        params.insert("effect".to_owned(), DataValue::Str(effect.as_str().into()));
        self.run_mut(&queries::rm_causal_edge(), params)
    }

    /// Query all direct effects of a given cause fact.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the query fails.
    #[instrument(skip(self))]
    pub fn query_effects(
        &self,
        cause_id: &crate::id::FactId,
    ) -> crate::error::Result<Vec<crate::knowledge::CausalEdge>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"
            ?[cause, effect, ordering, confidence, created_at] :=
                *causal_edges{cause, effect, ordering, confidence, created_at},
                cause = $cause
        ";
        let mut params = BTreeMap::new();
        params.insert("cause".to_owned(), DataValue::Str(cause_id.as_str().into()));
        let rows = self.run_read(script, params)?;
        rows_to_causal_edges(&rows)
    }

    /// Query all direct causes of a given effect fact.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the query fails.
    #[instrument(skip(self))]
    pub fn query_causes(
        &self,
        effect_id: &crate::id::FactId,
    ) -> crate::error::Result<Vec<crate::knowledge::CausalEdge>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"
            ?[cause, effect, ordering, confidence, created_at] :=
                *causal_edges{cause, effect, ordering, confidence, created_at},
                effect = $effect
        ";
        let mut params = BTreeMap::new();
        params.insert(
            "effect".to_owned(),
            DataValue::Str(effect_id.as_str().into()),
        );
        let rows = self.run_read(script, params)?;
        rows_to_causal_edges(&rows)
    }

    /// List all causal edges in the graph.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the query fails.
    #[instrument(skip(self))]
    pub fn list_causal_edges(&self) -> crate::error::Result<Vec<crate::knowledge::CausalEdge>> {
        use std::collections::BTreeMap;

        let script = r"
            ?[cause, effect, ordering, confidence, created_at] :=
                *causal_edges{cause, effect, ordering, confidence, created_at}
            :order created_at
        ";
        let rows = self.run_read(script, BTreeMap::new())?;
        rows_to_causal_edges(&rows)
    }

    /// Compute the propagated confidence through a causal chain from `start` to `end`.
    ///
    /// Uses BFS to find the shortest causal path, then returns the product of
    /// edge confidences along that path. Returns `None` if no path exists.
    ///
    /// Transitive confidence = product of individual edge confidences along the chain.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if any query fails.
    #[instrument(skip(self))]
    pub fn propagate_confidence(
        &self,
        start: &crate::id::FactId,
        end: &crate::id::FactId,
    ) -> crate::error::Result<Option<f64>> {
        use std::collections::{HashMap, HashSet, VecDeque};

        if start == end {
            return Ok(Some(1.0));
        }

        // BFS through causal edges, tracking confidence at each edge.
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, f64)> = VecDeque::new();
        // parent map: child -> (parent, edge_confidence)
        let mut parent: HashMap<String, (String, f64)> = HashMap::new();

        visited.insert(start.as_str().to_owned());
        queue.push_back((start.as_str().to_owned(), 1.0));

        while let Some((current, _)) = queue.pop_front() {
            let current_id =
                crate::id::FactId::new(current.as_str()).context(crate::error::InvalidIdSnafu)?;
            let effects = self.query_effects(&current_id)?;

            for edge in effects {
                let effect_str = edge.effect.as_str().to_owned();
                if visited.contains(&effect_str) {
                    continue;
                }
                visited.insert(effect_str.clone());
                parent.insert(effect_str.clone(), (current.clone(), edge.confidence));

                if effect_str == end.as_str() {
                    // Reconstruct path and compute product of confidences.
                    let mut confidence = 1.0;
                    let mut node = end.as_str().to_owned();
                    while let Some((prev, edge_conf)) = parent.get(&node) {
                        confidence *= edge_conf;
                        node = prev.clone();
                    }
                    return Ok(Some(confidence));
                }

                queue.push_back((effect_str, edge.confidence));
            }
        }

        Ok(None)
    }
}

/// Convert Datalog result rows to `CausalEdge` structs.
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
fn rows_to_causal_edges(
    rows: &crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::CausalEdge>> {
    use super::marshal::{extract_float, extract_str};

    let mut edges = Vec::new();
    for row in &rows.rows {
        if row.len() < 5 {
            continue;
        }
        let cause_str = extract_str(&row[0])?;
        let effect_str = extract_str(&row[1])?;
        let ordering_str = extract_str(&row[2])?;
        let confidence = extract_float(&row[3])?;
        let created_at_str = extract_str(&row[4])?;

        let ordering = ordering_str
            .parse::<crate::knowledge::TemporalOrdering>()
            .unwrap_or(crate::knowledge::TemporalOrdering::Before);

        let created_at =
            crate::knowledge::parse_timestamp(&created_at_str).unwrap_or_else(jiff::Timestamp::now);

        let cause =
            crate::id::FactId::new(cause_str.as_str()).context(crate::error::InvalidIdSnafu)?;
        let effect =
            crate::id::FactId::new(effect_str.as_str()).context(crate::error::InvalidIdSnafu)?;
        edges.push(crate::knowledge::CausalEdge {
            cause,
            effect,
            ordering,
            confidence,
            created_at,
        });
    }
    Ok(edges)
}
