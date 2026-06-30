use snafu::ResultExt;
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive 1:1 field-to-column marshalling; splitting would obscure the mapping"
)]
pub(super) fn fact_to_params(
    fact: &crate::knowledge::Fact,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::DataValue;
    use crate::knowledge::format_timestamp;
    let mut p = std::collections::BTreeMap::new();
    p.insert("id".to_owned(), DataValue::Str(fact.id.as_str().into()));
    p.insert(
        "valid_from".to_owned(),
        DataValue::Str(format_timestamp(&fact.temporal.valid_from).into()),
    );
    p.insert(
        "content".to_owned(),
        DataValue::Str(fact.content.as_str().into()),
    );
    p.insert(
        "nous_id".to_owned(),
        DataValue::Str(fact.nous_id.as_str().into()),
    );
    p.insert(
        "confidence".to_owned(),
        DataValue::from(fact.provenance.confidence),
    );
    p.insert(
        "tier".to_owned(),
        DataValue::Str(fact.provenance.tier.as_str().into()),
    );
    p.insert(
        "valid_to".to_owned(),
        DataValue::Str(format_timestamp(&fact.temporal.valid_to).into()),
    );
    p.insert(
        "superseded_by".to_owned(),
        match &fact.lifecycle.superseded_by {
            Some(s) => DataValue::Str(s.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "source_session_id".to_owned(),
        match &fact.provenance.source_session_id {
            Some(s) => DataValue::Str(s.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "recorded_at".to_owned(),
        DataValue::Str(format_timestamp(&fact.temporal.recorded_at).into()),
    );
    p.insert(
        "access_count".to_owned(),
        DataValue::from(i64::from(fact.access.access_count)),
    );
    p.insert(
        "last_accessed_at".to_owned(),
        match &fact.access.last_accessed_at {
            Some(ts) => DataValue::Str(format_timestamp(ts).into()),
            None => DataValue::Str("".into()),
        },
    );
    p.insert(
        "stability_hours".to_owned(),
        DataValue::from(fact.provenance.stability_hours),
    );
    p.insert(
        "fact_type".to_owned(),
        DataValue::Str(fact.fact_type.as_str().into()),
    );
    p.insert(
        "is_forgotten".to_owned(),
        DataValue::Bool(fact.lifecycle.is_forgotten),
    );
    p.insert(
        "forgotten_at".to_owned(),
        match &fact.lifecycle.forgotten_at {
            Some(ts) => DataValue::Str(format_timestamp(ts).into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "forget_reason".to_owned(),
        match &fact.lifecycle.forget_reason {
            Some(r) => DataValue::Str(r.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "scope".to_owned(),
        match &fact.scope {
            Some(s) => DataValue::Str(s.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "project_id".to_owned(),
        match &fact.project_id {
            Some(id) => DataValue::Str(id.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "visibility".to_owned(),
        DataValue::Str(fact.visibility.as_str().into()),
    );
    p.insert(
        "sensitivity".to_owned(),
        DataValue::Str(fact.sensitivity.as_str().into()),
    );
    p
}

#[cfg(feature = "mneme-engine")]
pub(super) fn entity_to_params(
    entity: &crate::knowledge::Entity,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::DataValue;
    let mut p = std::collections::BTreeMap::new();
    p.insert("id".to_owned(), DataValue::Str(entity.id.as_str().into()));
    p.insert(
        "name".to_owned(),
        DataValue::Str(entity.name.as_str().into()),
    );
    p.insert(
        "entity_type".to_owned(),
        DataValue::Str(entity.entity_type.as_str().into()),
    );
    p.insert(
        "aliases".to_owned(),
        DataValue::Str(entity.aliases.join(",").into()),
    );
    p.insert(
        "created_at".to_owned(),
        DataValue::Str(crate::knowledge::format_timestamp(&entity.created_at).into()),
    );
    p.insert(
        "updated_at".to_owned(),
        DataValue::Str(crate::knowledge::format_timestamp(&entity.updated_at).into()),
    );
    // WHY (#4165 Path A): the entities relation gained a nullable
    // `name_embedding` column in v13. `Entity` does not carry the embedding
    // (cross-crate change avoided); callers populate it via
    // `KnowledgeStore::update_entity_name_embedding` or the dedup-time
    // backfill. Default to Null here so `insert_entity` preserves prior
    // behaviour for callers without an `EmbeddingProvider` in scope.
    p.insert("name_embedding".to_owned(), DataValue::Null);
    p
}

#[cfg(feature = "mneme-engine")]
pub(super) fn relationship_to_params(
    rel: &crate::knowledge::Relationship,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::DataValue;
    let mut p = std::collections::BTreeMap::new();
    p.insert("src".to_owned(), DataValue::Str(rel.src.as_str().into()));
    p.insert("dst".to_owned(), DataValue::Str(rel.dst.as_str().into()));
    p.insert(
        "relation".to_owned(),
        DataValue::Str(rel.relation.as_str().into()),
    );
    p.insert("weight".to_owned(), DataValue::from(rel.weight));
    p.insert(
        "created_at".to_owned(),
        DataValue::Str(crate::knowledge::format_timestamp(&rel.created_at).into()),
    );
    p
}

#[cfg(feature = "mneme-engine")]
pub(super) fn embedding_to_params(
    chunk: &crate::knowledge::EmbeddedChunk,
    _dim: usize,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::{Array1, DataValue, Vector};
    let mut p = std::collections::BTreeMap::new();
    p.insert("id".to_owned(), DataValue::Str(chunk.id.as_str().into()));
    p.insert(
        "content".to_owned(),
        DataValue::Str(chunk.content.as_str().into()),
    );
    p.insert(
        "source_type".to_owned(),
        DataValue::Str(chunk.source_type.as_str().into()),
    );
    p.insert(
        "source_id".to_owned(),
        DataValue::Str(chunk.source_id.as_str().into()),
    );
    p.insert(
        "nous_id".to_owned(),
        DataValue::Str(chunk.nous_id.as_str().into()),
    );
    p.insert(
        "embedding".to_owned(),
        DataValue::Vec(Vector::F32(Array1::from(chunk.embedding.clone()))),
    );
    p.insert(
        "created_at".to_owned(),
        DataValue::Str(crate::knowledge::format_timestamp(&chunk.created_at).into()),
    );
    p
}

/// Number of columns a query must project for full [`Fact`] hydration via
/// [`rows_to_facts`]. The canonical projection order lives in
/// `crate::query::queries::FULL_FACT_SELECT`; the two MUST stay in lockstep
/// (a compile-time assertion in that module enforces the count). See
/// #4677/#4549: any query feeding `rows_to_facts` whose shape is shorter than
/// this is a positional-decoding hazard and is rejected at read time.
#[cfg(feature = "mneme-engine")]
pub(crate) const FULL_FACT_COLUMNS: usize = 21;

#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::too_many_lines,
    reason = "column extraction is sequential — splitting would obscure the mapping"
)]
pub(super) fn rows_to_facts(
    rows: crate::engine::NamedRows,
    nous_id: &str,
) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
    use crate::knowledge::Fact;
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        // SECURITY (#4677/#4549): scope, project_id, and visibility are
        // policy/tenancy fields decoded by fixed column index below. A row that
        // is too short would silently shift those fields and default security
        // metadata to a more permissive value. Refuse the whole read instead.
        if row.len() < FULL_FACT_COLUMNS {
            return Err(crate::error::ConversionSnafu {
                message: format!(
                    "fact row: expected {FULL_FACT_COLUMNS} columns for full hydration, got {}",
                    row.len()
                ),
            }
            .build());
        }
        let id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing id",
            }
            .build()
        })?)?;
        let content = extract_str(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing content",
            }
            .build()
        })?)?;
        let confidence = extract_float(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing confidence",
            }
            .build()
        })?)?;
        let tier_str = extract_str(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing tier",
            }
            .build()
        })?)?;
        let recorded_at = extract_str(row.get(4).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing recorded_at",
            }
            .build()
        })?)?;
        let nous_id_col = extract_str(row.get(5).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing nous_id",
            }
            .build()
        })?)?;
        let valid_from = extract_str(row.get(6).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing valid_from",
            }
            .build()
        })?)?;
        let valid_to = extract_str(row.get(7).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing valid_to",
            }
            .build()
        })?)?;
        let superseded_by = extract_optional_str(row.get(8).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing superseded_by",
            }
            .build()
        })?)?;
        let source_session_id = extract_optional_str(row.get(9).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing source_session_id",
            }
            .build()
        })?)?;

        let tier = parse_epistemic_tier(&tier_str);

        let access_count =
            u32::try_from(row.get(10).and_then(|v| extract_int(v).ok()).unwrap_or(0)).unwrap_or(0);
        let last_accessed_at = row
            .get(11)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let stability_hours = row
            .get(12)
            .and_then(|v| extract_float(v).ok())
            .unwrap_or(720.0);
        let fact_type = row
            .get(13)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let is_forgotten = row
            .get(14)
            .and_then(|v| extract_bool(v).ok())
            .unwrap_or(false);
        let forgotten_at = row
            .get(15)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None);
        let forget_reason = row
            .get(16)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| s.parse::<crate::knowledge::ForgetReason>().ok());
        // SECURITY (#4677/#4549): decode policy/tenancy fields loudly. `scope`
        // and `project_id` are nullable (a SQL null legitimately means
        // unscoped/global), but a *present-but-undecodable* value must error
        // rather than silently widen to global. `visibility` is required and is
        // never allowed to fall back to a default.
        let scope = parse_optional_scope(row.get(17))?;
        let project_id = parse_optional_project_id(row.get(18))?;
        let visibility = parse_visibility(row.get(19))?;
        let sensitivity = parse_fact_sensitivity(row.get(20))?;

        let fact_id = crate::id::FactId::new(id).context(crate::error::InvalidIdSnafu)?;
        let superseded_by_id = superseded_by
            .map(crate::id::FactId::new)
            .transpose()
            .context(crate::error::InvalidIdSnafu)?;
        out.push(Fact {
            id: fact_id,
            nous_id: if nous_id_col.is_empty() {
                nous_id.to_owned()
            } else {
                nous_id_col
            },
            content,
            fact_type,
            scope,
            project_id,
            visibility,
            temporal: crate::knowledge::FactTemporal {
                valid_from: crate::knowledge::parse_timestamp(&valid_from)
                    .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
                valid_to: crate::knowledge::parse_timestamp(&valid_to)
                    .unwrap_or_else(crate::knowledge::far_future),
                recorded_at: crate::knowledge::parse_timestamp(&recorded_at)
                    .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
            },
            provenance: crate::knowledge::FactProvenance {
                confidence,
                tier,
                source_session_id,
                stability_hours,
            },
            lifecycle: crate::knowledge::FactLifecycle {
                superseded_by: superseded_by_id,
                is_forgotten,
                forgotten_at: forgotten_at.and_then(|s| crate::knowledge::parse_timestamp(&s)),
                forget_reason,
            },
            access: crate::knowledge::FactAccess {
                access_count,
                last_accessed_at: crate::knowledge::parse_timestamp(&last_accessed_at),
            },
            sensitivity,
        });
    }
    Ok(out)
}

