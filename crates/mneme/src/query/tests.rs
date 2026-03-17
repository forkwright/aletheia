use super::*;
use crate::engine::DataValue;

/// Normalize whitespace for comparison: collapse runs of whitespace to single
/// space, trim, then remove spaces adjacent to brackets/braces (`CozoDB`
/// ignores these formatting differences).
fn normalize(s: &str) -> String {
    let collapsed: String = s.split_whitespace().fold(String::new(), |mut acc, word| {
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(word);
        acc
    });
    collapsed
        .replace("[ ", "[")
        .replace(" ]", "]")
        .replace("{ ", "{")
        .replace(" }", "}")
}

// -- Builder unit tests --

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
    // Param values with special chars go through $binding, not interpolation
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
    // Facts DDL fields
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

    // Entities DDL fields
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

    // Relationships DDL fields
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

    // Embeddings DDL fields
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

#[test]
fn test_raw_escape_hatch() {
    let script = QueryBuilder::new()
        .raw("hop1[dst, rel] := *relationships{src: $id, dst, relation: rel}")
        .raw("?[dst, rel] := hop1[dst, rel]")
        .build_script();

    assert!(
        script.contains("hop1[dst, rel]"),
        "raw script should include derived rule head"
    );
    assert!(
        script.contains("*relationships{src: $id"),
        "raw script should include relation scan with param"
    );
}

// -- Regression: builder output matches original constants --

#[test]
fn test_builder_matches_upsert_fact() {
    let original = r"
    ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
      superseded_by, source_session_id, recorded_at,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason] <- [[$id, $valid_from,
      $content, $nous_id, $confidence, $tier, $valid_to, $superseded_by,
      $source_session_id, $recorded_at,
      $access_count, $last_accessed_at, $stability_hours, $fact_type,
      $is_forgotten, $forgotten_at, $forget_reason]]
    :put facts {id, valid_from => content, nous_id, confidence, tier,
                valid_to, superseded_by, source_session_id, recorded_at,
                access_count, last_accessed_at, stability_hours, fact_type,
                is_forgotten, forgotten_at, forget_reason}
";
    let built = queries::upsert_fact();
    assert_eq!(
        normalize(&built),
        normalize(original),
        "builder output should match upsert_fact constant"
    );
}

#[test]
fn test_builder_matches_current_facts() {
    let original = r"
    ?[id, content, confidence, tier, recorded_at] :=
        *facts{id, valid_from, content, nous_id, confidence, tier,
               valid_to, superseded_by, recorded_at,
               access_count, last_accessed_at, stability_hours, fact_type,
               is_forgotten, forgotten_at, forget_reason},
        nous_id = $nous_id,
        valid_from <= $now,
        valid_to > $now,
        is_null(superseded_by),
        is_forgotten == false
    :order -confidence
    :limit $limit
";
    let built = queries::current_facts();
    assert_eq!(
        normalize(&built),
        normalize(original),
        "builder output should match current_facts constant"
    );
}

#[test]
fn test_builder_matches_facts_at_time() {
    let original = r"
    ?[id, content, confidence, tier] :=
        *facts{id, valid_from, content, confidence, tier, valid_to, is_forgotten},
        valid_from <= $time,
        valid_to > $time,
        is_forgotten == false
";
    let built = queries::facts_at_time();
    assert_eq!(
        normalize(&built),
        normalize(original),
        "builder output should match facts_at_time constant"
    );
}

#[test]
fn test_builder_matches_supersede_fact() {
    let original = r#"
    ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
      superseded_by, source_session_id, recorded_at,
      access_count, last_accessed_at, stability_hours, fact_type,
      is_forgotten, forgotten_at, forget_reason] <- [
        [$old_id, $old_valid_from, $old_content, $nous_id, $old_confidence,
         $old_tier, $now, $new_id, $old_source, $old_recorded,
         $old_access_count, $old_last_accessed_at, $old_stability_hours, $old_fact_type,
         $old_is_forgotten, $old_forgotten_at, $old_forget_reason],
        [$new_id, $now, $new_content, $nous_id, $new_confidence,
         $new_tier, "9999-12-31", null, $source_session_id, $now,
         0, "", $stability_hours, $fact_type,
         false, null, null]
    ]
    :put facts {id, valid_from => content, nous_id, confidence, tier,
                valid_to, superseded_by, source_session_id, recorded_at,
                access_count, last_accessed_at, stability_hours, fact_type,
                is_forgotten, forgotten_at, forget_reason}
"#;
    let built = queries::supersede_fact();
    assert_eq!(
        normalize(&built),
        normalize(original),
        "builder output should match supersede_fact constant"
    );
}

