#[cfg(feature = "mneme-engine")]
pub(super) fn fact_to_params(
    fact: &crate::knowledge::Fact,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::DataValue;
    use crate::knowledge::format_timestamp;
    let mut p = std::collections::BTreeMap::new();
    p.insert("id".to_owned(), DataValue::Str(fact.id.as_str().into()));
    p.insert(
        "valid_from".to_owned(),
        DataValue::Str(format_timestamp(&fact.valid_from).into()),
    );
    p.insert(
        "content".to_owned(),
        DataValue::Str(fact.content.as_str().into()),
    );
    p.insert(
        "nous_id".to_owned(),
        DataValue::Str(fact.nous_id.as_str().into()),
    );
    p.insert("confidence".to_owned(), DataValue::from(fact.confidence));
    p.insert("tier".to_owned(), DataValue::Str(fact.tier.as_str().into()));
    p.insert(
        "valid_to".to_owned(),
        DataValue::Str(format_timestamp(&fact.valid_to).into()),
    );
    p.insert(
        "superseded_by".to_owned(),
        match &fact.superseded_by {
            Some(s) => DataValue::Str(s.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "source_session_id".to_owned(),
        match &fact.source_session_id {
            Some(s) => DataValue::Str(s.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "recorded_at".to_owned(),
        DataValue::Str(format_timestamp(&fact.recorded_at).into()),
    );
    p.insert(
        "access_count".to_owned(),
        DataValue::from(i64::from(fact.access_count)),
    );
    p.insert(
        "last_accessed_at".to_owned(),
        match &fact.last_accessed_at {
            Some(ts) => DataValue::Str(format_timestamp(ts).into()),
            None => DataValue::Str("".into()),
        },
    );
    p.insert(
        "stability_hours".to_owned(),
        DataValue::from(fact.stability_hours),
    );
    p.insert(
        "fact_type".to_owned(),
        DataValue::Str(fact.fact_type.as_str().into()),
    );
    p.insert(
        "is_forgotten".to_owned(),
        DataValue::Bool(fact.is_forgotten),
    );
    p.insert(
        "forgotten_at".to_owned(),
        match &fact.forgotten_at {
            Some(ts) => DataValue::Str(format_timestamp(ts).into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "forget_reason".to_owned(),
        match &fact.forget_reason {
            Some(r) => DataValue::Str(r.as_str().into()),
            None => DataValue::Null,
        },
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
    intersection as f64 / union as f64
}

/// Compute name similarity using longest common subsequence ratio.
///
/// Returns 1.0 for identical names, 0.0 for completely different.
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::cast_precision_loss,
    reason = "name lengths are small; precision loss is impossible in practice"
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
    lcs as f64 / max_len as f64
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

        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "access count fits in u32"
        )]
        let access_count = row.get(10).and_then(|v| extract_int(v).ok()).unwrap_or(0) as u32;
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

        out.push(Fact {
            id: crate::id::FactId::new_unchecked(id),
            nous_id: if nous_id_col.is_empty() {
                nous_id.to_owned()
            } else {
                nous_id_col
            },
            content,
            confidence,
            tier,
            valid_from: crate::knowledge::parse_timestamp(&valid_from)
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
            valid_to: crate::knowledge::parse_timestamp(&valid_to)
                .unwrap_or_else(crate::knowledge::far_future),
            superseded_by: superseded_by.map(crate::id::FactId::new_unchecked),
            source_session_id,
            recorded_at: crate::knowledge::parse_timestamp(&recorded_at)
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
            access_count,
            last_accessed_at: crate::knowledge::parse_timestamp(&last_accessed_at),
            stability_hours,
            fact_type,
            is_forgotten,
            forgotten_at: forgotten_at.and_then(|s| crate::knowledge::parse_timestamp(&s)),
            forget_reason,
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
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "access count fits in u32"
        )]
        let access_count = row.get(10).and_then(|v| extract_int(v).ok()).unwrap_or(0) as u32;
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
        out.push(Fact {
            id: crate::id::FactId::new_unchecked(id),
            nous_id,
            content,
            confidence,
            tier,
            valid_from: crate::knowledge::parse_timestamp(&valid_from)
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
            valid_to: crate::knowledge::parse_timestamp(&valid_to)
                .unwrap_or_else(crate::knowledge::far_future),
            superseded_by: superseded_by.map(crate::id::FactId::new_unchecked),
            source_session_id,
            recorded_at: crate::knowledge::parse_timestamp(&recorded_at)
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
            access_count,
            last_accessed_at: crate::knowledge::parse_timestamp(&last_accessed_at),
            stability_hours,
            fact_type,
            is_forgotten,
            forgotten_at: forgotten_at.and_then(|s| crate::knowledge::parse_timestamp(&s)),
            forget_reason,
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

        out.push(Fact {
            id: crate::id::FactId::new_unchecked(id),
            nous_id: String::new(),
            content,
            confidence,
            tier,
            valid_from: jiff::Timestamp::UNIX_EPOCH,
            valid_to: crate::knowledge::far_future(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: jiff::Timestamp::UNIX_EPOCH,
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
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

        out.push(RecallResult {
            content,
            distance,
            source_type,
            source_id,
        });
    }
    Ok(out)
}

#[cfg(feature = "mneme-engine")]
pub(super) fn build_hybrid_query(q: &super::HybridQuery) -> String {
    use super::queries;

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
    queries::HYBRID_SEARCH_BASE.replace("{GRAPH_RULES}", &graph_rules)
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
        out.push(super::HybridResult {
            id: crate::id::FactId::new_unchecked(id),
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
