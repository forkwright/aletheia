//! Query builder field type and injection tests.
use super::*;

#[test]
fn query_builder_empty() {
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
            prop_assert!(params.contains_key("user_input"), "param map should contain user_input binding");
        }
    }
}
