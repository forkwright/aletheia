use super::migration_atomic::{IndexSpec, RebuildSpec, rebuild_relation_atomically};
use super::{
    CAUSAL_EDGES_DDL, DEFAULTS_DDL, DERIVED_FACTS_DDL, DERIVED_RULE_WATERMARKS_DDL,
    DERIVED_SOURCE_REVISION_DDL, EMBEDDING_META_DDL, ENTITY_FLAGS_DDL, FACT_ENTITIES_DDL,
    FACTS_DDL, KnowledgeStore, MERGE_AUDIT_DDL, PENDING_MERGES_DDL, PROVENANCE_DDL,
    PUBLISHED_FACTS_DDL, TYPE_HIERARCHY_DDL, entities_ddl, fts_ddl,
};
use crate::engine::{DataValue, NamedRows};

pub(super) struct MigrationStep {
    pub(super) target_version: i64,
    pub(super) run: fn(&KnowledgeStore) -> crate::error::Result<()>,
}

pub(super) const MIGRATIONS: &[MigrationStep] = &[
    MigrationStep {
        target_version: 2,
        run: KnowledgeStore::migrate_v1_to_v2,
    },
    MigrationStep {
        target_version: 3,
        run: KnowledgeStore::migrate_v2_to_v3,
    },
    MigrationStep {
        target_version: 4,
        run: KnowledgeStore::migrate_v3_to_v4,
    },
    MigrationStep {
        target_version: 5,
        run: KnowledgeStore::migrate_v4_to_v5,
    },
    MigrationStep {
        target_version: 6,
        run: KnowledgeStore::migrate_v5_to_v6,
    },
    MigrationStep {
        target_version: 7,
        run: KnowledgeStore::migrate_v6_to_v7,
    },
    MigrationStep {
        target_version: 8,
        run: KnowledgeStore::migrate_v7_to_v8,
    },
    MigrationStep {
        target_version: 9,
        run: KnowledgeStore::migrate_v8_to_v9,
    },
    MigrationStep {
        target_version: 10,
        run: KnowledgeStore::migrate_v9_to_v10,
    },
    MigrationStep {
        target_version: 11,
        run: KnowledgeStore::migrate_v10_to_v11,
    },
    MigrationStep {
        target_version: 12,
        run: KnowledgeStore::migrate_v11_to_v12,
    },
    MigrationStep {
        target_version: 13,
        run: KnowledgeStore::migrate_v12_to_v13,
    },
    MigrationStep {
        target_version: 14,
        run: KnowledgeStore::migrate_v13_to_v14,
    },
    MigrationStep {
        target_version: 15,
        run: KnowledgeStore::migrate_v14_to_v15,
    },
    MigrationStep {
        target_version: 16,
        run: KnowledgeStore::migrate_v15_to_v16,
    },
    MigrationStep {
        target_version: 17,
        run: KnowledgeStore::migrate_v16_to_v17,
    },
    MigrationStep {
        target_version: 18,
        run: KnowledgeStore::migrate_v17_to_v18,
    },
    MigrationStep {
        target_version: 19,
        run: KnowledgeStore::migrate_v18_to_v19,
    },
];

// ---------------------------------------------------------------------
// `facts` rebuild: the current (v19-shape) column order, and a transform
// that carries every old-shape column over unchanged and backfills any
// column the old shape lacked with the literal value each migration
// historically wrote for it (aletheia#5779, plan §8.2).
// ---------------------------------------------------------------------

const FACTS_FULL_COLUMNS: &[&str] = &[
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
    "project_id",
    "visibility",
    "sensitivity",
];

fn facts_default_value(column: &str) -> DataValue {
    match column {
        "access_count" => DataValue::from(0_i64),
        "last_accessed_at" | "fact_type" => DataValue::Str("".into()),
        "stability_hours" => DataValue::from(720.0_f64),
        "is_forgotten" => DataValue::Bool(false),
        "visibility" => DataValue::Str("private".into()),
        "sensitivity" => DataValue::Str("public".into()),
        // "forgotten_at", "forget_reason", "scope", "project_id" — nullable,
        // no declared default; every migration that predates them backfills
        // explicit null rather than relying on the engine (which requires
        // every non-key column to be either bound or `default`-bearing).
        _ => DataValue::Null,
    }
}

