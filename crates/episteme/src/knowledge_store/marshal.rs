#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with direct indexing throughout"
)]
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

/// Compute Jaccard overlap between two tool lists.
///
/// Returns 1.0 for identical sets, 0.0 for disjoint.
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "tool set sizes are small; precision loss is impossible in practice"
)]
pub(super) fn compute_tool_overlap(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let set_a: std::collections::HashSet<&str> = a.iter().map(String::as_str).collect();
    let set_b: std::collections::HashSet<&str> = b.iter().map(String::as_str).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 1.0;
    }
    intersection as f64 / union as f64 // SAFETY: set counts fit f64
}

/// Compute name similarity using longest common subsequence ratio.
///
/// Returns 1.0 for identical names, 0.0 for completely different.
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "string lengths are small; precision loss is impossible in practice"
)]
pub(super) fn compute_name_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let a_chars: Vec<char> = a_lower.chars().collect();
    let b_chars: Vec<char> = b_lower.chars().collect();
    let max_len = a_chars.len().max(b_chars.len());
    if max_len == 0 {
        return 1.0;
    }
    let lcs = lcs_char_length(&a_chars, &b_chars);
    lcs as f64 / max_len as f64 // SAFETY: string lengths fit f64
}

/// Classic DP Longest Common Subsequence length for char slices.
#[cfg(feature = "mneme-engine")]
fn lcs_char_length(a: &[char], b: &[char]) -> usize {
    let m = a.len();
    let n = b.len();
    let mut dp = vec![0usize; (m + 1) * (n + 1)];
    let idx = |i: usize, j: usize| i * (n + 1) + j;
    for i in 1..=m {
        for j in 1..=n {
            dp[idx(i, j)] = if a[i - 1] == b[j - 1] {
                dp[idx(i - 1, j - 1)] + 1
            } else {
                dp[idx(i - 1, j)].max(dp[idx(i, j - 1)])
            };
        }
    }
    dp[idx(m, n)]
}

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
        let scope = row
            .get(17)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| s.parse::<crate::knowledge::MemoryScope>().ok());
        let project_id = row
            .get(18)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok());
        let visibility = row
            .get(19)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default()
            .parse::<crate::knowledge::Visibility>()
            .unwrap_or_default();
        let sensitivity = parse_fact_sensitivity(row.get(20));

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
        let scope = row
            .get(17)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| s.parse::<crate::knowledge::MemoryScope>().ok());
        let project_id = row
            .get(18)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok());
        let visibility = row
            .get(19)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default()
            .parse::<crate::knowledge::Visibility>()
            .unwrap_or_default();
        let sensitivity = parse_fact_sensitivity(row.get(20));
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
        let sensitivity = parse_fact_sensitivity(row.get(4));

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
        let sensitivity = parse_fact_sensitivity(row.get(9));
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
pub(super) fn build_hybrid_query(q: &super::HybridQuery) -> String {
    use super::queries;

    // WHY: When the message has no full-text terms (e.g. an emoji- or
    // punctuation-only message that `sanitize_fts_query` reduces to ""), the FTS
    // `query: ""` argument is itself an FTS parse error (#4156). Emit an empty
    // `bm25` relation in that case so vector + graph search still run; the
    // `$query_text` param is then left unbound by the caller.
    let bm25_rule = if sanitize_fts_query(&q.text).is_empty() {
        "bm25[id, score] <- []".to_owned()
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
        format!(
            "seed_list[e] <- [{seeds_inline}]\n        \
             graph_raw[id, score] := seed_list[seed], *relationships{{src: seed, dst: id, weight: score}}\n        \
             graph_raw[id, score] := seed_list[seed], *relationships{{src: seed, dst: mid, weight: _w}}, \
             *relationships{{src: mid, dst: id, weight}}, score = weight * 0.5\n        \
             graph[id, sum(score)] := graph_raw[id, score]"
        )
    };
    queries::HYBRID_SEARCH_BASE
        .replace("{BM25_RULE}", &bm25_rule)
        .replace("{GRAPH_RULES}", &graph_rules)
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

#[cfg(feature = "mneme-engine")]
fn parse_fact_sensitivity(
    val: Option<&crate::engine::DataValue>,
) -> crate::knowledge::FactSensitivity {
    val.and_then(|v| extract_str(v).ok())
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<crate::knowledge::FactSensitivity>().ok())
        .unwrap_or_default()
}

