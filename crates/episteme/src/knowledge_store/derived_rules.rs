//! Derived-rule materialization for the knowledge store.
//!
//! Implements [`KnowledgeStore`] methods that evaluate Datalog rule sets
//! and write the resulting derived facts into the `derived_facts` relation.
//!
//! Three rule families are supported:
//!
//! 1. **Ontological** — transitive IS-A closure over `type_hierarchy`.
//! 2. **Causal chain** — recursive transitive closure over `causal_edges`
//!    with multiplicative confidence decay.
//! 3. **Defeasible defaults** — entity-scoped defaults from `defaults` that
//!    are suppressed when a `verified` fact overrides the same tag.
//!
//! Call [`KnowledgeStore::materialize_derived_facts`] to run all three rule
//! sets in sequence. Individual rule-set methods are exposed as
//! `pub(crate)` for targeted testing.

use std::collections::BTreeMap;

use tracing::instrument;

use super::KnowledgeStore;
use crate::engine::{DataValue, ScriptMutability};

/// A single row produced by a materialization pass.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedFact {
    /// The entity this derived fact is about.
    pub entity_id: String,
    /// The rule that produced this fact. One of [`crate::derived_rules::RULE_IDS`].
    pub rule_id: String,
    /// The inferred content string (format depends on rule family).
    pub derived_content: String,
    /// Confidence score in `[0.0, 1.0]`.
    pub confidence: f64,
}

// ── Type-hierarchy helpers ─────────────────────────────────────────────────────

impl KnowledgeStore {
    /// Insert an IS-A edge into the `type_hierarchy` relation.
    ///
    /// Inserts `child_type IS-A parent_type`. Both values are free-form
    /// strings matching entity `entity_type` values in the `entities` relation.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the write fails.
    #[instrument(skip(self))]
    pub fn insert_type_hierarchy(
        &self,
        child_type: &str,
        parent_type: &str,
    ) -> crate::error::Result<()> {
        let now = jiff::Timestamp::now().to_string();
        let mut params = BTreeMap::new();
        params.insert("child_type".to_owned(), DataValue::Str(child_type.into()));
        params.insert("parent_type".to_owned(), DataValue::Str(parent_type.into()));
        params.insert("created_at".to_owned(), DataValue::Str(now.into()));
        self.run_mut(
            r"?[child_type, parent_type, created_at] <- [[$child_type, $parent_type, $created_at]]
            :put type_hierarchy { child_type, parent_type => created_at }",
            params,
        )
    }

    /// Insert a defeasible default assertion for an entity+tag.
    ///
    /// The `tag` identifies the topic area (used for override matching).
    /// The `default_content` is the assertion text.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the write fails.
    #[instrument(skip(self))]
    pub fn insert_default(
        &self,
        entity_id: &str,
        tag: &str,
        default_content: &str,
        confidence: f64,
    ) -> crate::error::Result<()> {
        let now = jiff::Timestamp::now().to_string();
        let mut params = BTreeMap::new();
        params.insert("entity_id".to_owned(), DataValue::Str(entity_id.into()));
        params.insert("tag".to_owned(), DataValue::Str(tag.into()));
        params.insert(
            "default_content".to_owned(),
            DataValue::Str(default_content.into()),
        );
        params.insert("confidence".to_owned(), DataValue::from(confidence));
        params.insert("created_at".to_owned(), DataValue::Str(now.into()));
        self.run_mut(
            r"?[entity_id, tag, default_content, confidence, created_at] <-
                [[$entity_id, $tag, $default_content, $confidence, $created_at]]
            :put defaults { entity_id, tag => default_content, confidence, created_at }",
            params,
        )
    }

    // ── Materialization ────────────────────────────────────────────────────────

    /// Materialize all derived-rule sets and persist results to `derived_facts`.
    ///
    /// Runs ontological IS-A closure, transitive causal chains, and defeasible
    /// defaults in sequence. Existing `derived_facts` rows are replaced
    /// (`:put` semantics — upsert by `(entity_id, rule_id, derived_content)` key).
    ///
    /// # Errors
    ///
    /// Returns the first error encountered across all rule-set materializations.
    #[instrument(skip(self))]
    pub fn materialize_derived_facts(&self) -> crate::error::Result<usize> {
        let mut total = 0usize;
        total += self.materialize_ontological_rules()?;
        total += self.materialize_causal_chain_rules()?;
        total += self.materialize_defeasible_rules()?;
        Ok(total)
    }

    /// Materialize ontological IS-A closure into `derived_facts`.
    ///
    /// For every entity whose `entity_type` participates in the `type_hierarchy`
    /// relation, emits a `derived_facts` row for each ancestor type reachable
    /// via transitive IS-A traversal.
    ///
    /// Returns the number of derived rows written.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if query or
    /// write fails.
    #[instrument(skip(self))]
    pub(crate) fn materialize_ontological_rules(&self) -> crate::error::Result<usize> {
        // Step 1: query derived rows from the recursive rule.
        let rows = self.run_read(
            crate::derived_rules::ONTOLOGICAL_MATERIALIZATION,
            BTreeMap::new(),
        )?;
        let derived = parse_derived_rows(&rows)?;

        // Step 2: persist each row with upsert semantics.
        let count = derived.len();
        let now = jiff::Timestamp::now().to_string();
        for fact in &derived {
            self.put_derived_fact(fact, &now)?;
        }
        Ok(count)
    }