fn rebuild_facts_row(
    old_columns: &[&str],
    old_row: &[DataValue],
) -> crate::error::Result<Vec<DataValue>> {
    FACTS_FULL_COLUMNS
        .iter()
        .map(|col| match old_columns.iter().position(|c| c == col) {
            Some(idx) => old_row.get(idx).cloned().ok_or_else(|| {
                crate::error::EngineQuerySnafu {
                    message: format!(
                        "facts row shorter than its declared old-shape column list at '{col}'"
                    ),
                }
                .build()
            }),
            None => Ok(facts_default_value(col)),
        })
        .collect()
}

/// Build a `facts` rebuild transform for a migration whose old shape had
/// exactly `old_columns`, in that order (matching the migration's own
/// `read_script` head clause).
fn facts_rebuild_transform(
    old_columns: &'static [&'static str],
) -> impl Fn(&NamedRows) -> crate::error::Result<NamedRows> {
    move |old: &NamedRows| {
        let rows: Vec<Vec<DataValue>> = old
            .rows
            .iter()
            .map(|row| rebuild_facts_row(old_columns, row))
            .collect::<crate::error::Result<Vec<_>>>()?;
        Ok(NamedRows::new(
            FACTS_FULL_COLUMNS.iter().map(|c| (*c).to_owned()).collect(),
            rows,
        ))
    }
}

fn facts_content_fts_index() -> Vec<IndexSpec> {
    vec![IndexSpec {
        name: "content_fts",
        recreate_ddl: fts_ddl().to_owned(),
    }]
}

// ---------------------------------------------------------------------
// `causal_edges` rebuild: same pattern. `id` (minted fresh per row — no
// prior identity exists to preserve, #4551) and `relationship_type`
// ('caused' — the only relationship type that existed before v7 named the
// concept) are the two columns every pre-v17 shape lacks.
// ---------------------------------------------------------------------

const CAUSAL_EDGES_FULL_COLUMNS: &[&str] = &[
    "cause",
    "effect",
    "id",
    "ordering",
    "relationship_type",
    "confidence",
    "evidence_session_id",
    "created_at",
];

fn rebuild_causal_edge_row(
    old_columns: &[&str],
    old_row: &[DataValue],
) -> crate::error::Result<Vec<DataValue>> {
    CAUSAL_EDGES_FULL_COLUMNS
        .iter()
        .map(|col| match old_columns.iter().position(|c| c == col) {
            Some(idx) => old_row.get(idx).cloned().ok_or_else(|| {
                crate::error::EngineQuerySnafu {
                    message: format!(
                        "causal_edges row shorter than its declared old-shape column list at '{col}'"
                    ),
                }
                .build()
            }),
            None if *col == "id" => {
                Ok(DataValue::Str(koina::ulid::Ulid::new().to_string().into()))
            }
            None if *col == "relationship_type" => Ok(DataValue::Str("caused".into())),
            None => Ok(DataValue::Null),
        })
        .collect()
}

