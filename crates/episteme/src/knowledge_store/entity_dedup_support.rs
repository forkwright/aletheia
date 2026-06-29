use super::marshal::extract_str;
use super::{KnowledgeStore, queries};

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Transfer `fact_entities` mappings from merged entity to canonical.
    pub(super) fn transfer_fact_entities(
        &self,
        from_id: &crate::id::EntityId,
        to_id: &crate::id::EntityId,
    ) -> crate::error::Result<u32> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(
            "from_id".to_owned(),
            DataValue::Str(from_id.as_str().into()),
        );
        let script = r"?[fact_id, entity_id, created_at] :=
            *fact_entities{fact_id, entity_id, created_at},
            entity_id = $from_id";
        let rows = self.run_read(script, params)?;

        let count = rows.rows.len();
        for row in &rows.rows {
            if row.len() < 3 {
                return Err(short_row_error("fact entity transfer row", 3, row.len()));
            }
            let fact_id = row
                .first()
                .ok_or_else(|| short_row_error("fact entity transfer row", 3, row.len()))
                .and_then(extract_str)?;
            let created_at = row
                .get(2)
                .ok_or_else(|| short_row_error("fact entity transfer row", 3, row.len()))
                .and_then(extract_str)?;

            let mut put_params = BTreeMap::new();
            put_params.insert(
                "fact_id".to_owned(),
                DataValue::Str(fact_id.as_str().into()),
            );
            put_params.insert(
                "entity_id".to_owned(),
                DataValue::Str(to_id.as_str().into()),
            );
            put_params.insert("created_at".to_owned(), DataValue::Str(created_at.into()));
            self.run_mut(&queries::upsert_fact_entity(), put_params)?;

            let mut rm_params = BTreeMap::new();
            rm_params.insert("fact_id".to_owned(), DataValue::Str(fact_id.into()));
            rm_params.insert(
                "entity_id".to_owned(),
                DataValue::Str(from_id.as_str().into()),
            );
            // kanon:ignore RUST/no-silent-result-swallow — stale row cleanup after merge; non-fatal if missing
            let _ = self.run_mut(&queries::rm_fact_entity(), rm_params);
        }

        usize_to_u32(count, "transferred fact entity count")
    }

    /// Add an alias to an entity's alias list.
    pub(super) fn add_alias_to_entity(
        &self,
        entity_id: &crate::id::EntityId,
        new_alias: &str,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{Array1, DataValue, Vector};
        let entity = self.load_entity(entity_id)?;
        let lower_new = new_alias.to_lowercase();

        if entity.name.to_lowercase() == lower_new
            || entity.aliases.iter().any(|a| a.to_lowercase() == lower_new)
        {
            return Ok(());
        }

        let mut aliases = entity.aliases;
        aliases.push(new_alias.to_owned());
        let aliases_str = aliases.join(",");

        // WHY (#4165 Path A): the entities upsert requires
        // `$name_embedding` — preserve the existing column value so the
        // alias update does not silently clear a populated embedding.
        let existing_embedding = self.get_entity_name_embedding(entity_id)?;
        let emb_value = existing_embedding.map_or(DataValue::Null, |v| {
            DataValue::Vec(Vector::F32(Array1::from(v)))
        });

        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        params.insert("aliases".to_owned(), DataValue::Str(aliases_str.into()));
        params.insert(
            "updated_at".to_owned(),
            DataValue::Str(crate::knowledge::format_timestamp(&jiff::Timestamp::now()).into()),
        );
        params.insert("name".to_owned(), DataValue::Str(entity.name.into()));
        params.insert(
            "entity_type".to_owned(),
            DataValue::Str(entity.entity_type.into()),
        );
        params.insert(
            "created_at".to_owned(),
            DataValue::Str(crate::knowledge::format_timestamp(&entity.created_at).into()),
        );
        params.insert("name_embedding".to_owned(), emb_value);
        self.run_mut(&queries::upsert_entity(), params)
    }

    /// Store a pending merge candidate for review.
    pub(super) fn store_pending_merge(
        &self,
        nous_id: &str,
        candidate: &crate::dedup::EntityMergeCandidate,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert(
            "entity_a".to_owned(),
            DataValue::Str(candidate.entity_a.as_str().into()),
        );
        params.insert(
            "entity_b".to_owned(),
            DataValue::Str(candidate.entity_b.as_str().into()),
        );
        params.insert(
            "name_a".to_owned(),
            DataValue::Str(candidate.name_a.as_str().into()),
        );
        params.insert(
            "name_b".to_owned(),
            DataValue::Str(candidate.name_b.as_str().into()),
        );
        params.insert(
            "name_similarity".to_owned(),
            DataValue::from(candidate.name_similarity),
        );
        params.insert(
            "embed_similarity".to_owned(),
            DataValue::from(candidate.embed_similarity),
        );
        params.insert(
            "type_match".to_owned(),
            DataValue::Bool(candidate.type_match),
        );
        params.insert(
            "alias_overlap".to_owned(),
            DataValue::Bool(candidate.alias_overlap),
        );
        params.insert(
            "merge_score".to_owned(),
            DataValue::from(candidate.merge_score),
        );
        params.insert("created_at".to_owned(), DataValue::Str(now.into()));
        self.run_mut(&queries::put_pending_merge(), params)
    }
}

#[cfg(feature = "mneme-engine")]
pub(super) fn short_row_error(
    context: &str,
    expected: usize,
    actual: usize,
) -> crate::error::Error {
    crate::error::ConversionSnafu {
        message: format!("{context}: expected at least {expected} columns, got {actual}"),
    }
    .build()
}

#[cfg(feature = "mneme-engine")]
pub(super) fn strict_timestamp(raw: &str, context: &str) -> crate::error::Result<jiff::Timestamp> {
    crate::knowledge::parse_timestamp(raw).ok_or_else(|| {
        crate::error::EngineQuerySnafu {
            message: format!("{context}: invalid timestamp '{raw}'"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
pub(super) fn checked_u32(value: i64, context: &str) -> crate::error::Result<u32> {
    u32::try_from(value).map_err(|err| {
        crate::error::ConversionSnafu {
            message: format!("{context}: cannot convert {value} to u32: {err}"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
fn usize_to_u32(value: usize, context: &str) -> crate::error::Result<u32> {
    u32::try_from(value).map_err(|err| {
        crate::error::ConversionSnafu {
            message: format!("{context}: cannot convert {value} to u32: {err}"),
        }
        .build()
    })
}