#[test]
fn test_builder_matches_upsert_entity() {
    let original = r"
    ?[id, name, entity_type, aliases, created_at, updated_at] <- [
        [$id, $name, $entity_type, $aliases, $created_at, $updated_at]
    ]
    :put entities {id => name, entity_type, aliases, created_at, updated_at}
";
    let built = queries::upsert_entity();
    assert_eq!(
        normalize(&built),
        normalize(original),
        "builder output should match upsert_entity constant"
    );
}

#[test]
fn test_builder_matches_upsert_relationship() {
    let original = r"
    ?[src, dst, relation, weight, created_at] <- [
        [$src, $dst, $relation, $weight, $created_at]
    ]
    :put relationships {src, dst => relation, weight, created_at}
";
    let built = queries::upsert_relationship();
    assert_eq!(
        normalize(&built),
        normalize(original),
        "builder output should match upsert_relationship constant"
    );
}

#[test]
fn test_builder_matches_upsert_embedding() {
    let original = r"?[id, content, source_type, source_id, nous_id, embedding, created_at] <- [
            [$id, $content, $source_type, $source_id, $nous_id, $embedding, $created_at]
          ]
          :put embeddings { id => content, source_type, source_id, nous_id, embedding, created_at }";
    let built = queries::upsert_embedding();
    assert_eq!(
        normalize(&built),
        normalize(original),
        "builder output should match upsert_embedding constant"
    );
}

#[test]
fn test_builder_matches_full_current_facts() {
    let original = r"
?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to, superseded_by, source_session_id,
  access_count, last_accessed_at, stability_hours, fact_type,
  is_forgotten, forgotten_at, forget_reason] :=
    *facts{id, valid_from, content, nous_id, confidence, tier,
           valid_to, superseded_by, source_session_id, recorded_at,
           access_count, last_accessed_at, stability_hours, fact_type,
           is_forgotten, forgotten_at, forget_reason},
    nous_id = $nous_id,
    valid_from <= $now,
    valid_to > $now,
    is_null(superseded_by),
    is_forgotten == false
:order -confidence
:limit $limit
";
    let built = queries::full_current_facts();
    assert_eq!(
        normalize(&built),
        normalize(original),
        "builder output should match full_current_facts constant"
    );
}

#[test]
fn query_builder_prevents_injection() {
    let malicious_input = r#"test" :- *drop_all[], panic"#;
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{id: $user_input, content: x}")
        .param("user_input", DataValue::from(malicious_input))
        .build();

    assert!(
        !script.contains(malicious_input),
        "raw malicious input must not appear in script body"
    );
    assert!(
        script.contains("$user_input"),
        "script must use parameter binding"
    );
    assert!(
        params.contains_key("user_input"),
        "malicious input must be in params map"
    );
}

#[test]
fn query_builder_all_field_types() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{id: $str_val, content: x}")
        .param("str_val", DataValue::from("hello"))
        .param("int_val", DataValue::from(42_i64))
        .param("float_val", DataValue::from(2.72_f64))
        .param("bool_val", DataValue::from(true))
        .param("null_val", DataValue::Null)
        .build();

    assert!(
        params.contains_key("str_val"),
        "param map should contain str_val"
    );
    assert!(
        params.contains_key("int_val"),
        "param map should contain int_val"
    );
    assert!(
        params.contains_key("float_val"),
        "param map should contain float_val"
    );
    assert!(
        params.contains_key("bool_val"),
        "param map should contain bool_val"
    );
    assert!(
        params.contains_key("null_val"),
        "param map should contain null_val"
    );
    assert_eq!(
        params.len(),
        5,
        "param map should have exactly five entries"
    );

    assert!(
        !script.contains("hello"),
        "string literal must not leak into script"
    );
    assert!(
        !script.contains("42"),
        "int literal must not leak into script"
    );
    assert!(
        !script.contains("3.14"),
        "float literal must not leak into script"
    );
}

#[test]
fn query_builder_compound_filters() {
    use FactsField::*;
    let script = QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Content, Confidence])
        .bind(Id)
        .bind(Content)
        .bind(Confidence)
        .bind(NousId)
        .bind(Tier)
        .filter("nous_id = $nous_id")
        .filter("confidence > 0.5")
        .filter("tier != \"assumed\"")
        .done()
        .build_script();

    assert!(script.contains("nous_id = $nous_id"), "first filter");
    assert!(script.contains("confidence > 0.5"), "second filter");
    assert!(script.contains("tier != \"assumed\""), "third filter");

    let filter_count = script.matches(",\n").count();
    assert!(
        filter_count >= 3,
        "filters must be comma-separated in conjunction (got {filter_count})"
    );
}