#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::too_many_lines,
    reason = "flat row parser — splitting would not improve clarity"
)]
pub(super) fn rows_to_raw_facts(
    rows: crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
    use crate::knowledge::Fact;
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        // SECURITY (#4677/#4549): same short-row guard as `rows_to_facts` — a
        // truncated row would shift policy/tenancy columns and default them to a
        // more permissive value.
        if row.len() < FULL_FACT_COLUMNS {
            return Err(crate::error::ConversionSnafu {
                message: format!(
                    "raw fact row: expected {FULL_FACT_COLUMNS} columns for full hydration, got {}",
                    row.len()
                ),
            }
            .build());
        }
        let id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing id",
            }
            .build()
        })?)?;
        let valid_from = extract_str(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing valid_from",
            }
            .build()
        })?)?;
        let content = extract_str(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing content",
            }
            .build()
        })?)?;
        let nous_id = extract_str(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing nous_id",
            }
            .build()
        })?)?;
        let confidence = extract_float(row.get(4).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing confidence",
            }
            .build()
        })?)?;
        let tier_str = extract_str(row.get(5).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing tier",
            }
            .build()
        })?)?;
        let valid_to = extract_str(row.get(6).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing valid_to",
            }
            .build()
        })?)?;
        let superseded_by = extract_optional_str(row.get(7).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing superseded_by",
            }
            .build()
        })?)?;
        let source_session_id = extract_optional_str(row.get(8).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing source_session_id",
            }
            .build()
        })?)?;
        let recorded_at = extract_str(row.get(9).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing recorded_at",
            }
            .build()
        })?)?;
        let tier = parse_epistemic_tier(&tier_str);
        let access_count =
            u32::try_from(row.get(10).and_then(|v| extract_int(v).ok()).unwrap_or(0)).unwrap_or(0);
        let last_accessed_at = row
            .get(11)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let stability_hours = row
            .get(12)
            .and_then(|v| extract_float(v).ok())
            .unwrap_or(720.0);
        let fact_type = row
            .get(13)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let is_forgotten = row
            .get(14)
            .and_then(|v| extract_bool(v).ok())
            .unwrap_or(false);
        let forgotten_at = row
            .get(15)
            .and_then(|v| extract_optional_str(v).ok())
            .flatten();
        let forget_reason = row
            .get(16)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| s.parse::<crate::knowledge::ForgetReason>().ok());
        // SECURITY (#4677/#4549): decode policy/tenancy fields loudly. `scope`
        // and `project_id` are nullable (a SQL null legitimately means
        // unscoped/global), but a *present-but-undecodable* value must error
        // rather than silently widen to global. `visibility` is required and is
        // never allowed to fall back to a default.
        let scope = parse_optional_scope(row.get(17))?;
        let project_id = parse_optional_project_id(row.get(18))?;
        let visibility = parse_visibility(row.get(19))?;
        let sensitivity = parse_fact_sensitivity(row.get(20))?;
        let fact_id = crate::id::FactId::new(id).context(crate::error::InvalidIdSnafu)?;
        let superseded_by_id = superseded_by
            .map(crate::id::FactId::new)
            .transpose()
            .context(crate::error::InvalidIdSnafu)?;
        out.push(Fact {
            id: fact_id,
            nous_id,
            content,
            fact_type,
            scope,
            project_id,
            visibility,
            temporal: crate::knowledge::FactTemporal {
                valid_from: crate::knowledge::parse_timestamp(&valid_from)
                    .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
                valid_to: crate::knowledge::parse_timestamp(&valid_to)
                    .unwrap_or_else(crate::knowledge::far_future),
                recorded_at: crate::knowledge::parse_timestamp(&recorded_at)
                    .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
            },
            provenance: crate::knowledge::FactProvenance {
                confidence,
                tier,
                source_session_id,
                stability_hours,
            },
            lifecycle: crate::knowledge::FactLifecycle {
                superseded_by: superseded_by_id,
                is_forgotten,
                forgotten_at: forgotten_at.and_then(|s| crate::knowledge::parse_timestamp(&s)),
                forget_reason,
            },
            access: crate::knowledge::FactAccess {
                access_count,
                last_accessed_at: crate::knowledge::parse_timestamp(&last_accessed_at),
            },
            sensitivity,
        });
    }
    Ok(out)
}

