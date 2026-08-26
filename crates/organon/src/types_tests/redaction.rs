//! `RedactionPolicy` enforcement tests (#6808): the declared policy is
//! applied to trace-surface copies of tool inputs/results, covering both
//! directions (redact and pass-through), the misspelled-declaration case,
//! and the non-object fail-closed case.

use super::super::*;

fn schema_with(properties: &[&str]) -> InputSchema {
    InputSchema {
        properties: properties
            .iter()
            .map(|name| (name.to_string(), PropertyDef::default()))
            .collect(),
        required: vec![],
    }
}

#[test]
fn none_passes_input_and_result_through() {
    let policy = RedactionPolicy::None;
    let mut input = serde_json::json!({"url": "https://acme.corp", "headers": {"x": "y"}});
    let before = input.clone();
    let missed = policy.apply_to_input(&mut input);
    assert!(missed.is_empty(), "None reports no misses");
    assert_eq!(input, before, "None leaves the input untouched");

    let mut result = "response body".to_owned();
    policy.apply_to_result(&mut result);
    assert_eq!(result, "response body", "None leaves the result untouched");
}

#[test]
fn full_replaces_input_with_fixed_payload_independent_of_shape() {
    let policy = RedactionPolicy::Full;
    let mut input = serde_json::json!({
        "url": "https://acme.corp",
        "headers": {"authorization": "Bearer token"},
        "retries": 3,
        "tags": ["a", "b"],
    });
    let missed = policy.apply_to_input(&mut input);
    assert!(missed.is_empty(), "Full reports no misses");
    assert_eq!(
        input,
        serde_json::json!({"__redaction__": REDACTED_MARKER}),
        "Full must preserve no input-derived keys, shape, or array length"
    );
}

#[test]
fn full_redacts_result_text() {
    let policy = RedactionPolicy::Full;
    let mut result = "body containing an access token".to_owned();
    policy.apply_to_result(&mut result);
    assert_eq!(result, REDACTED_MARKER);
}

#[test]
fn fields_redacts_named_fields_and_passes_others() {
    let policy = RedactionPolicy::Fields(vec!["headers".to_owned()]);
    let mut input = serde_json::json!({
        "url": "https://acme.corp",
        "headers": {"PRIVATE-TOKEN": "tok"},
    });
    let missed = policy.apply_to_input(&mut input);
    assert!(missed.is_empty(), "all declared fields matched");
    assert_eq!(
        input["headers"],
        serde_json::json!(REDACTED_MARKER),
        "declared field is redacted"
    );
    assert_eq!(
        input["url"],
        serde_json::json!("https://acme.corp"),
        "undeclared field passes through unchanged"
    );

    let mut result = "response body".to_owned();
    policy.apply_to_result(&mut result);
    assert_eq!(
        result, "response body",
        "Fields is argument-scoped: results pass through"
    );
}

#[test]
fn fields_absent_optional_field_passes_payload_and_reports_miss() {
    let policy = RedactionPolicy::Fields(vec!["headers".to_owned()]);
    let mut input = serde_json::json!({"url": "https://acme.corp"});
    let before = input.clone();
    let missed = policy.apply_to_input(&mut input);
    assert_eq!(
        missed,
        vec!["headers".to_owned()],
        "the absent declared field is reported, not silent"
    );
    assert_eq!(
        input, before,
        "an absent optional field changes nothing else in the payload"
    );
}

#[test]
fn fields_misspelled_declaration_is_flagged_against_schema() {
    let schema = schema_with(&["headers", "url"]);
    let typo = RedactionPolicy::Fields(vec!["heders".to_owned()]);
    assert_eq!(
        typo.unrecognized_fields(&schema),
        vec!["heders".to_owned()],
        "a misspelled declared field names itself against the schema"
    );
    let correct = RedactionPolicy::Fields(vec!["headers".to_owned()]);
    assert!(
        correct.unrecognized_fields(&schema).is_empty(),
        "a declared field present in the schema is recognized"
    );
    assert!(
        RedactionPolicy::None
            .unrecognized_fields(&schema)
            .is_empty(),
        "None declares no fields to misspell"
    );
    assert!(
        RedactionPolicy::Full
            .unrecognized_fields(&schema)
            .is_empty(),
        "Full declares no fields to misspell"
    );
}

#[test]
fn malformed_fields_declarations_are_invalid_for_schema() {
    let schema = schema_with(&["headers", "url"]);
    assert!(RedactionPolicy::Fields(vec!["headers".to_owned()]).is_valid_for_schema(&schema));
    assert!(!RedactionPolicy::Fields(vec![]).is_valid_for_schema(&schema));
    assert!(
        !RedactionPolicy::Fields(vec!["headers".to_owned(), "headers".to_owned()])
            .is_valid_for_schema(&schema)
    );
    assert!(
        !RedactionPolicy::Fields(vec!["headers".to_owned(), "missing".to_owned()])
            .is_valid_for_schema(&schema)
    );
}

#[test]
fn fields_on_non_object_payload_fails_closed() {
    let policy = RedactionPolicy::Fields(vec!["headers".to_owned()]);
    let mut input = serde_json::json!("not an object");
    let missed = policy.apply_to_input(&mut input);
    assert_eq!(
        missed,
        vec!["headers".to_owned()],
        "every declared field missed on a non-object payload"
    );
    let text = input.to_string();
    assert_eq!(input, serde_json::json!({"__redaction__": REDACTED_MARKER}));
    assert!(
        !text.contains("not an object"),
        "the unverifiable payload value must not survive: {text}"
    );
}

#[test]
fn vault_placeholders_follow_the_declared_policy() {
    // Precedence pin: dispatch persists the `{{secret:...}}` placeholder
    // form (the resolved value never leaves the vault); the policy then
    // applies to that placeholder like any other value.
    let placeholder_input = || serde_json::json!({"auth": "{{secret:token}}", "path": "/tmp/x"});

    let mut none_input = placeholder_input();
    RedactionPolicy::None.apply_to_input(&mut none_input);
    assert_eq!(
        none_input["auth"],
        serde_json::json!("{{secret:token}}"),
        "None passes the placeholder form through by design"
    );

    let mut fields_input = placeholder_input();
    RedactionPolicy::Fields(vec!["auth".to_owned()]).apply_to_input(&mut fields_input);
    assert_eq!(
        fields_input["auth"],
        serde_json::json!(REDACTED_MARKER),
        "Fields hides even the vault key name on declared fields"
    );

    let mut full_input = placeholder_input();
    RedactionPolicy::Full.apply_to_input(&mut full_input);
    assert_eq!(
        full_input,
        serde_json::json!({"__redaction__": REDACTED_MARKER}),
        "Full hides the placeholder and its input-derived field name"
    );
}