#[test]
fn query_builder_empty_filter() {
    use FactsField::*;
    let script = QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Content])
        .bind(Id)
        .bind(Content)
        .done()
        .build_script();

    assert!(script.contains("?[id, content]"), "select list present");
    assert!(script.contains("*facts{id, content}"), "scan present");
    assert!(!script.contains(":order"), "no order when not specified");
    assert!(!script.contains(":limit"), "no limit when not specified");
}

#[test]
fn query_builder_empty_returns_valid_datalog() {
    let script = QueryBuilder::new().build_script();
    assert!(script.is_empty(), "empty builder produces empty script");

    let (script, params) = QueryBuilder::new().build();
    assert!(
        script.is_empty(),
        "empty builder script should be empty string"
    );
    assert!(
        params.is_empty(),
        "empty builder params should be empty map"
    );
}

#[test]
fn query_builder_string_field() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{id: $name, content: x}")
        .param("name", DataValue::from("alice"))
        .build();

    assert!(
        script.contains("$name"),
        "script should reference $name parameter"
    );
    assert!(
        !script.contains("alice"),
        "string value must not appear in script body"
    );
    assert_eq!(
        params.get("name").expect("name param must exist"),
        &DataValue::from("alice"),
        "name param should have the correct value"
    );
}

#[test]
fn query_builder_int_field() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{id: x}, x > $threshold")
        .param("threshold", DataValue::from(100_i64))
        .build();

    assert!(
        script.contains("$threshold"),
        "script should reference $threshold parameter"
    );
    assert!(
        !script.contains("100"),
        "int value must not appear in script body"
    );
    assert_eq!(
        params.get("threshold").expect("threshold param must exist"),
        &DataValue::from(100_i64),
        "threshold param should have the correct value"
    );
}

#[test]
fn query_builder_float_field() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{confidence: x}, x >= $min_conf")
        .param("min_conf", DataValue::from(0.75_f64))
        .build();

    assert!(
        script.contains("$min_conf"),
        "script should reference $min_conf parameter"
    );
    assert!(
        params.contains_key("min_conf"),
        "param map should contain min_conf"
    );
    assert_eq!(
        params.get("min_conf").expect("min_conf param must exist"),
        &DataValue::from(0.75_f64),
        "min_conf param should have the correct value"
    );
}

#[test]
fn query_builder_bool_field() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{is_forgotten: $forgotten, content: x}")
        .param("forgotten", DataValue::from(false))
        .build();

    assert!(
        script.contains("$forgotten"),
        "script should reference $forgotten parameter"
    );
    assert!(
        params.contains_key("forgotten"),
        "param map should contain forgotten"
    );
    assert_eq!(
        params.get("forgotten").expect("forgotten param must exist"),
        &DataValue::from(false),
        "forgotten param should have the correct value"
    );
}

#[test]
fn query_builder_timestamp_field() {
    let ts = "2025-06-15T12:00:00Z";
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{valid_from: x}, x <= $ts")
        .param("ts", DataValue::from(ts))
        .build();

    assert!(
        script.contains("$ts"),
        "script should reference $ts parameter"
    );
    assert!(
        !script.contains(ts),
        "timestamp string must not appear in script body"
    );
    assert_eq!(
        params.get("ts").expect("ts param must exist"),
        &DataValue::from(ts),
        "ts param should have the correct value"
    );
}

#[test]
fn query_builder_multiple_conditions() {
    use FactsField::*;
    let script = QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Content, Confidence])
        .bind(Id)
        .bind(Content)
        .bind(Confidence)
        .bind(NousId)
        .bind(IsForgotten)
        .filter("nous_id = $nous_id")
        .filter("confidence >= 0.5")
        .filter("is_forgotten == false")
        .done()
        .build_script();

    assert!(
        script.contains("nous_id = $nous_id"),
        "nous_id filter should be present"
    );
    assert!(
        script.contains("confidence >= 0.5"),
        "confidence filter should be present"
    );
    assert!(
        script.contains("is_forgotten == false"),
        "is_forgotten filter should be present"
    );
    let comma_newline_count = script.matches(",\n").count();
    assert!(
        comma_newline_count >= 3,
        "expected at least 3 comma-separated clauses, got {comma_newline_count}"
    );
}