#[cfg(feature = "mneme-engine")]
pub(super) fn rows_to_facts_partial(
    rows: crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
    use crate::knowledge::Fact;
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        let id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact_at row: missing id",
            }
            .build()
        })?)?;
        let content = extract_str(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact_at row: missing content",
            }
            .build()
        })?)?;
        let confidence = extract_float(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact_at row: missing confidence",
            }
            .build()
        })?)?;
        let tier_str = extract_str(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact_at row: missing tier",
            }
            .build()
        })?)?;
        let tier = parse_epistemic_tier(&tier_str);
        let sensitivity = parse_fact_sensitivity(row.get(4))?;

        let fact_id = crate::id::FactId::new(id).context(crate::error::InvalidIdSnafu)?;
        out.push(Fact {
            id: fact_id,
            nous_id: String::new(),
            content,
            fact_type: String::new(),
            scope: None,
            project_id: None,
            visibility: crate::knowledge::Visibility::Private,
            temporal: crate::knowledge::FactTemporal {
                valid_from: jiff::Timestamp::UNIX_EPOCH,
                valid_to: crate::knowledge::far_future(),
                recorded_at: jiff::Timestamp::UNIX_EPOCH,
            },
            provenance: crate::knowledge::FactProvenance {
                confidence,
                tier,
                source_session_id: None,
                stability_hours: 720.0,
            },
            lifecycle: crate::knowledge::FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: crate::knowledge::FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
            sensitivity,
        });
    }
    Ok(out)
}

