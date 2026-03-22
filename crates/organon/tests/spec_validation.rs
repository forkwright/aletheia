//! Component spec validation tests.
//!
//! Validates two concrete [`ToolExecutor`] implementations against the
//! [`ToolExecutorSpec`] contract: the echo-style mock and the error mock.
//!
//! Enabled by `--features test-support`.
//!
//! Run: `cargo test -p aletheia-organon --features test-support --test spec_validation`

#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use aletheia_koina::id::ToolName;
use aletheia_organon::testing::{MockToolExecutor, ToolExecutorSpec, make_test_context};
use aletheia_organon::types::{InputSchema, PropertyDef, PropertyType, ToolCategory, ToolDef};
use indexmap::IndexMap;

fn echo_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("mock").expect("valid name"),
        description: "echo: returns whatever text was injected".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category: ToolCategory::System,
        auto_activate: false,
    }
}

fn note_def() -> ToolDef {
    let mut props = IndexMap::new();
    props.insert(
        "text".to_owned(),
        PropertyDef {
            property_type: PropertyType::String,
            description: "note content".to_owned(),
            enum_values: None,
            default: None,
        },
    );
    ToolDef {
        name: ToolName::new("mock").expect("valid name"),
        description: "note: stores a note string".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: props,
            required: vec!["text".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

/// Validates the echo mock executor against the full spec contract.
#[tokio::test]
async fn spec_echo_executor_passes() {
    let executor = MockToolExecutor::text("pong");
    let ctx = make_test_context();
    let spec = ToolExecutorSpec::new(ToolName::new("mock").expect("valid name"));
    let _ = echo_def(); // registered separately; spec validates behaviour

    let report = spec.validate_async(&executor, &ctx).await;
    assert!(
        report.is_passing(),
        "echo executor failed spec:\n  failures: {:?}",
        report.failures()
    );
    assert!(
        !report.passes().is_empty(),
        "spec must run at least one check"
    );
}

/// Validates the note-storage mock executor against the full spec contract.
///
/// Uses a sequence mock to exercise the reusability check distinctly.
#[tokio::test]
async fn spec_note_executor_passes() {
    use aletheia_organon::types::ToolResult;

    let executor = MockToolExecutor::sequence(vec![
        ToolResult::text("note stored"),
        ToolResult::text("note stored again"),
        ToolResult::text("note stored again"),
    ]);
    let ctx = make_test_context();
    let spec = ToolExecutorSpec::new(ToolName::new("mock").expect("valid name"));
    let _ = note_def();

    let report = spec.validate_async(&executor, &ctx).await;
    assert!(
        report.is_passing(),
        "note executor failed spec:\n  failures: {:?}",
        report.failures()
    );
}

/// Validates that a ToolExecutorSpec correctly flags an error-only executor.
///
/// An executor that always returns `ToolResult::error(...)` must be flagged
/// on the `success-result-not-marked-error` check.
#[tokio::test]
async fn spec_detects_always_error_executor() {
    let executor = MockToolExecutor::tool_error("something broke");
    let ctx = make_test_context();
    let spec = ToolExecutorSpec::new(ToolName::new("mock").expect("valid name"));

    let report = spec.validate_async(&executor, &ctx).await;
    // The executor returns Ok(ToolResult { is_error: true }) — the spec must flag this.
    let failure_names: Vec<&str> = report
        .failures()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        failure_names.contains(&"success-result-not-marked-error"),
        "spec must detect an always-error executor; failures: {:?}",
        report.failures()
    );
}

/// Validates that call_count tracking works across spec validation runs.
#[tokio::test]
async fn spec_validation_drives_multiple_calls() {
    let executor = MockToolExecutor::text("hi");
    let ctx = make_test_context();
    let spec = ToolExecutorSpec::new(ToolName::new("mock").expect("valid name"));

    spec.validate_async(&executor, &ctx).await;

    assert!(
        executor.call_count() > 1,
        "spec validation must invoke the executor more than once (got {})",
        executor.call_count()
    );
}
