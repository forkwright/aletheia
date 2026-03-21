//! Datalog query generation tests.
use super::*;

#[test]
fn test_put_generates_valid_datalog() {
    use FactsField::*;
    let script = QueryBuilder::new()
        .put(Relation::Facts)
        .keys(&[Id, ValidFrom])
        .values(&[Content, NousId])
        .done()
        .build_script();

    assert!(
        script.contains("?[id, valid_from, content, nous_id]"),
        "put script should have output head with all fields"
    );
    assert!(
        script.contains("[$id, $valid_from, $content, $nous_id]"),
        "put script should bind params for each field"
    );
    assert!(
        script.contains(":put facts {id, valid_from => content, nous_id}"),
        "put script should have key => value separation"
    );
}

#[test]
fn test_put_multi_row() {
    use FactsField::*;
    let script = QueryBuilder::new()
        .put(Relation::Facts)
        .keys(&[Id, ValidFrom])
        .values(&[Content])
        .row(&["$a", "$b", "$c"])
        .row(&["$x", "$y", "$z"])
        .done()
        .build_script();

    assert!(
        script.contains("[$a, $b, $c], [$x, $y, $z]"),
        "multi-row put should include both rows"
    );
    assert!(
        script.contains(":put facts {id, valid_from => content}"),
        "multi-row put should have correct relation directive"
    );
}

#[test]
fn test_scan_generates_valid_datalog() {
    use FactsField::*;
    let script = QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Content, Confidence])
        .bind(Id)
        .bind_to(NousId, "$nous_id")
        .bind(Content)
        .bind(Confidence)
        .filter("confidence > 0.5")
        .order("-confidence")
        .limit("$limit")
        .done()
        .build_script();

    assert!(
        script.contains("?[id, content, confidence]"),
        "scan script should have output head with selected fields"
    );
    assert!(
        script.contains("*facts{id, nous_id: $nous_id, content, confidence}"),
        "scan script should bind nous_id to named param"
    );
    assert!(
        script.contains("confidence > 0.5"),
        "scan script should include filter expression"
    );
    assert!(
        script.contains(":order -confidence"),
        "scan script should include order directive"
    );
    assert!(
        script.contains(":limit $limit"),
        "scan script should include limit directive"
    );
}

#[test]
fn test_params_are_bound_not_interpolated() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := x = $val")
        .param("val", DataValue::from(42_i64))
        .build();

    assert!(script.contains("$val"), "script must reference $val");
    assert!(
        !script.contains("42"),
        "script must not contain literal value"
    );
    assert!(
        params.contains_key("val"),
        "param map should contain the bound value"
    );
}

#[test]
fn test_injection_attempt() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{id: $id, content: x}")
        .param("id", DataValue::Str("evil}; :rm facts".into()))
        .build();

    assert!(
        !script.contains("evil}"),
        "injection payload must not appear in script"
    );
    assert!(
        params.contains_key("id"),
        "param map should contain the injected id"
    );
}

#[test]
fn test_order_and_limit() {
    use EntitiesField::*;
    let script = QueryBuilder::new()
        .scan(Relation::Entities)
        .select(&[Id, Name])
        .bind(Id)
        .bind(Name)
        .order("name")
        .limit("10")
        .done()
        .build_script();

    let lines: Vec<&str> = script.lines().collect();
    let order_pos = lines.iter().position(|l| l.contains(":order"));
    let limit_pos = lines.iter().position(|l| l.contains(":limit"));
    assert!(order_pos.is_some(), "must have :order");
    assert!(limit_pos.is_some(), "must have :limit");
    assert!(
        order_pos.expect(":order directive must be present")
            < limit_pos.expect(":limit directive must be present"),
        ":order must come before :limit"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "comprehensive schema field validation"
)]
fn test_field_names_match_schema() {
    let facts_ddl_fields = [
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
    let facts_enum_fields: Vec<&str> = [
        FactsField::Id,
        FactsField::ValidFrom,
        FactsField::Content,
        FactsField::NousId,
        FactsField::Confidence,
        FactsField::Tier,
        FactsField::ValidTo,
        FactsField::SupersededBy,
        FactsField::SourceSessionId,
        FactsField::RecordedAt,
        FactsField::AccessCount,
        FactsField::LastAccessedAt,
        FactsField::StabilityHours,
        FactsField::FactType,
        FactsField::IsForgotten,
        FactsField::ForgottenAt,
        FactsField::ForgetReason,
    ]
    .iter()
    .map(|f| f.name())
    .collect();
    assert_eq!(
        facts_ddl_fields.as_slice(),
        facts_enum_fields.as_slice(),
        "FactsField enum names should match DDL field names in order"
    );

    let entities_ddl = [
        "id",
        "name",
        "entity_type",
        "aliases",
        "created_at",
        "updated_at",
    ];
    let entities_enum: Vec<&str> = [
        EntitiesField::Id,
        EntitiesField::Name,
        EntitiesField::EntityType,
        EntitiesField::Aliases,
        EntitiesField::CreatedAt,
        EntitiesField::UpdatedAt,
    ]
    .iter()
    .map(|f| f.name())
    .collect();
    assert_eq!(
        entities_ddl.as_slice(),
        entities_enum.as_slice(),
        "EntitiesField enum names should match DDL field names in order"
    );

    let rels_ddl = ["src", "dst", "relation", "weight", "created_at"];
    let rels_enum: Vec<&str> = [
        RelationshipsField::Src,
        RelationshipsField::Dst,
        RelationshipsField::Relation,
        RelationshipsField::Weight,
        RelationshipsField::CreatedAt,
    ]
    .iter()
    .map(|f| f.name())
    .collect();
    assert_eq!(
        rels_ddl.as_slice(),
        rels_enum.as_slice(),
        "RelationshipsField enum names should match DDL field names in order"
    );

    let emb_ddl = [
        "id",
        "content",
        "source_type",
        "source_id",
        "nous_id",
        "embedding",
        "created_at",
    ];
    let emb_enum: Vec<&str> = [
        EmbeddingsField::Id,
        EmbeddingsField::Content,
        EmbeddingsField::SourceType,
        EmbeddingsField::SourceId,
        EmbeddingsField::NousId,
        EmbeddingsField::Embedding,
        EmbeddingsField::CreatedAt,
    ]
    .iter()
    .map(|f| f.name())
    .collect();
    assert_eq!(
        emb_ddl.as_slice(),
        emb_enum.as_slice(),
        "EmbeddingsField enum names should match DDL field names in order"
    );
}