#[cfg(feature = "mneme-engine")]
pub(super) fn rows_to_recall_results(
    rows: crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
    use crate::knowledge::RecallResult;
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        let _id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing id",
            }
            .build()
        })?)?;
        let content = extract_str(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing content",
            }
            .build()
        })?)?;
        let source_type = extract_str(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing source_type",
            }
            .build()
        })?)?;
        let source_id = extract_str(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing source_id",
            }
            .build()
        })?)?;
        let distance = extract_float(row.get(4).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing dist",
            }
            .build()
        })?)?;

        let scope = row
            .get(5)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| s.parse::<crate::knowledge::MemoryScope>().ok());
        let project_id = row
            .get(6)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok());
        let visibility = row
            .get(7)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default()
            .parse::<crate::knowledge::Visibility>()
            .unwrap_or_default();
        let nous_id = row
            .get(8)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let sensitivity = parse_fact_sensitivity(row.get(9))?;
        out.push(RecallResult {
            content,
            distance,
            source_type,
            source_id,
            nous_id,
            sensitivity,
            graph_importance: 0.0,
            scope,
            project_id,
            visibility,
            // WHY (#4415): populated by `enrich_source_counts` on the recall
            // path; the generic row marshaller has no side-index access.
            source_count: 0,
        });
    }
    Ok(out)
}

/// Reduce a natural-language message to a bag-of-terms full-text-search query.
///
/// WHY: The FTS `query:` argument of `~facts:content_fts{... query: $query_text ...}`
/// is parsed by Cozo's *own* full-text query grammar, in which characters such as
/// `?`, `*`, `"`, parentheses and boolean keywords are operators. Binding a raw user
/// message (e.g. any question, which ends in `?`) therefore triggers an FTS parse
/// error that is swallowed, silently disabling knowledge recall for that turn
/// (#4156). Keeping only alphanumeric word tokens yields a universally valid
/// bare-term query that preserves recall on the meaningful words while dropping all
/// FTS-syntax characters. Returns an empty string when the message has no word
/// characters; callers treat that as "no text query".
#[cfg(feature = "mneme-engine")]
pub(super) fn sanitize_fts_query(raw: &str) -> String {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(feature = "mneme-engine")]
pub(super) fn scoped_visibility_rules() -> &'static str {
    r"
    visible_fact[id] := *facts{id, nous_id: $requester_nous_id}
    visible_fact[id] := *facts{id, visibility: 'shared'}
    visible_fact[id] := *facts{id, visibility: 'published'}
"
}

#[cfg(feature = "mneme-engine")]
pub(super) fn build_hybrid_query(q: &super::HybridQuery) -> String {
    build_hybrid_query_inner(q, false)
}

#[cfg(feature = "mneme-engine")]
pub(super) fn build_scoped_hybrid_query(q: &super::HybridQuery) -> String {
    build_hybrid_query_inner(q, true)
}

#[cfg(feature = "mneme-engine")]
fn build_hybrid_query_inner(q: &super::HybridQuery, scoped: bool) -> String {
    use super::queries;

    // WHY: When the message has no full-text terms (e.g. an emoji- or
    // punctuation-only message that `sanitize_fts_query` reduces to ""), the FTS
    // `query: ""` argument is itself an FTS parse error (#4156). Emit an empty
    // `bm25` relation in that case so vector + graph search still run; the
    // `$query_text` param is then left unbound by the caller.
    let bm25_rule = if sanitize_fts_query(&q.text).is_empty() {
        "bm25[id, score] <- []".to_owned()
    } else if scoped {
        "bm25_raw[id, score] := ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: score}\n    bm25[id, score] := bm25_raw[id, score], visible_fact[id]".to_owned()
    } else {
        "bm25[id, score] := ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: score}".to_owned()
    };

    let graph_rules = if q.seed_entities.is_empty() {
        "graph[id, score] <- []".to_owned()
    } else {
        let seed_data: Vec<String> = q
            .seed_entities
            .iter()
            .map(|s| format!("[\"{}\"]", s.as_str().replace('"', "\\\"")))
            .collect();
        let seeds_inline = seed_data.join(", ");
        if scoped {
            format!(
                "seed_list[e] <- [{seeds_inline}]\n        \
                 graph_entity[eid, score] := seed_list[seed], *relationships{{src: seed, dst: eid, weight: score}}\n        \
                 graph_entity[eid, score] := seed_list[seed], *relationships{{src: seed, dst: mid, weight: _w}}, \
                 *relationships{{src: mid, dst: eid, weight}}, score = weight * 0.5\n        \
                 graph_raw[id, score] := graph_entity[eid, score], *fact_entities{{fact_id: id, entity_id: eid}}\n        \
                 graph[id, sum(score)] := graph_raw[id, score], visible_fact[id]"
            )
        } else {
            format!(
                "seed_list[e] <- [{seeds_inline}]\n        \
                 graph_raw[id, score] := seed_list[seed], *relationships{{src: seed, dst: id, weight: score}}\n        \
                 graph_raw[id, score] := seed_list[seed], *relationships{{src: seed, dst: mid, weight: _w}}, \
                 *relationships{{src: mid, dst: id, weight}}, score = weight * 0.5\n        \
                 graph[id, sum(score)] := graph_raw[id, score]"
            )
        }
    };
    let mut script = queries::HYBRID_SEARCH_BASE
        .replace("{BM25_RULE}", &bm25_rule)
        .replace("{GRAPH_RULES}", &graph_rules);
    if scoped {
        script = script.replace(
            "vec[id, score] :=\n        ~embeddings:semantic_idx{id | query: $query_vec, k: $k, ef: $ef, bind_distance: raw_dist},\n        score = 1.0 - raw_dist",
            "vec_raw[source_id, score] :=\n        ~embeddings:semantic_idx{id, source_type, source_id | query: $query_vec, k: $k, ef: $ef, bind_distance: raw_dist},\n        source_type == 'fact',\n        score = 1.0 - raw_dist\n\n    vec[id, score] := vec_raw[id, score], visible_fact[id]",
        );
        format!("{}\n{script}", scoped_visibility_rules())
    } else {
        script
    }
}

