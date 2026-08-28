//! Table tests for the shared argument extractors and codec helper.
//!
//! WHY one table rather than a case per helper: every extractor here resolves
//! through the same two steps -- `get(field)` then a typed accessor -- so
//! "missing" and "wrong type" are one behaviour with several error adapters
//! over it. Asserting it once per helper restates the same fact N times and
//! lets the copies drift, which is the defect this consolidation removed.
#![expect(clippy::expect_used, reason = "test assertions")]

use koina::id::ToolName;

use super::super::*;

/// Every shape a required-string field can take that is not a string. A helper
/// that accepts any of these has stopped validating.
fn non_string_args() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("field absent", serde_json::json!({ "other": "value" })),
        ("number", serde_json::json!({ "path": 42 })),
        ("bool", serde_json::json!({ "path": true })),
        ("array", serde_json::json!({ "path": ["a"] })),
        ("object", serde_json::json!({ "path": { "a": 1 } })),
        ("null", serde_json::json!({ "path": null })),
    ]
}

#[test]
fn required_string_extractors_reject_every_non_string_shape() {
    let name = ToolName::new("test").expect("valid");
    for (case, args) in non_string_args() {
        let typed = extract_str(&args, "path", &name);
        assert!(typed.is_err(), "extract_str must reject {case}");
        assert!(
            typed
                .expect_err("checked is_err above")
                .to_string()
                .contains("missing or invalid field: path"),
            "extract_str must name the field for {case}"
        );

        assert!(
            extract_str_or_tool_error(&args, "path").is_err(),
            "extract_str_or_tool_error must reject {case}"
        );

        let adapted: std::result::Result<&str, &'static str> =
            extract_str_with(&args, "path", |_| "adapter ran");
        assert_eq!(
            adapted,
            Err("adapter ran"),
            "extract_str_with must invoke the caller's adapter for {case}"
        );
    }
}

#[test]
fn required_string_extractors_accept_a_string_and_agree_on_the_value() {
    let name = ToolName::new("test").expect("valid");
    let args = serde_json::json!({ "path": "/tmp/x" });

    assert_eq!(
        extract_str(&args, "path", &name).expect("string is accepted"),
        "/tmp/x"
    );
    assert_eq!(
        extract_str_or_tool_error(&args, "path").expect("string is accepted"),
        "/tmp/x"
    );
    let adapted: std::result::Result<&str, &'static str> =
        extract_str_with(&args, "path", |_| "adapter ran");
    assert_eq!(
        adapted,
        Ok("/tmp/x"),
        "extract_str_with must not invoke the adapter on success"
    );
}

#[test]
fn optional_extractors_return_none_for_both_absent_and_wrong_type() {
    let absent = serde_json::json!({});

    assert_eq!(extract_opt_str(&absent, "f"), None, "opt_str, absent");
    assert_eq!(extract_opt_u64(&absent, "f"), None, "opt_u64, absent");
    assert_eq!(extract_opt_bool(&absent, "f"), None, "opt_bool, absent");
    assert_eq!(extract_opt_f64(&absent, "f"), None, "opt_f64, absent");

    // A value of the wrong type must read as absent, not as a coerced value.
    assert_eq!(
        extract_opt_str(&serde_json::json!({ "f": 42 }), "f"),
        None,
        "opt_str must not coerce a number"
    );
    assert_eq!(
        extract_opt_u64(&serde_json::json!({ "f": "42" }), "f"),
        None,
        "opt_u64 must not parse a string"
    );
    assert_eq!(
        extract_opt_u64(&serde_json::json!({ "f": -1 }), "f"),
        None,
        "opt_u64 must reject a negative number"
    );
    assert_eq!(
        extract_opt_bool(&serde_json::json!({ "f": "true" }), "f"),
        None,
        "opt_bool must not parse a string"
    );
    assert_eq!(
        extract_opt_f64(&serde_json::json!({ "f": "1.5" }), "f"),
        None,
        "opt_f64 must not parse a string"
    );
}

#[test]
fn base64_decode_round_trips_and_stringifies_its_error() {
    let encoded = koina::base64::encode(b"hello");
    assert_eq!(
        base64_decode(&encoded).expect("round-trip decodes"),
        b"hello".to_vec()
    );

    let err = base64_decode("!!! not base64 !!!").expect_err("invalid input must fail");
    assert!(
        !err.is_empty(),
        "the error must stringify to something displayable to the tool caller"
    );
}