fn causal_edges_rebuild_transform(
    old_columns: &'static [&'static str],
) -> impl Fn(&NamedRows) -> crate::error::Result<NamedRows> {
    move |old: &NamedRows| {
        let rows: Vec<Vec<DataValue>> = old
            .rows
            .iter()
            .map(|row| rebuild_causal_edge_row(old_columns, row))
            .collect::<crate::error::Result<Vec<_>>>()?;
        Ok(NamedRows::new(
            CAUSAL_EDGES_FULL_COLUMNS
                .iter()
                .map(|c| (*c).to_owned())
                .collect(),
            rows,
        ))
    }
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Run a single DDL statement and wrap errors with `context`.
    fn run_ddl(&self, ddl: &str, context: &str) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::ScriptMutability;
        self.db
            .run(ddl, BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("{context}: {e}"),
                }
                .build()
            })
            .map(|_| ())
    }

    pub(super) fn migrate_v1_to_v2(&self) -> crate::error::Result<()> {
        const OLD_COLUMNS: &[&str] = &[
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
        ];
        tracing::info!("migrating knowledge schema v1 -> v2");
        let transform = facts_rebuild_transform(OLD_COLUMNS);
        rebuild_relation_atomically(
            self,
            &RebuildSpec {
                relation: "facts",
                label: "v1->v2",
                target_version: 2,
                read_script: r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at}",
                live_ddl: FACTS_DDL.to_owned(),
                expected_indices: facts_content_fts_index(),
                transform: &transform,
            },
        )?;
        tracing::info!("knowledge schema migration v1 -> v2 complete");
        Ok(())
    }

    pub(super) fn migrate_v2_to_v3(&self) -> crate::error::Result<()> {
        const OLD_COLUMNS: &[&str] = &[
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
        ];
        tracing::info!("migrating knowledge schema v2 -> v3");
        let transform = facts_rebuild_transform(OLD_COLUMNS);
        rebuild_relation_atomically(
            self,
            &RebuildSpec {
                relation: "facts",
                label: "v2->v3",
                target_version: 3,
                read_script: r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type}",
                live_ddl: FACTS_DDL.to_owned(),
                expected_indices: facts_content_fts_index(),
                transform: &transform,
            },
        )?;
        tracing::info!("knowledge schema migration v2 -> v3 complete");
        Ok(())
    }

    /// Migrate v3 → v4: add `fact_entities`, `merge_audit`, `pending_merges` relations.
    pub(super) fn migrate_v3_to_v4(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::ScriptMutability;
        tracing::info!("migrating knowledge schema v3 -> v4");

        // WHY: create exactly the three relations added in v4; named constants
        // prevent later insertions from silently shifting slice indices.
        self.run_ddl(FACT_ENTITIES_DDL, "v3->v4 create fact_entities")?;
        self.run_ddl(MERGE_AUDIT_DDL, "v3->v4 create merge_audit")?;
        self.run_ddl(PENDING_MERGES_DDL, "v3->v4 create pending_merges")?;

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

        self.stamp_schema_version(4, "v3->v4")?;

        tracing::info!("knowledge schema migration v3 -> v4 complete");
        Ok(())
    }

    pub(super) fn migrate_v4_to_v5(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::ScriptMutability;
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

        self.stamp_schema_version(5, "v4->v5")?;

        tracing::info!("knowledge schema migration v4 -> v5 complete");
        Ok(())
    }

    /// Migrate v5 → v6: add `causal_edges` relation.
    pub(super) fn migrate_v5_to_v6(&self) -> crate::error::Result<()> {
        tracing::info!("migrating knowledge schema v5 -> v6");

        self.run_ddl(CAUSAL_EDGES_DDL, "v5->v6 create causal_edges")?;

        self.stamp_schema_version(6, "v5->v6")?;

        tracing::info!("knowledge schema migration v5 -> v6 complete");
        Ok(())
    }

    /// Migrate v6 → v7: add `relationship_type` (and, since this rebuild
    /// targets the current `CAUSAL_EDGES_DDL` shape like every other site in
    /// this file, `id` + `evidence_session_id`) to `causal_edges`. `id` is
    /// minted fresh per row exactly as `migrate_v16_to_v17` already does —
    /// no prior identity exists to preserve.
    pub(super) fn migrate_v6_to_v7(&self) -> crate::error::Result<()> {
        const OLD_COLUMNS: &[&str] = &["cause", "effect", "ordering", "confidence", "created_at"];
        tracing::info!("migrating knowledge schema v6 -> v7");
        let transform = causal_edges_rebuild_transform(OLD_COLUMNS);
        rebuild_relation_atomically(
            self,
            &RebuildSpec {
                relation: "causal_edges",
                label: "v6->v7",
                target_version: 7,
                read_script: r"?[cause, effect, ordering, confidence, created_at] :=
                    *causal_edges{cause, effect, ordering, confidence, created_at}",
                live_ddl: CAUSAL_EDGES_DDL.to_owned(),
                expected_indices: vec![],
                transform: &transform,
            },
        )?;
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

        use crate::engine::ScriptMutability;
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

        self.stamp_schema_version(9, "v8->v9")?;

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
        tracing::info!("migrating knowledge schema v7 -> v8");

        self.run_ddl(TYPE_HIERARCHY_DDL, "v7->v8 create type_hierarchy")?;
        self.run_ddl(DERIVED_FACTS_DDL, "v7->v8 create derived_facts")?;
        self.run_ddl(DEFAULTS_DDL, "v7->v8 create defaults")?;

        self.stamp_schema_version(8, "v7->v8")?;

        tracing::info!("knowledge schema migration v7 -> v8 complete");
        Ok(())
    }

    /// Migrate v9 → v10: add `published_facts` and `provenance` relations.
    ///
    /// R716 Phase 3 introduces multi-agent verification + provenance tracking.
    /// Both relations are additive; no existing data is migrated.
    pub(super) fn migrate_v9_to_v10(&self) -> crate::error::Result<()> {
        tracing::info!("migrating knowledge schema v9 -> v10");

        self.run_ddl(PUBLISHED_FACTS_DDL, "v9->v10 create published_facts")?;
        self.run_ddl(PROVENANCE_DDL, "v9->v10 create provenance")?;

        self.stamp_schema_version(10, "v9->v10")?;

        tracing::info!("knowledge schema migration v9 -> v10 complete");
        Ok(())
    }

    /// Migrate v10 → v11: add `scope` and `visibility` to `facts` relation.
    ///
    /// R722 wires `MemoryScope` and `Visibility` through the Datalog storage
    /// layer so that `apply_scope_quotas` and visibility filtering work
    /// end-to-end. Existing rows are backfilled with `scope = null` and
    /// `visibility = 'private'` to preserve existing semantics.
    pub(super) fn migrate_v10_to_v11(&self) -> crate::error::Result<()> {
        const OLD_COLUMNS: &[&str] = &[
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
        ];
        tracing::info!("migrating knowledge schema v10 -> v11");
        let transform = facts_rebuild_transform(OLD_COLUMNS);
        rebuild_relation_atomically(
            self,
            &RebuildSpec {
                relation: "facts",
                label: "v10->v11",
                target_version: 11,
                read_script: r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type,
                           is_forgotten, forgotten_at, forget_reason}",
                live_ddl: FACTS_DDL.to_owned(),
                expected_indices: facts_content_fts_index(),
                transform: &transform,
            },
        )?;
        tracing::info!("knowledge schema migration v10 -> v11 complete");
        Ok(())
    }

    /// Migrate v11 → v12: add `project_id` to `facts`.
    ///
    /// Existing rows are backfilled with `project_id = null`, which preserves
    /// previous global recall semantics until runtime capture supplies a
    /// git-remote-derived partition.
    pub(super) fn migrate_v11_to_v12(&self) -> crate::error::Result<()> {
        const OLD_COLUMNS: &[&str] = &[
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
        ];
        tracing::info!("migrating knowledge schema v11 -> v12");
        let transform = facts_rebuild_transform(OLD_COLUMNS);
        rebuild_relation_atomically(
            self,
            &RebuildSpec {
                relation: "facts",
                label: "v11->v12",
                target_version: 12,
                read_script: r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason, scope, visibility] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type,
                           is_forgotten, forgotten_at, forget_reason, scope, visibility}",
                live_ddl: FACTS_DDL.to_owned(),
                expected_indices: facts_content_fts_index(),
                transform: &transform,
            },
        )?;
        tracing::info!("knowledge schema migration v11 -> v12 complete");
        Ok(())
    }

    /// Migrate v12 → v13: add `name_embedding` to `entities`.
    ///
    /// Wires Path A of the memory-dedup reachability fix (#4165). The
    /// dedup pipeline weights `embed_sim` at 0.30; with no column to store
    /// per-entity name embeddings, both production callers passed
    /// `|_, _| 0.0`, capping the composite score at 0.70 and making
    /// `MergeDecision::AutoMerge` (≥ 0.90) structurally unreachable. This
    /// migration adds a nullable `name_embedding: <F32; DIM>?` column;
    /// existing rows are backfilled with NULL (preserving prior behaviour),
    /// and callers with an [`EmbeddingProvider`] in scope can populate
    /// embeddings via [`KnowledgeStore::update_entity_name_embedding`] or
    /// the dedup-time backfill in
    /// [`KnowledgeStore::run_entity_dedup_with_embeddings`].
    ///
    /// [`EmbeddingProvider`]: crate::embedding::EmbeddingProvider
    pub(super) fn migrate_v12_to_v13(&self) -> crate::error::Result<()> {
        tracing::info!("migrating knowledge schema v12 -> v13");
        let transform = |old: &NamedRows| -> crate::error::Result<NamedRows> {
            const NEW_COLUMNS: &[&str] = &[
                "id",
                "name",
                "entity_type",
                "aliases",
                "created_at",
                "updated_at",
                "name_embedding",
            ];
            let rows: Vec<Vec<DataValue>> = old
                .rows
                .iter()
                .map(|row| {
                    let mut new_row = row.clone();
                    new_row.push(DataValue::Null); // name_embedding: no prior column to carry over
                    new_row
                })
                .collect();
            Ok(NamedRows::new(
                NEW_COLUMNS.iter().map(|c| (*c).to_owned()).collect(),
                rows,
            ))
        };
        rebuild_relation_atomically(
            self,
            &RebuildSpec {
                relation: "entities",
                label: "v12->v13",
                target_version: 13,
                read_script: r"?[id, name, entity_type, aliases, created_at, updated_at] :=
                    *entities{id, name, entity_type, aliases, created_at, updated_at}",
                live_ddl: entities_ddl(self.dim),
                expected_indices: vec![],
                transform: &transform,
            },
        )?;
        tracing::info!("knowledge schema migration v12 -> v13 complete");
        Ok(())
    }

    /// Migrate v13 → v14: add `sensitivity` to `facts`.
    ///
    /// Existing rows predate durable sensitivity storage, so the migration
    /// backfills the documented default (`public`) explicitly. Runtime
    /// sensitivity edits and recall filtering then hydrate from the facts
    /// relation instead of reconstructing protected facts as public after a
    /// restart.
    pub(super) fn migrate_v13_to_v14(&self) -> crate::error::Result<()> {
        const OLD_COLUMNS: &[&str] = &[
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
            "project_id",
            "visibility",
        ];
        tracing::info!("migrating knowledge schema v13 -> v14");
        let transform = facts_rebuild_transform(OLD_COLUMNS);
        rebuild_relation_atomically(
            self,
            &RebuildSpec {
                relation: "facts",
                label: "v13->v14",
                target_version: 14,
                read_script: r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type,
                           is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility}",
                live_ddl: FACTS_DDL.to_owned(),
                expected_indices: facts_content_fts_index(),
                transform: &transform,
            },
        )?;
        tracing::info!("knowledge schema migration v13 -> v14 complete");
        Ok(())
    }

    /// Migrate v14 → v15: persist embedding schema metadata.
    ///
    /// Existing stores did not record the embedding model that produced their
    /// vectors. The migration writes the explicit `assumed` marker instead of
    /// guessing a provider name, forcing normal startup to ask the operator for
    /// a re-embed before recall uses unknown vectors.
    pub(super) fn migrate_v14_to_v15(&self) -> crate::error::Result<()> {
        tracing::info!("migrating knowledge schema v14 -> v15");

        if !self
            .relation_names()?
            .iter()
            .any(|name| name == "embedding_meta")
        {
            self.run_ddl(EMBEDDING_META_DDL, "v14->v15 create embedding_meta")?;
        }
        self.replace_embedding_meta(Self::ASSUMED_EMBEDDING_MODEL, self.dim)?;
        self.stamp_schema_version(15, "v14->v15")?;

        tracing::info!("knowledge schema migration v14 -> v15 complete");
        Ok(())
    }

    /// Migrate v15 -> v16: add entity review flags relation.
    pub(super) fn migrate_v15_to_v16(&self) -> crate::error::Result<()> {
        tracing::info!("migrating knowledge schema v15 -> v16");

        if !self
            .relation_names()?
            .iter()
            .any(|name| name == "entity_flags")
        {
            self.run_ddl(ENTITY_FLAGS_DDL, "v15->v16 create entity_flags")?;
        }
        self.stamp_schema_version(16, "v15->v16")?;

        tracing::info!("knowledge schema migration v15 -> v16 complete");
        Ok(())
    }

    /// Migrate v16 → v17: add `id` and `evidence_session_id` to `causal_edges`.
    ///
    /// Pre-v17 edges stored only `(cause, effect)` identity and no provenance,
    /// so reads synthesized a fresh edge ID and dropped the evidence session.
    /// Existing rows are re-keyed unchanged; each is assigned a stable ULID
    /// edge ID (no prior identity exists to preserve) and a null
    /// `evidence_session_id` (#4551). New edges carry both fields from insert.
    pub(super) fn migrate_v16_to_v17(&self) -> crate::error::Result<()> {
        const OLD_COLUMNS: &[&str] = &[
            "cause",
            "effect",
            "ordering",
            "relationship_type",
            "confidence",
            "created_at",
        ];
        tracing::info!("migrating knowledge schema v16 -> v17");
        let transform = causal_edges_rebuild_transform(OLD_COLUMNS);
        rebuild_relation_atomically(
            self,
            &RebuildSpec {
                relation: "causal_edges",
                label: "v16->v17",
                target_version: 17,
                read_script: r"?[cause, effect, ordering, relationship_type, confidence, created_at] :=
                    *causal_edges{cause, effect, ordering, relationship_type, confidence, created_at}",
                live_ddl: CAUSAL_EDGES_DDL.to_owned(),
                expected_indices: vec![],
                transform: &transform,
            },
        )?;
        tracing::info!("knowledge schema migration v16 -> v17 complete");
        Ok(())
    }

    /// Migrate v17 -> v18: backfill `fact_entities` edges for existing facts.
    ///
    /// Before #4675, normal extraction never linked facts to the entities they
    /// reference, so historical facts have no `fact_entities` edges and graph
    /// recall, scoped dedup, and consolidation cannot see them. This backfill
    /// infers edges by matching each stored entity id against the slugified
    /// fact content: an entity whose id appears as a whole hyphen-delimited run
    /// in `slugify(content)` is linked to that fact. Matching is bounded to
    /// token boundaries so a short entity id cannot match inside a longer word.
    /// The link is idempotent, so re-running is safe and new extraction-time
    /// edges are never duplicated.
    pub(super) fn migrate_v17_to_v18(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v17 -> v18");

        let entity_rows = self
            .db
            .run(
                "?[id] := *entities{id}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v17->v18 read entities: {e}"),
                }
                .build()
            })?;
        let entity_slugs: Vec<String> = entity_rows
            .rows
            .iter()
            .filter_map(|row| row.first().and_then(crate::engine::DataValue::get_str))
            .map(str::to_owned)
            .collect();

        if entity_slugs.is_empty() {
            self.stamp_schema_version(18, "v17->v18")?;
            tracing::info!("knowledge schema migration v17 -> v18 complete (no entities)");
            return Ok(());
        }

        let fact_rows = self
            .db
            .run(
                "?[id, content] := *facts{id, content}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v17->v18 read facts: {e}"),
                }
                .build()
            })?;

        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let mut linked = 0_usize;
        for row in &fact_rows.rows {
            let (Some(fact_id), Some(content)) = (
                row.first().and_then(DataValue::get_str),
                row.get(1).and_then(DataValue::get_str),
            ) else {
                continue;
            };
            // Pad so a whole-token entity id matches only on hyphen boundaries.
            let haystack = format!("-{}-", crate::extract::utils::slugify(content));
            for entity_slug in &entity_slugs {
                if !haystack.contains(&format!("-{entity_slug}-")) {
                    continue;
                }
                let script = r"
                    ?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]
                    :put fact_entities {fact_id, entity_id => created_at}
                ";
                let mut params = BTreeMap::new();
                params.insert("fact_id".to_owned(), DataValue::Str(fact_id.into()));
                params.insert(
                    "entity_id".to_owned(),
                    DataValue::Str(entity_slug.as_str().into()),
                );
                params.insert("created_at".to_owned(), DataValue::Str(now.as_str().into()));
                self.db
                    .run(script, params, ScriptMutability::Mutable)
                    .map_err(|e| {
                        crate::error::EngineQuerySnafu {
                            message: format!("v17->v18 link fact-entity: {e}"),
                        }
                        .build()
                    })?;
                linked += 1;
            }
        }

        self.stamp_schema_version(18, "v17->v18")?;

        tracing::info!(linked, "knowledge schema migration v17 -> v18 complete");
        Ok(())
    }

    /// Migrate v18 → v19: add derived-rule freshness bookkeeping and
    /// consolidation provenance side-index (#4660, #4662).
    ///
    /// - `derived_source_revision` holds a global monotonic revision bumped by
    ///   base writes (`facts`, `entities`, `causal_edges`, `type_hierarchy`,
    ///   `defaults`).
    /// - `derived_rule_watermarks` records, per rule family, the source
    ///   revision the materialization ran against, the timestamp, and a dirty
    ///   flag. Existing derived rows are marked dirty so the next scheduled
    ///   materialization refreshes them against the new revision counter.
    /// - `consolidation_provenance` stores source fact IDs and source session
    ///   IDs for each consolidated fact.
    #[expect(
        clippy::too_many_lines,
        reason = "sequential migration that creates multiple relations and backfills watermarks"
    )]
    pub(super) fn migrate_v18_to_v19(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        tracing::info!("migrating knowledge schema v18 -> v19");

        // The migration must be idempotent: a store created at the current
        // schema, or a partially-applied migration, may already contain these
        // relations. Skip creation when present, but still ensure the
        // bookkeeping rows and watermarks are correct.
        if !self.relation_exists("derived_source_revision")? {
            self.db
                .run(
                    DERIVED_SOURCE_REVISION_DDL,
                    BTreeMap::new(),
                    ScriptMutability::Mutable,
                )
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v18->v19 derived_source_revision DDL failed: {e}"),
                    }
                    .build()
                })?;
        }

        // Initialize the global source revision at 0. Base writes that happen
        // after this migration will bump it; materializations will record the
        // current revision in their watermark and clear the dirty flag.
        // `:put` is an upsert, so this is safe to re-run.
        self.db
            .run(
                r"?[key, revision] <- [['global', 0]]
                  :put derived_source_revision { key => revision }",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v18->v19 init derived_source_revision: {e}"),
                }
                .build()
            })?;

        let has_watermarks = self.relation_exists("derived_rule_watermarks")?;
        if !has_watermarks {
            self.db
                .run(
                    DERIVED_RULE_WATERMARKS_DDL,
                    BTreeMap::new(),
                    ScriptMutability::Mutable,
                )
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v18->v19 derived_rule_watermarks DDL failed: {e}"),
                    }
                    .build()
                })?;
        }

        // Collect watermarks that already exist so a re-run does not overwrite
        // a clean watermark (and therefore a valid materialization) with a
        // stale dirty marker.
        let mut existing_families = std::collections::BTreeSet::<String>::new();
        if has_watermarks {
            let rows = self
                .db
                .run(
                    "?[rule_id] := *derived_rule_watermarks{rule_id}",
                    BTreeMap::new(),
                    ScriptMutability::Immutable,
                )
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v18->v19 read existing watermarks: {e}"),
                    }
                    .build()
                })?;
            for row in &rows.rows {
                if let Some(family) = row.first().and_then(DataValue::get_str) {
                    let _ = existing_families.insert(family.to_owned());
                }
            }
        }

        // Mark every rule family that already has derived output as dirty with
        // source_revision 0, but only once. This forces a refresh before callers
        // trust the output as fresh, rather than silently presenting
        // pre-migration rows.
        let mut seen_families = std::collections::BTreeSet::new();
        if self.relation_exists("derived_facts")? {
            let existing_rule_families = self
                .db
                .run(
                    "?[rule_id] := *derived_facts{entity_id, rule_id, derived_content, confidence, materialized_at}",
                    BTreeMap::new(),
                    ScriptMutability::Immutable,
                )
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v18->v19 read existing rule families: {e}"),
                    }
                    .build()
                })?;

            for row in &existing_rule_families.rows {
                let Some(rule_id) = row.first().and_then(DataValue::get_str) else {
                    continue;
                };
                // Rule IDs are `family:detail`; watermark at the family level.
                let family = match rule_id.split_once(':') {
                    Some((family, _)) => family,
                    None => rule_id,
                };
                if !seen_families.insert(family) {
                    continue;
                }
                if existing_families.contains(family) {
                    continue;
                }
                let mut params = BTreeMap::new();
                params.insert("rule_id".to_owned(), DataValue::Str(family.into()));
                params.insert("source_revision".to_owned(), DataValue::from(0_i64));
                params.insert("materialized_at".to_owned(), DataValue::Str("".into()));
                params.insert("dirty".to_owned(), DataValue::Bool(true));
                self.db
                    .run(
                        r"?[rule_id, source_revision, materialized_at, dirty] <-
                            [[$rule_id, $source_revision, $materialized_at, $dirty]]
                          :put derived_rule_watermarks {
                              rule_id => source_revision, materialized_at, dirty
                          }",
                        params,
                        ScriptMutability::Mutable,
                    )
                    .map_err(|e| {
                        crate::error::EngineQuerySnafu {
                            message: format!("v18->v19 watermark existing family {family}: {e}"),
                        }
                        .build()
                    })?;
            }
        }

        if !self.relation_exists("consolidation_provenance")? {
            self.db
                .run(
                    crate::consolidation::CONSOLIDATION_PROVENANCE_DDL,
                    BTreeMap::new(),
                    ScriptMutability::Mutable,
                )
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v18->v19 consolidation_provenance DDL failed: {e}"),
                    }
                    .build()
                })?;
        }

        self.stamp_schema_version(19, "v18->v19")?;

        tracing::info!("knowledge schema migration v18 -> v19 complete");
        Ok(())
    }
}