#[cfg(feature = "mneme-engine")]
pub(super) fn rows_to_hybrid_results(
    rows: crate::engine::NamedRows,
) -> crate::error::Result<Vec<super::HybridResult>> {
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        let id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing id",
            }
            .build()
        })?)?;
        let rrf_score = extract_float(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing rrf_score",
            }
            .build()
        })?)?;
        let bm25_rank = extract_int(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing bm25_rank",
            }
            .build()
        })?)?;
        let vec_rank = extract_int(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing vec_rank",
            }
            .build()
        })?)?;
        let graph_rank = extract_int(row.get(4).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing graph_rank",
            }
            .build()
        })?)?;
        let fact_id = crate::id::FactId::new(id).context(crate::error::InvalidIdSnafu)?;
        out.push(super::HybridResult {
            id: fact_id,
            rrf_score,
            bm25_rank,
            vec_rank,
            graph_rank,
        });
    }
    // WHY: Safety sort; Datalog :order is applied by the engine but we re-sort for correctness guarantee.
    out.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

#[cfg(feature = "mneme-engine")]
pub(super) fn extract_str(val: &crate::engine::DataValue) -> crate::error::Result<String> {
    match val {
        crate::engine::DataValue::Str(s) => Ok(s.to_string()),
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Str, got {other:?}"),
        }
        .build()),
    }
}

#[cfg(feature = "mneme-engine")]
pub(super) fn extract_optional_str(
    val: &crate::engine::DataValue,
) -> crate::error::Result<Option<String>> {
    match val {
        crate::engine::DataValue::Null => Ok(None),
        crate::engine::DataValue::Str(s) => Ok(Some(s.to_string())),
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Str or Null, got {other:?}"),
        }
        .build()),
    }
}

#[cfg(feature = "mneme-engine")]
pub(super) fn extract_float(val: &crate::engine::DataValue) -> crate::error::Result<f64> {
    val.get_float().ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!("expected Num(Float), got {val:?}"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
pub(super) fn extract_int(val: &crate::engine::DataValue) -> crate::error::Result<i64> {
    val.get_int().ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!("expected Num(Int), got {val:?}"),
        }
        .build()
    })
}

/// Extract a nullable `<F32; DIM>` vector column from a `DataValue`.
///
/// Returns `None` for `DataValue::Null` (e.g. an entity row whose
/// `name_embedding` has not been populated yet); returns `Some` for a
/// stored vector. Used by the dedup pipeline to feed real cosine
/// similarity into the merge score (#4165 Path A).
#[cfg(feature = "mneme-engine")]
pub(super) fn extract_optional_f32_vec(
    val: &crate::engine::DataValue,
) -> crate::error::Result<Option<Vec<f32>>> {
    use crate::engine::{DataValue, Vector};
    match val {
        DataValue::Null => Ok(None),
        DataValue::Vec(Vector::F32(arr)) => Ok(Some(arr.to_vec())),
        DataValue::Vec(Vector::F64(arr)) => {
            // WHY: callers only ever write F32; defensively coerce F64 in
            // case a stored column was widened by a future migration so the
            // dedup pipeline does not silently fall back to embed_sim=0.0.
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "narrowing F64 → F32 for embedding storage parity; precision loss is acceptable here because cosine-similarity inputs are already normalised"
            )]
            let v: Vec<f32> = arr.iter().map(|x| *x as f32).collect();
            Ok(Some(v))
        }
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Vec(F32) or Null, got {other:?}"),
        }
        .build()),
    }
}

#[cfg(feature = "mneme-engine")]
pub(super) fn extract_bool(val: &crate::engine::DataValue) -> crate::error::Result<bool> {
    match val {
        crate::engine::DataValue::Bool(b) => Ok(*b),
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Bool, got {other:?}"),
        }
        .build()),
    }
}

/// Decode the `sensitivity` column. Like `scope` and `visibility`, a
/// present-but-undecodable value must error rather than silently widen to
/// `Public` (the default/least-restrictive variant), which would allow a
/// `Confidential` fact to egress to cloud providers undetected (#5753).
#[cfg(feature = "mneme-engine")]
fn parse_fact_sensitivity(
    val: Option<&crate::engine::DataValue>,
) -> crate::error::Result<crate::knowledge::FactSensitivity> {
    let cell = val.ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: "fact row: missing sensitivity column",
        }
        .build()
    })?;
    match extract_optional_str(cell)?.filter(|s| !s.is_empty()) {
        None => Ok(crate::knowledge::FactSensitivity::default()),
        Some(s) => s
            .parse::<crate::knowledge::FactSensitivity>()
            .map_err(|_unparsed| {
                crate::error::ConversionSnafu {
                    message: format!(
                        "fact row: undecodable sensitivity '{s}' — refusing to default to Public"
                    ),
                }
                .build()
            }),
    }
}

/// Decode the nullable `scope` column. A SQL null or empty string is a genuine
/// "unscoped/global" value, but a present-but-undecodable string is a policy
/// hazard and must error rather than silently widen to global (#4677/#4549).
#[cfg(feature = "mneme-engine")]
fn parse_optional_scope(
    val: Option<&crate::engine::DataValue>,
) -> crate::error::Result<Option<crate::knowledge::MemoryScope>> {
    let cell = val.ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: "fact row: missing scope column",
        }
        .build()
    })?;
    match extract_optional_str(cell)?.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => s
            .parse::<crate::knowledge::MemoryScope>()
            .map(Some)
            .map_err(|_unparsed| {
                crate::error::ConversionSnafu {
                    message: format!("fact row: undecodable scope '{s}'"),
                }
                .build()
            }),
    }
}