    /// Materialize transitive causal chains into `derived_facts`.
    ///
    /// Traverses `causal_edges` recursively (up to the engine's fixpoint), emitting
    /// one derived row per (cause, reachable-effect) pair where the confidence
    /// product exceeds 0.05. Existing rows are upserted.
    ///
    /// Returns the number of derived rows written.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if query or write fails.
    #[instrument(skip(self))]
    pub(crate) fn materialize_causal_chain_rules(&self) -> crate::error::Result<usize> {
        let rows = self.run_read(
            crate::derived_rules::CAUSAL_CHAIN_MATERIALIZATION,
            BTreeMap::new(),
        )?;
        let derived = parse_derived_rows(&rows)?;
        let count = derived.len();
        let now = jiff::Timestamp::now().to_string();
        for fact in &derived {
            self.put_derived_fact(fact, &now)?;
        }
        Ok(count)
    }

    /// Materialize defeasible defaults into `derived_facts`.
    ///
    /// For each entry in `defaults`, checks whether a `verified`-tier fact for
    /// the same entity already covers the tag. If no override exists, emits a
    /// derived row. Entity-scoped: a verified fact for entity A does not suppress
    /// a default for entity B.
    ///
    /// Returns the number of derived rows written.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if query or write fails.
    #[instrument(skip(self))]
    pub(crate) fn materialize_defeasible_rules(&self) -> crate::error::Result<usize> {
        let rows = self.run_read(
            crate::derived_rules::DEFEASIBLE_SCOPED_MATERIALIZATION,
            BTreeMap::new(),
        )?;
        let derived = parse_derived_rows(&rows)?;
        let count = derived.len();
        let now = jiff::Timestamp::now().to_string();
        for fact in &derived {
            self.put_derived_fact(fact, &now)?;
        }
        Ok(count)
    }

    /// Query all derived facts for an entity.
    ///
    /// Returns all `derived_facts` rows whose `entity_id` matches. Ordered by
    /// confidence descending.
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the query fails.
    #[instrument(skip(self))]
    pub fn query_derived_facts(&self, entity_id: &str) -> crate::error::Result<Vec<DerivedFact>> {
        let mut params = BTreeMap::new();
        params.insert("entity_id".to_owned(), DataValue::Str(entity_id.into()));
        let rows = self.run_read(
            r"?[entity_id, rule_id, derived_content, confidence] :=
                *derived_facts{entity_id, rule_id, derived_content, confidence},
                entity_id = $entity_id
            :order -confidence",
            params,
        )?;
        parse_derived_rows(&rows)
    }

    /// Query derived facts for an entity filtered by rule family.
    ///
    /// `rule_prefix` matches against the `rule_id` field with prefix semantics
    /// (e.g. `"ontological"` matches `"ontological:is_a"`).
    ///
    /// # Errors
    ///
    /// Returns [`EngineQuery`](crate::error::Error::EngineQuery) if the query fails.
    #[instrument(skip(self))]
    pub fn query_derived_facts_by_rule(
        &self,
        entity_id: &str,
        rule_prefix: &str,
    ) -> crate::error::Result<Vec<DerivedFact>> {
        let mut params = BTreeMap::new();
        params.insert("entity_id".to_owned(), DataValue::Str(entity_id.into()));
        params.insert("rule_prefix".to_owned(), DataValue::Str(rule_prefix.into()));
        let rows = self.run_read(
            r"?[entity_id, rule_id, derived_content, confidence] :=
                *derived_facts{entity_id, rule_id, derived_content, confidence},
                entity_id = $entity_id,
                starts_with(rule_id, $rule_prefix)
            :order -confidence",
            params,
        )?;
        parse_derived_rows(&rows)
    }

    // ── Internal helpers ───────────────────────────────────────────────────────

    /// Upsert a single derived fact row.
    fn put_derived_fact(&self, fact: &DerivedFact, now: &str) -> crate::error::Result<()> {
        let mut params = BTreeMap::new();
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(fact.entity_id.as_str().into()),
        );
        params.insert(
            "rule_id".to_owned(),
            DataValue::Str(fact.rule_id.as_str().into()),
        );
        params.insert(
            "derived_content".to_owned(),
            DataValue::Str(fact.derived_content.as_str().into()),
        );
        params.insert("confidence".to_owned(), DataValue::from(fact.confidence));
        params.insert("materialized_at".to_owned(), DataValue::Str(now.into()));
        self.db
            .run(
                r"?[entity_id, rule_id, derived_content, confidence, materialized_at] <-
                    [[$entity_id, $rule_id, $derived_content, $confidence, $materialized_at]]
                :put derived_facts {
                    entity_id, rule_id, derived_content =>
                    confidence, materialized_at
                }",
                params,
                ScriptMutability::Mutable,
            )
            .map(|_| ())
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }
}

// ── Row parsing ────────────────────────────────────────────────────────────────

/// Parse Datalog result rows into [`DerivedFact`] structs.
///
/// Expected column order: `entity_id, rule_id, derived_content, confidence`.
#[expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: direct row indexing throughout — row width validated by column-count guard"
)]
fn parse_derived_rows(rows: &crate::engine::NamedRows) -> crate::error::Result<Vec<DerivedFact>> {
    use super::marshal::{extract_float, extract_str};

    let mut out = Vec::with_capacity(rows.rows.len());
    for row in &rows.rows {
        if row.len() < 4 {
            continue;
        }
        out.push(DerivedFact {
            entity_id: extract_str(&row[0])?,
            rule_id: extract_str(&row[1])?,
            derived_content: extract_str(&row[2])?,
            confidence: extract_float(&row[3])?,
        });
    }
    Ok(out)
}