#[test]
fn query_builder_special_chars_escaped() {
    let dangerous = r#"value with "quotes" and \backslash\ and 'apostrophes'"#;
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{content: x, id: $input}")
        .param("input", DataValue::from(dangerous))
        .build();

    assert!(
        !script.contains(dangerous),
        "special characters must not appear in script body"
    );
    assert!(
        script.contains("$input"),
        "script should reference $input parameter"
    );
    assert_eq!(
        params.get("input").expect("input param must exist"),
        &DataValue::from(dangerous),
        "input param should have the correct value"
    );
}

#[test]
fn put_builder_produces_valid_datalog() {
    use EntitiesField::*;
    let script = QueryBuilder::new()
        .put(Relation::Entities)
        .keys(&[Id])
        .values(&[Name, EntityType])
        .done()
        .build_script();

    assert!(
        script.contains("?[id, name, entity_type]"),
        "field list present"
    );
    assert!(
        script.contains("[$id, $name, $entity_type]"),
        "auto-generated param row present"
    );
    assert!(
        script.contains(":put entities {id => name, entity_type}"),
        "put clause with key => value separation"
    );
}

#[test]
fn scan_builder_produces_valid_datalog() {
    use RelationshipsField::*;
    let script = QueryBuilder::new()
        .scan(super::Relation::Relationships)
        .select(&[Src, Dst, Relation])
        .bind(Src)
        .bind(Dst)
        .bind(Relation)
        .bind(Weight)
        .filter("weight > 0.5")
        .order("-weight")
        .limit("10")
        .done()
        .build_script();

    assert!(script.contains("?[src, dst, relation]"), "select clause");
    assert!(
        script.contains("*relationships{src, dst, relation, weight}"),
        "scan clause"
    );
    assert!(script.contains("weight > 0.5"), "filter present");
    assert!(script.contains(":order -weight"), "order directive");
    assert!(script.contains(":limit 10"), "limit directive");
}

#[test]
fn query_builder_limit_applied() {
    use FactsField::*;
    let script = QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Content])
        .bind(Id)
        .bind(Content)
        .limit("$limit")
        .done()
        .build_script();

    assert!(
        script.contains(":limit $limit"),
        "limit directive must appear in output"
    );
}

#[test]
fn query_builder_order_by() {
    use FactsField::*;
    let script = QueryBuilder::new()
        .scan(Relation::Facts)
        .select(&[Id, Confidence])
        .bind(Id)
        .bind(Confidence)
        .order("-confidence")
        .done()
        .build_script();

    assert!(
        script.contains(":order -confidence"),
        "order directive must appear in output"
    );
}

#[test]
fn upsert_fact_builder() {
    let script = queries::upsert_fact();
    assert!(
        script.contains(":put facts"),
        "must contain :put facts directive"
    );
    assert!(
        script.contains("id, valid_from =>"),
        "must have key => value separation"
    );
    assert!(script.contains("$id"), "must reference $id parameter");
    assert!(
        script.contains("$content"),
        "must reference $content parameter"
    );
    assert!(
        script.contains("$confidence"),
        "must reference $confidence parameter"
    );
}

#[test]
fn query_builder_injection_semicolon() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{id: $user_input, content: x}")
        .param("user_input", DataValue::from("test; :rm facts"))
        .build();

    assert!(
        !script.contains("; :rm facts"),
        "semicolon injection must not appear in script"
    );
    assert!(
        script.contains("$user_input"),
        "script should reference $user_input parameter"
    );
    assert!(
        params.contains_key("user_input"),
        "param map should contain user_input"
    );
}

#[test]
fn query_builder_injection_colon_put() {
    let (script, params) = QueryBuilder::new()
        .raw("?[x] := *facts{id: $user_input, content: x}")
        .param("user_input", DataValue::from(":put evil {x => y}"))
        .build();

    assert!(
        !script.contains(":put evil"),
        ":put injection must not appear in script"
    );
    assert!(
        script.contains("$user_input"),
        "script should reference $user_input parameter"
    );
    assert_eq!(
        params
            .get("user_input")
            .expect("user_input param must exist"),
        &DataValue::from(":put evil {x => y}"),
        "user_input param should have the correct value"
    );
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn query_builder_never_produces_raw_user_input(
            // Use a suffix that is unique and cannot appear in any Datalog template.
            // Proptest shrinks strings by truncation, so we anchor the uniqueness
            // at the END of the string to survive shrinking.
            input in "[a-zA-Z0-9]{10,50}XEND"
        ) {
            let (script, params) = QueryBuilder::new()
                .raw("?[x] := *facts{id: $user_input, content: x}")
                .param("user_input", DataValue::from(input.as_str()))
                .build();

            prop_assert!(
                !script.contains(&input),
                "raw user input must not appear in script: {input}"
            );
            prop_assert!(params.contains_key("user_input"));
        }
    }
}