/// Decode the nullable `project_id` column. As with `scope`, null/empty is the
/// global tenant, but a present-but-invalid project hash must error rather than
/// silently demote the fact to global (#4677/#4549).
#[cfg(feature = "mneme-engine")]
fn parse_optional_project_id(
    val: Option<&crate::engine::DataValue>,
) -> crate::error::Result<Option<eidos::workspace::ProjectId>> {
    let cell = val.ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: "fact row: missing project_id column",
        }
        .build()
    })?;
    match extract_optional_str(cell)?.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => eidos::workspace::ProjectId::from_sha256_hex(&s)
            .map(Some)
            .map_err(|_unparsed| {
                crate::error::ConversionSnafu {
                    message: format!("fact row: undecodable project_id '{s}'"),
                }
                .build()
            }),
    }
}

/// Decode the required `visibility` column. Visibility is a security boundary:
/// a missing, empty, or undecodable value must error rather than fall back to a
/// default that could expose or hide a fact incorrectly (#4677/#4549).
#[cfg(feature = "mneme-engine")]
fn parse_visibility(
    val: Option<&crate::engine::DataValue>,
) -> crate::error::Result<crate::knowledge::Visibility> {
    let raw = extract_str(val.ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: "fact row: missing visibility column",
        }
        .build()
    })?)?;
    if raw.is_empty() {
        return Err(crate::error::ConversionSnafu {
            message: "fact row: empty visibility — refusing to default a security field",
        }
        .build());
    }
    raw.parse::<crate::knowledge::Visibility>()
        .map_err(|_unparsed| {
            crate::error::ConversionSnafu {
                message: format!("fact row: undecodable visibility '{raw}'"),
            }
            .build()
        })
}

#[cfg(feature = "mneme-engine")]
pub(super) fn parse_epistemic_tier(s: &str) -> crate::knowledge::EpistemicTier {
    use crate::knowledge::EpistemicTier;
    match s {
        "verified" => EpistemicTier::Verified,
        "reflected" => EpistemicTier::Reflected,
        "inferred" => EpistemicTier::Inferred,
        "assumed" => EpistemicTier::Assumed,
        "training" => EpistemicTier::Training,
        other => {
            tracing::warn!(tier = %other, "unknown epistemic tier in stored fact, defaulting to assumed");
            EpistemicTier::Assumed
        }
    }
}

