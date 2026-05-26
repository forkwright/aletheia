#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use super::{KNOWLEDGE_DDL, KnowledgeStore, fts_ddl};

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    #[expect(
        clippy::too_many_lines,
        reason = "migration is a single linear sequence"
    )]
    pub(super) fn migrate_v1_to_v2(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v1 -> v2");

        let all_facts = self
            .db
            .run(
                r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 read facts: {e}"),
                }
                .build()
            })?;

        let _ = self.db.run(
            "::fts drop facts:content_fts",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        );

        self.db
            .run("::remove facts", BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 remove facts: {e}"),
                }
                .build()
            })?;

        self.db
            .run(KNOWLEDGE_DDL[0], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 recreate facts: {e}"),
                }
                .build()
            })?;

        for row in &all_facts.rows {
            let script = r"
                ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                  superseded_by, source_session_id, recorded_at,
                  access_count, last_accessed_at, stability_hours, fact_type] <- [[
                    $id, $valid_from, $content, $nous_id, $confidence, $tier, $valid_to,
                    $superseded_by, $source_session_id, $recorded_at,
                    0, '', 720.0, ''
                ]]
                :put facts {id, valid_from => content, nous_id, confidence, tier,
                            valid_to, superseded_by, source_session_id, recorded_at,
                            access_count, last_accessed_at, stability_hours, fact_type}
            ";
            let mut params = BTreeMap::new();
            for (i, name) in [
                "id",
                "valid_from",
                "content",
                "nous_id",
                "confidence",
                "tier",
                "valid_to",
                "superseded_by",
                "source_session_id",
                "recorded_at",
            ]
            .iter()
            .enumerate()
            {
                if let Some(val) = row.get(i) {
                    params.insert((*name).to_owned(), val.clone());
                }
            }
            self.db
                .run(script, params, ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v1->v2 reinsert fact: {e}"),
                    }
                    .build()
                })?;
        }

        self.db
            .run(fts_ddl(), BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 recreate FTS: {e}"),
                }
                .build()
            })?;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v1 -> v2 complete");
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "migration is a single linear sequence"
    )]
    pub(super) fn migrate_v2_to_v3(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v2 -> v3");

        let all_facts = self
            .db
            .run(
                r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 read facts: {e}"),
                }
                .build()
            })?;

        let _ = self.db.run(
            "::fts drop facts:content_fts",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        );

        self.db
            .run("::remove facts", BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 remove facts: {e}"),
                }
                .build()
            })?;

        self.db
            .run(KNOWLEDGE_DDL[0], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 recreate facts: {e}"),
                }
                .build()
            })?;

        for row in &all_facts.rows {
            let script = r"
                ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                  superseded_by, source_session_id, recorded_at,
                  access_count, last_accessed_at, stability_hours, fact_type,
                  is_forgotten, forgotten_at, forget_reason] <- [[
                    $id, $valid_from, $content, $nous_id, $confidence, $tier, $valid_to,
                    $superseded_by, $source_session_id, $recorded_at,
                    $access_count, $last_accessed_at, $stability_hours, $fact_type,
                    false, null, null
                ]]
                :put facts {id, valid_from => content, nous_id, confidence, tier,
                            valid_to, superseded_by, source_session_id, recorded_at,
                            access_count, last_accessed_at, stability_hours, fact_type,
                            is_forgotten, forgotten_at, forget_reason}
            ";
            let mut params = BTreeMap::new();
            for (i, name) in [
                "id",
                "valid_from",
                "content",
                "nous_id",
                "confidence",
                "tier",
                "valid_to",
                "superseded_by",
                "source_session_id",
                "recorded_at",
                "access_count",
                "last_accessed_at",
                "stability_hours",
                "fact_type",
            ]
            .iter()
            .enumerate()
            {
                if let Some(val) = row.get(i) {
                    params.insert((*name).to_owned(), val.clone());
                }
            }
            self.db
                .run(script, params, ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v2->v3 reinsert fact: {e}"),
                    }
                    .build()
                })?;
        }

        self.db
            .run(fts_ddl(), BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 recreate FTS: {e}"),
                }
                .build()
            })?;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v2 -> v3 complete");
        Ok(())
    }

    /// Migrate v3 → v4: add `fact_entities`, `merge_audit`, `pending_merges` relations.
    pub(super) fn migrate_v3_to_v4(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v3 -> v4");

        // WHY: bounded range [3..6) to avoid creating relations from later migrations (causal_edges = index 6).
        for ddl in &KNOWLEDGE_DDL[3..6] {
            self.db
                .run(ddl, BTreeMap::new(), ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v3->v4 relation DDL failed: {e}"),
                    }
                    .build()
                })?;
        }

        self.db
            .run(
                crate::graph_intelligence::GRAPH_SCORES_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v3->v4 graph_scores DDL failed: {e}"),
                }
                .build()
            })?;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v3->v4 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v3 -> v4 complete");
        Ok(())
    }

    pub(super) fn migrate_v4_to_v5(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v4 -> v5");

        self.db
            .run(
                crate::consolidation::CONSOLIDATION_AUDIT_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v4->v5 consolidation_audit DDL failed: {e}"),
                }
                .build()
            })?;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v4->v5 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v4 -> v5 complete");
        Ok(())
    }

    /// Migrate v5 → v6: add `causal_edges` relation.
    pub(super) fn migrate_v5_to_v6(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v5 -> v6");

        // KNOWLEDGE_DDL[6] is the causal_edges relation (index 6, zero-based).
        self.db
            .run(KNOWLEDGE_DDL[6], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v5->v6 causal_edges DDL failed: {e}"),
                }
                .build()
            })?;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v5->v6 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v5 -> v6 complete");
        Ok(())
    }

    /// Migrate v6 → v7: add `relationship_type` to `causal_edges`.
    pub(super) fn migrate_v6_to_v7(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v6 -> v7");

        let all_edges = self
            .db
            .run(
                r"?[cause, effect, ordering, confidence, created_at] :=
                    *causal_edges{cause, effect, ordering, confidence, created_at}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v6->v7 read causal_edges: {e}"),
                }
                .build()
            })?;

        self.db
            .run(
                "::remove causal_edges",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v6->v7 remove causal_edges: {e}"),
                }
                .build()
            })?;

        self.db
            .run(KNOWLEDGE_DDL[6], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v6->v7 recreate causal_edges: {e}"),
                }
                .build()
            })?;

        for row in &all_edges.rows {
            let script = r"
                ?[cause, effect, ordering, relationship_type, confidence, created_at] <- [[
                    $cause, $effect, $ordering, 'caused', $confidence, $created_at
                ]]
                :put causal_edges {cause, effect => ordering, relationship_type, confidence, created_at}
            ";
            let mut params = BTreeMap::new();
            for (i, name) in ["cause", "effect", "ordering", "confidence", "created_at"]
                .iter()
                .enumerate()
            {
                if let Some(val) = row.get(i) {
                    params.insert((*name).to_owned(), val.clone());
                }
            }
            self.db
                .run(script, params, ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v6->v7 reinsert causal_edge: {e}"),
                    }
                    .build()
                })?;
        }

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v6->v7 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v6 -> v7 complete");
        Ok(())
    }

    /// Migrate v8 → v9: add `fact_multiplicity` side-index (#3634).
    ///
    /// Preserves multiplicity metadata for consolidated facts — source
    /// observation count, time spread, first/last observed timestamps —
    /// so recall and conflict resolution can weight consolidated facts by
    /// convergence strength without joining against the audit relation.
    ///
    /// Additive migration: no existing data is rewritten. Facts consolidated
    /// before v9 will have no multiplicity record; `get_fact_multiplicity`
    /// returns `None` for those.
    pub(super) fn migrate_v8_to_v9(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v8 -> v9");

        self.db
            .run(
                crate::consolidation::FACT_MULTIPLICITY_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v8->v9 fact_multiplicity DDL failed: {e}"),
                }
                .build()
            })?;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v8->v9 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v8 -> v9 complete");
        Ok(())
    }

    /// Migrate v7 → v8: add `type_hierarchy`, `derived_facts`, and `defaults` relations.
    ///
    /// These relations support the derived-rule engine introduced in the Wave 5
    /// Datalog feature (`derived_rules` module). All three are additive; no
    /// existing data is migrated.
    ///
    /// - `type_hierarchy` — IS-A edges used by ontological rules
    /// - `derived_facts` — materialized output of all rule sets
    /// - `defaults` — defeasible default assertions per entity+tag
    pub(super) fn migrate_v7_to_v8(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v7 -> v8");

        // KNOWLEDGE_DDL[7] = type_hierarchy, [8] = derived_facts, [9] = defaults.
        for ddl in &KNOWLEDGE_DDL[7..=9] {
            self.db
                .run(ddl, BTreeMap::new(), ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v7->v8 relation DDL failed: {e}"),
                    }
                    .build()
                })?;
        }

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v7->v8 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v7 -> v8 complete");
        Ok(())
    }

    /// Migrate v9 → v10: add `published_facts` and `provenance` relations.
    ///
    /// R716 Phase 3 introduces multi-agent verification + provenance tracking.
    /// Both relations are additive; no existing data is migrated.
    pub(super) fn migrate_v9_to_v10(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v9 -> v10");

        // KNOWLEDGE_DDL[10] = published_facts, [11] = provenance.
        for ddl in &KNOWLEDGE_DDL[10..=11] {
            self.db
                .run(ddl, BTreeMap::new(), ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v9->v10 relation DDL failed: {e}"),
                    }
                    .build()
                })?;
        }

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v9->v10 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v9 -> v10 complete");
        Ok(())
    }

    /// Migrate v10 → v11: add `scope` and `visibility` to `facts` relation.
    ///
    /// R722 wires `MemoryScope` and `Visibility` through the Datalog storage
    /// layer so that `apply_scope_quotas` and visibility filtering work
    /// end-to-end. Existing rows are backfilled with `scope = null` and
    /// `visibility = 'private'` to preserve existing semantics.
    #[expect(
        clippy::too_many_lines,
        reason = "migration is a single linear sequence"
    )]
    pub(super) fn migrate_v10_to_v11(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v10 -> v11");

        let all_facts = self
            .db
            .run(
                r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type,
                           is_forgotten, forgotten_at, forget_reason}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v10->v11 read facts: {e}"),
                }
                .build()
            })?;

        let _ = self.db.run(
            "::fts drop facts:content_fts",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        );

        self.db
            .run("::remove facts", BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v10->v11 remove facts: {e}"),
                }
                .build()
            })?;

        self.db
            .run(KNOWLEDGE_DDL[0], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v10->v11 recreate facts: {e}"),
                }
                .build()
            })?;

        for row in &all_facts.rows {
            let script = r"
                ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                  superseded_by, source_session_id, recorded_at,
                  access_count, last_accessed_at, stability_hours, fact_type,
                  is_forgotten, forgotten_at, forget_reason,
                  scope, project_id, visibility] <- [[
                    $id, $valid_from, $content, $nous_id, $confidence, $tier, $valid_to,
                    $superseded_by, $source_session_id, $recorded_at,
                    $access_count, $last_accessed_at, $stability_hours, $fact_type,
                    $is_forgotten, $forgotten_at, $forget_reason,
                    null, null, 'private'
                ]]
                :put facts {id, valid_from => content, nous_id, confidence, tier,
                            valid_to, superseded_by, source_session_id, recorded_at,
                            access_count, last_accessed_at, stability_hours, fact_type,
                            is_forgotten, forgotten_at, forget_reason,
                            scope, project_id, visibility}
            ";
            let mut params = BTreeMap::new();
            for (i, name) in [
                "id",
                "valid_from",
                "content",
                "nous_id",
                "confidence",
                "tier",
                "valid_to",
                "superseded_by",
                "source_session_id",
                "recorded_at",
                "access_count",
                "last_accessed_at",
                "stability_hours",
                "fact_type",
                "is_forgotten",
                "forgotten_at",
                "forget_reason",
            ]
            .iter()
            .enumerate()
            {
                if let Some(val) = row.get(i) {
                    params.insert((*name).to_owned(), val.clone());
                }
            }
            self.db
                .run(script, params, ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v10->v11 reinsert fact: {e}"),
                    }
                    .build()
                })?;
        }

        self.db
            .run(fts_ddl(), BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v10->v11 recreate FTS: {e}"),
                }
                .build()
            })?;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v10->v11 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v10 -> v11 complete");
        Ok(())
    }

    /// Migrate v11 → v12: add `project_id` to `facts`.
    ///
    /// Existing rows are backfilled with `project_id = null`, which preserves
    /// previous global recall semantics until runtime capture supplies a
    /// git-remote-derived partition.
    #[expect(
        clippy::too_many_lines,
        reason = "migration is a single linear sequence"
    )]
    pub(super) fn migrate_v11_to_v12(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v11 -> v12");

        let all_facts = self
            .db
            .run(
                r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason, scope, visibility] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type,
                           is_forgotten, forgotten_at, forget_reason, scope, visibility}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v11->v12 read facts: {e}"),
                }
                .build()
            })?;

        let _ = self.db.run(
            "::fts drop facts:content_fts",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        );

        self.db
            .run("::remove facts", BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v11->v12 remove facts: {e}"),
                }
                .build()
            })?;

        self.db
            .run(KNOWLEDGE_DDL[0], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v11->v12 recreate facts: {e}"),
                }
                .build()
            })?;

        for row in &all_facts.rows {
            let script = r"
                ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                  superseded_by, source_session_id, recorded_at,
                  access_count, last_accessed_at, stability_hours, fact_type,
                  is_forgotten, forgotten_at, forget_reason,
                  scope, project_id, visibility] <- [[
                    $id, $valid_from, $content, $nous_id, $confidence, $tier, $valid_to,
                    $superseded_by, $source_session_id, $recorded_at,
                    $access_count, $last_accessed_at, $stability_hours, $fact_type,
                    $is_forgotten, $forgotten_at, $forget_reason,
                    $scope, null, $visibility
                ]]
                :put facts {id, valid_from => content, nous_id, confidence, tier,
                            valid_to, superseded_by, source_session_id, recorded_at,
                            access_count, last_accessed_at, stability_hours, fact_type,
                            is_forgotten, forgotten_at, forget_reason,
                            scope, project_id, visibility}
            ";
            let mut params = BTreeMap::new();
            for (i, name) in [
                "id",
                "valid_from",
                "content",
                "nous_id",
                "confidence",
                "tier",
                "valid_to",
                "superseded_by",
                "source_session_id",
                "recorded_at",
                "access_count",
                "last_accessed_at",
                "stability_hours",
                "fact_type",
                "is_forgotten",
                "forgotten_at",
                "forget_reason",
                "scope",
                "visibility",
            ]
            .iter()
            .enumerate()
            {
                if let Some(val) = row.get(i) {
                    params.insert((*name).to_owned(), val.clone());
                }
            }
            self.db
                .run(script, params, ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v11->v12 reinsert fact: {e}"),
                    }
                    .build()
                })?;
        }

        self.db
            .run(fts_ddl(), BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v11->v12 recreate FTS: {e}"),
                }
                .build()
            })?;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v11->v12 version write failed: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v11 -> v12 complete");
        Ok(())
    }
}
