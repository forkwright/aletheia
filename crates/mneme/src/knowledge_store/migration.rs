use super::{KNOWLEDGE_DDL, KnowledgeStore, fts_ddl};

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    // --- Migration ---

    #[expect(
        clippy::too_many_lines,
        reason = "migration is a single linear sequence"
    )]
    pub(super) fn migrate_v1_to_v2(&self) -> crate::error::Result<()> {
        use crate::engine::{DataValue, ScriptMutability};
        use std::collections::BTreeMap;

        tracing::info!("migrating knowledge schema v1 -> v2");

        // 1. Read all existing facts
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

        // 2. Drop FTS index (must be dropped before relation)
        let _ = self.db.run(
            "::fts drop facts:content_fts",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        );

        // 3. Drop old facts relation
        self.db
            .run("::remove facts", BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 remove facts: {e}"),
                }
                .build()
            })?;

        // 4. Recreate with new schema (includes access tracking columns)
        self.db
            .run(KNOWLEDGE_DDL[0], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 recreate facts: {e}"),
                }
                .build()
            })?;

        // 5. Reinsert facts with defaults for new columns
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

        // 6. Recreate FTS index
        self.db
            .run(fts_ddl(), BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 recreate FTS: {e}"),
                }
                .build()
            })?;

        // 7. Update schema version
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
                    message: format!("v1->v2 update version: {e}"),
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
        use crate::engine::{DataValue, ScriptMutability};
        use std::collections::BTreeMap;

        tracing::info!("migrating knowledge schema v2 -> v3");

        // 1. Read all existing facts (v2 schema: 14 columns)
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

        // 2. Drop FTS index
        let _ = self.db.run(
            "::fts drop facts:content_fts",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        );

        // 3. Drop old facts relation
        self.db
            .run("::remove facts", BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 remove facts: {e}"),
                }
                .build()
            })?;

        // 4. Recreate with new schema (includes forget columns)
        self.db
            .run(KNOWLEDGE_DDL[0], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 recreate facts: {e}"),
                }
                .build()
            })?;

        // 5. Reinsert facts with defaults for new columns
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

        // 6. Recreate FTS index
        self.db
            .run(fts_ddl(), BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 recreate FTS: {e}"),
                }
                .build()
            })?;

        // 7. Update schema version
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
                    message: format!("v2->v3 update version: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v2 -> v3 complete");
        Ok(())
    }

    /// Migrate v3 → v4: add `fact_entities`, `merge_audit`, `pending_merges` relations.
    pub(super) fn migrate_v3_to_v4(&self) -> crate::error::Result<()> {
        use crate::engine::{DataValue, ScriptMutability};
        use std::collections::BTreeMap;

        tracing::info!("migrating knowledge schema v3 -> v4");

        // Add new relations (indices 3, 4, 5 in KNOWLEDGE_DDL)
        for ddl in &KNOWLEDGE_DDL[3..] {
            self.db
                .run(ddl, BTreeMap::new(), ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v3->v4 create relation: {e}"),
                    }
                    .build()
                })?;
        }

        // Add graph_scores relation for PageRank + Louvain cache
        self.db
            .run(
                crate::graph_intelligence::GRAPH_SCORES_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v3->v4 create graph_scores: {e}"),
                }
                .build()
            })?;

        // Update schema version
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
                    message: format!("v3->v4 update version: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v3 -> v4 complete");
        Ok(())
    }

    pub(super) fn migrate_v4_to_v5(&self) -> crate::error::Result<()> {
        use crate::engine::{DataValue, ScriptMutability};
        use std::collections::BTreeMap;

        tracing::info!("migrating knowledge schema v4 -> v5");

        // Add consolidation_audit relation
        self.db
            .run(
                crate::consolidation::CONSOLIDATION_AUDIT_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v4->v5 create consolidation_audit: {e}"),
                }
                .build()
            })?;

        // Update schema version
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
                    message: format!("v4->v5 update version: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v4 -> v5 complete");
        Ok(())
    }
}