#[cfg(all(test, feature = "mneme-engine"))]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::engine::{DataValue, NamedRows};
    use crate::knowledge::EpistemicTier;

    // ── extract_str / extract_optional_str — DataValue extraction ───────────

    #[test]
    fn extract_str_from_str_value() {
        let val = DataValue::Str("hello".into());
        let result = extract_str(&val).expect("should extract");
        assert_eq!(result, "hello");
    }

    #[test]
    fn extract_str_rejects_non_str() {
        let val = DataValue::from(42i64);
        let result = extract_str(&val);
        assert!(result.is_err(), "extract_str on int should return Err");
    }

    #[test]
    fn extract_optional_str_from_null() {
        let val = DataValue::Null;
        let result = extract_optional_str(&val).expect("should extract");
        assert!(result.is_none(), "Null DataValue should map to None");
    }

    #[test]
    fn extract_optional_str_from_str_value() {
        let val = DataValue::Str("present".into());
        let result = extract_optional_str(&val).expect("should extract");
        assert_eq!(
            result.as_deref(),
            Some("present"),
            "extract_optional_str should yield the inner str"
        );
    }

    #[test]
    fn extract_optional_str_rejects_other_types() {
        let val = DataValue::Bool(true);
        let result = extract_optional_str(&val);
        assert!(
            result.is_err(),
            "extract_optional_str on Bool should return Err"
        );
    }

    // ── extract_float / extract_int / extract_bool ──────────────────────────

    #[test]
    fn extract_float_from_float_value() {
        let val = DataValue::from(1.5_f64);
        let result = extract_float(&val).expect("should extract");
        assert!(
            (result - 1.5).abs() < f64::EPSILON,
            "extract_float should return input 1.5"
        );
    }

    #[test]
    fn extract_float_rejects_str() {
        let val = DataValue::Str("not a number".into());
        let result = extract_float(&val);
        assert!(result.is_err(), "extract_float on Str should return Err");
    }

    #[test]
    fn extract_int_from_int_value() {
        let val = DataValue::from(42i64);
        let result = extract_int(&val).expect("should extract");
        assert_eq!(result, 42, "extract_int should return input 42");
    }

    #[test]
    fn extract_int_rejects_str() {
        let val = DataValue::Str("42".into());
        let result = extract_int(&val);
        assert!(result.is_err(), "extract_int on Str should return Err");
    }

    #[test]
    fn extract_bool_true() {
        let val = DataValue::Bool(true);
        let result = extract_bool(&val).expect("should extract");
        assert!(result, "extract_bool on Bool(true) should be true");
    }

    #[test]
    fn extract_bool_false() {
        let val = DataValue::Bool(false);
        let result = extract_bool(&val).expect("should extract");
        assert!(!result, "extract_bool on Bool(false) should be false");
    }

    #[test]
    fn extract_bool_rejects_int() {
        let val = DataValue::from(1i64);
        let result = extract_bool(&val);
        assert!(result.is_err(), "extract_bool on int should return Err");
    }

    // ── parse_epistemic_tier — string → enum mapping ────────────────────────

    #[test]
    fn parse_verified_tier() {
        assert_eq!(
            parse_epistemic_tier("verified"),
            EpistemicTier::Verified,
            "`verified` tier string should map to Verified"
        );
    }

    #[test]
    fn parse_inferred_tier() {
        assert_eq!(
            parse_epistemic_tier("inferred"),
            EpistemicTier::Inferred,
            "`inferred` tier string should map to Inferred"
        );
    }

    #[test]
    fn parse_assumed_tier() {
        assert_eq!(
            parse_epistemic_tier("assumed"),
            EpistemicTier::Assumed,
            "`assumed` tier string should map to Assumed"
        );
    }

    #[test]
    fn parse_unknown_tier_defaults_to_assumed() {
        // Unknown strings fall back to Assumed (with a warn log)
        assert_eq!(
            parse_epistemic_tier("bogus"),
            EpistemicTier::Assumed,
            "unknown tier should default to Assumed"
        );
        assert_eq!(
            parse_epistemic_tier(""),
            EpistemicTier::Assumed,
            "empty tier should default to Assumed"
        );
        assert_eq!(
            parse_epistemic_tier("VERIFIED"),
            EpistemicTier::Assumed,
            "uppercase tier name should not match (case-sensitive)"
        );
    }

    // ── build_hybrid_query — script rendering ───────────────────────────────

    #[test]
    fn build_hybrid_query_contains_limits() {
        use super::super::HybridQuery;
        let query = HybridQuery {
            text: "test query".to_owned(),
            embedding: vec![0.1_f32; 384],
            seed_entities: Vec::new(),
            limit: 20,
            ef: 50,
        };
        let script = build_hybrid_query(&query);
        // The script should include the limit and ef parameters somewhere
        assert!(
            script.contains("20") || script.contains("$limit"),
            "script should reference the limit"
        );
        assert!(
            script.contains("50") || script.contains("$ef"),
            "script should reference the ef parameter"
        );
    }

    // ── sanitize_fts_query — FTS query-string hardening (#4156) ─────────────

    #[test]
    fn build_hybrid_query_omits_fts_when_no_text_terms() {
        use super::super::HybridQuery;
        let query = HybridQuery {
            // Punctuation-only message reduces to an empty FTS query (#4156).
            text: "???".to_owned(),
            embedding: vec![0.1_f32; 384],
            seed_entities: Vec::new(),
            limit: 20,
            ef: 50,
        };
        let script = build_hybrid_query(&query);
        assert!(
            script.contains("bm25[id, score] <- []"),
            "empty-text query should emit an empty bm25 relation, got: {script}"
        );
        assert!(
            !script.contains("content_fts"),
            "empty-text query must not reference the FTS index, got: {script}"
        );
        assert!(
            !script.contains("$query_text"),
            "empty-text query must not reference the unbound $query_text param, got: {script}"
        );
    }

    #[test]
    fn build_hybrid_query_includes_fts_when_text_present() {
        use super::super::HybridQuery;
        let query = HybridQuery {
            text: "datalog databases".to_owned(),
            embedding: vec![0.1_f32; 384],
            seed_entities: Vec::new(),
            limit: 20,
            ef: 50,
        };
        let script = build_hybrid_query(&query);
        assert!(
            script.contains("content_fts") && script.contains("$query_text"),
            "text query should reference the FTS index and $query_text, got: {script}"
        );
    }

    #[test]
    fn sanitize_fts_query_strips_question_mark() {
        // WHY (#4156): a trailing '?' is an FTS-special character — unsanitized
        // it causes a parse error that disables recall on every question.
        assert_eq!(
            sanitize_fts_query("what do you remember about datalog?"),
            "what do you remember about datalog"
        );
    }

    #[test]
    fn sanitize_fts_query_drops_fts_operators_and_punctuation() {
        // Quotes, parentheses, wildcards and other FTS operators must not reach
        // the engine as syntax — they are reduced to plain whitespace separators.
        assert_eq!(
            sanitize_fts_query("\"foo*\" AND (bar) -baz?!"),
            "foo AND bar baz"
        );
    }

    #[test]
    fn sanitize_fts_query_collapses_whitespace() {
        assert_eq!(sanitize_fts_query("  hello\t\nworld  "), "hello world");
    }

    #[test]
    fn sanitize_fts_query_empty_when_no_word_chars() {
        // A message with no alphanumerics yields an empty query rather than
        // invalid FTS syntax.
        assert_eq!(sanitize_fts_query("???"), "");
        assert_eq!(sanitize_fts_query(""), "");
    }

    // ── fail-loud policy-field decoding (#4677/#4549) ───────────────────────

    #[test]
    fn rows_to_facts_rejects_short_row() {
        // A row shorter than the full-fact contract would shift policy/tenancy
        // columns and silently default visibility/scope/project. It must error.
        let rows = crate::engine::NamedRows {
            headers: Vec::new(),
            rows: vec![vec![
                DataValue::Str("id1".into()),
                DataValue::Str("content".into()),
                DataValue::from(0.9_f64),
                DataValue::Str("inferred".into()),
                DataValue::Str("2026-01-01".into()),
            ]],
            next: None,
        };
        let err = rows_to_facts(rows, "agent").expect_err("short row must error");
        assert!(
            err.to_string().contains("columns for full hydration"),
            "error must name the column-count contract, got: {err}"
        );
    }

    #[test]
    fn parse_visibility_refuses_empty_or_undecodable() {
        assert!(
            parse_visibility(None).is_err(),
            "missing visibility column must error"
        );
        assert!(
            parse_visibility(Some(&DataValue::Str("".into()))).is_err(),
            "empty visibility must error, not default"
        );
        assert!(
            parse_visibility(Some(&DataValue::Str("not-a-visibility".into()))).is_err(),
            "undecodable visibility must error"
        );
        let ok = parse_visibility(Some(&DataValue::Str(
            crate::knowledge::Visibility::Shared.as_str().into(),
        )))
        .expect("valid visibility decodes");
        assert_eq!(
            ok,
            crate::knowledge::Visibility::Shared,
            "decoded visibility must round-trip to Shared"
        );
    }

    #[test]
    fn parse_optional_scope_nulls_ok_but_garbage_errors() {
        assert_eq!(
            parse_optional_scope(Some(&DataValue::Null)).expect("null scope ok"),
            None,
            "SQL null scope is a genuine unscoped value"
        );
        assert!(
            parse_optional_scope(Some(&DataValue::Str("bogus-scope".into()))).is_err(),
            "present-but-undecodable scope must error, not widen to global"
        );
        let scope = parse_optional_scope(Some(&DataValue::Str(
            crate::knowledge::MemoryScope::Project.as_str().into(),
        )))
        .expect("valid scope round-trips");
        assert_eq!(
            scope,
            Some(crate::knowledge::MemoryScope::Project),
            "decoded scope must round-trip to Project"
        );
    }

    #[test]
    fn parse_optional_project_id_nulls_ok_but_garbage_errors() {
        assert_eq!(
            parse_optional_project_id(Some(&DataValue::Null)).expect("null project ok"),
            None,
            "SQL null project id must decode to None"
        );
        assert!(
            parse_optional_project_id(Some(&DataValue::Str("not-a-hash".into()))).is_err(),
            "present-but-invalid project hash must error, not demote to global"
        );
        let project =
            eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/alpha.git")
                .expect("valid remote");
        assert_eq!(
            parse_optional_project_id(Some(&DataValue::Str(project.as_str().into())))
                .expect("valid project ok"),
            Some(project),
            "decoded project id must round-trip to the original value"
        );
    }

    // WHY (#5848): Helper building a full 21-column fact row in the order expected
    // by `rows_to_facts`, with the epistemic tier parameterized.
    fn rows_to_facts_row(tier: &str) -> Vec<DataValue> {
        vec![
            DataValue::from("f-tier-test"),
            DataValue::from("tier round-trip content"),
            DataValue::from(0.9f64),
            DataValue::from(tier),
            DataValue::from("2026-03-01T00:00:00Z"),
            DataValue::from("alice"),
            DataValue::from("2026-01-01T00:00:00Z"),
            DataValue::from("9999-01-01T00:00:00Z"),
            DataValue::Null,           // superseded_by
            DataValue::Null,           // source_session_id
            DataValue::from(0i64),     // access_count
            DataValue::from(""),       // last_accessed_at
            DataValue::from(720.0f64), // stability_hours
            DataValue::from("observation"),
            DataValue::Bool(false),     // is_forgotten
            DataValue::Null,            // forgotten_at
            DataValue::Null,            // forget_reason
            DataValue::Null,            // scope
            DataValue::Null,            // project_id
            DataValue::from("private"), // visibility
            DataValue::from("public"),  // sensitivity
        ]
    }

    // WHY (#5848): Helper building a full 21-column fact row in the order expected
    // by `rows_to_raw_facts`, with the epistemic tier parameterized.
    fn rows_to_raw_facts_row(tier: &str) -> Vec<DataValue> {
        vec![
            DataValue::from("f-tier-test"),
            DataValue::from("2026-01-01T00:00:00Z"), // valid_from
            DataValue::from("tier round-trip content"),
            DataValue::from("alice"),
            DataValue::from(0.9f64),
            DataValue::from(tier),
            DataValue::from("9999-01-01T00:00:00Z"), // valid_to
            DataValue::Null,                         // superseded_by
            DataValue::Null,                         // source_session_id
            DataValue::from("2026-03-01T00:00:00Z"), // recorded_at
            DataValue::from(0i64),                   // access_count
            DataValue::from(""),                     // last_accessed_at
            DataValue::from(720.0f64),               // stability_hours
            DataValue::from("observation"),
            DataValue::Bool(false),     // is_forgotten
            DataValue::Null,            // forgotten_at
            DataValue::Null,            // forget_reason
            DataValue::Null,            // scope
            DataValue::Null,            // project_id
            DataValue::from("private"), // visibility
            DataValue::from("public"),  // sensitivity
        ]
    }

    fn named_rows(row: Vec<DataValue>) -> NamedRows {
        NamedRows {
            headers: Vec::new(),
            rows: vec![row],
            next: None,
        }
    }

    /// Requirement #5848: `rows_to_facts` must preserve `Reflected` and `Training`
    /// epistemic tiers instead of silently downgrading them to `Assumed`.
    #[test]
    fn rows_to_facts_preserves_reflected_and_training_tiers() {
        for tier in [EpistemicTier::Reflected, EpistemicTier::Training] {
            let rows = named_rows(rows_to_facts_row(tier.as_str()));
            let facts = rows_to_facts(rows, "alice").expect("rows_to_facts must succeed");
            assert_eq!(
                facts.len(),
                1,
                "expected one fact for tier {}",
                tier.as_str()
            );
            let fact = facts.first().expect("asserted len == 1 above");
            assert_eq!(
                fact.provenance.tier,
                tier,
                "rows_to_facts must round-trip tier {}, not downgrade to Assumed",
                tier.as_str()
            );
        }
    }

    /// Requirement #5848: `rows_to_raw_facts` must preserve `Reflected` and
    /// `Training` epistemic tiers instead of silently downgrading them to `Assumed`.
    #[test]
    fn rows_to_raw_facts_preserves_reflected_and_training_tiers() {
        for tier in [EpistemicTier::Reflected, EpistemicTier::Training] {
            let rows = named_rows(rows_to_raw_facts_row(tier.as_str()));
            let facts = rows_to_raw_facts(rows).expect("rows_to_raw_facts must succeed");
            assert_eq!(
                facts.len(),
                1,
                "expected one fact for tier {}",
                tier.as_str()
            );
            let fact = facts.first().expect("asserted len == 1 above");
            assert_eq!(
                fact.provenance.tier,
                tier,
                "rows_to_raw_facts must round-trip tier {}, not downgrade to Assumed",
                tier.as_str()
            );
        }
    }
}
