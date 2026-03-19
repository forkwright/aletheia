//! Query builder datalog matching tests.
use super::*;

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