#[cfg(feature = "mneme-engine")]
pub(super) fn parse_epistemic_tier(s: &str) -> crate::knowledge::EpistemicTier {
    use crate::knowledge::EpistemicTier;
    match s {
        "verified" => EpistemicTier::Verified,
        "inferred" => EpistemicTier::Inferred,
        "assumed" => EpistemicTier::Assumed,
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
    use crate::engine::DataValue;
    use crate::knowledge::EpistemicTier;

    // ── compute_tool_overlap — Jaccard similarity on string slices ──────────

    #[test]
    fn tool_overlap_identical_sets() {
        let a = vec!["read".to_owned(), "write".to_owned(), "bash".to_owned()];
        let b = vec!["read".to_owned(), "write".to_owned(), "bash".to_owned()];
        assert!(
            (compute_tool_overlap(&a, &b) - 1.0).abs() < f64::EPSILON,
            "identical sets should yield overlap 1.0"
        );
    }

    #[test]
    fn tool_overlap_disjoint_sets() {
        let a = vec!["read".to_owned(), "write".to_owned()];
        let b = vec!["bash".to_owned(), "grep".to_owned()];
        assert!(
            compute_tool_overlap(&a, &b).abs() < f64::EPSILON,
            "disjoint sets should yield overlap 0.0"
        );
    }

    #[test]
    fn tool_overlap_partial_intersection() {
        // 2 shared of 4 total = 0.5
        let a = vec!["read".to_owned(), "write".to_owned(), "bash".to_owned()];
        let b = vec!["read".to_owned(), "write".to_owned(), "grep".to_owned()];
        let result = compute_tool_overlap(&a, &b);
        assert!(
            (result - 0.5).abs() < f64::EPSILON,
            "expected 0.5, got {result}"
        );
    }

    #[test]
    fn tool_overlap_both_empty_returns_one() {
        let a: Vec<String> = Vec::new();
        let b: Vec<String> = Vec::new();
        assert!(
            (compute_tool_overlap(&a, &b) - 1.0).abs() < f64::EPSILON,
            "both-empty tool sets should yield overlap 1.0"
        );
    }

    #[test]
    fn tool_overlap_one_empty_returns_zero() {
        let a = vec!["read".to_owned()];
        let b: Vec<String> = Vec::new();
        assert!(
            compute_tool_overlap(&a, &b).abs() < f64::EPSILON,
            "one-empty tool set should yield overlap 0.0"
        );
    }

    #[test]
    fn tool_overlap_duplicates_deduplicated() {
        // Duplicates in input should be collapsed by the HashSet
        let a = vec!["read".to_owned(), "read".to_owned(), "write".to_owned()];
        let b = vec!["read".to_owned(), "write".to_owned()];
        assert!(
            (compute_tool_overlap(&a, &b) - 1.0).abs() < f64::EPSILON,
            "duplicates in sets should not affect overlap 1.0"
        );
    }

    // ── compute_name_similarity — LCS ratio ─────────────────────────────────

    #[test]
    fn name_similarity_identical() {
        assert!(
            (compute_name_similarity("Alice", "Alice") - 1.0).abs() < f64::EPSILON,
            "identical names should yield similarity 1.0"
        );
    }

    #[test]
    fn name_similarity_case_insensitive() {
        // LCS runs on lowercased strings
        assert!(
            (compute_name_similarity("Alice", "alice") - 1.0).abs() < f64::EPSILON,
            "case-insensitive match should yield similarity 1.0"
        );
    }

    #[test]
    fn name_similarity_completely_different() {
        // "abc" vs "xyz" — LCS length 0, ratio 0.0
        assert!(
            compute_name_similarity("abc", "xyz").abs() < f64::EPSILON,
            "disjoint names should yield similarity 0.0"
        );
    }

    #[test]
    fn name_similarity_substring_match() {
        // "kitten" vs "kit" — LCS = "kit" (3), max_len = 6, ratio = 0.5
        let result = compute_name_similarity("kitten", "kit");
        assert!(
            (result - 0.5).abs() < f64::EPSILON,
            "expected 0.5, got {result}"
        );
    }

    #[test]
    fn name_similarity_both_empty() {
        assert!(
            (compute_name_similarity("", "") - 1.0).abs() < f64::EPSILON,
            "both-empty names should yield similarity 1.0"
        );
    }

    #[test]
    fn name_similarity_one_empty() {
        assert!(
            compute_name_similarity("hello", "").abs() < f64::EPSILON,
            "one-empty name should yield similarity 0.0"
        );
    }

    // ── lcs_char_length — the DP kernel ─────────────────────────────────────

    #[test]
    fn lcs_exact_match() {
        let a: Vec<char> = "abc".chars().collect();
        let b: Vec<char> = "abc".chars().collect();
        assert_eq!(
            lcs_char_length(&a, &b),
            3,
            "LCS of exact match should equal input length"
        );
    }

    #[test]
    fn lcs_partial_match() {
        // "abcde" vs "ace" → LCS = "ace" (3)
        let a: Vec<char> = "abcde".chars().collect();
        let b: Vec<char> = "ace".chars().collect();
        assert_eq!(
            lcs_char_length(&a, &b),
            3,
            "LCS of abcde vs ace should be 3"
        );
    }

    #[test]
    fn lcs_no_match() {
        let a: Vec<char> = "abc".chars().collect();
        let b: Vec<char> = "xyz".chars().collect();
        assert_eq!(
            lcs_char_length(&a, &b),
            0,
            "LCS of disjoint char sets should be 0"
        );
    }

    #[test]
    fn lcs_empty_inputs() {
        let empty: Vec<char> = Vec::new();
        let a: Vec<char> = "abc".chars().collect();
        assert_eq!(lcs_char_length(&empty, &a), 0, "LCS(empty, a) should be 0");
        assert_eq!(lcs_char_length(&a, &empty), 0, "LCS(a, empty) should be 0");
        assert_eq!(
            lcs_char_length(&empty, &empty),
            0,
            "LCS(empty, empty) should be 0"
        );
    }

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
            text: "cozo databases".to_owned(),
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
            sanitize_fts_query("what do you remember about cozo?"),
            "what do you remember about cozo"
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
}
