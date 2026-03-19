//! Integration tests for triage tools.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use aletheia_koina::id::{NousId, SessionId, ToolName};

use super::*;
use crate::types::{ServerToolConfig, ToolContext, ToolInput, ToolServices};

fn test_ctx() -> ToolContext {
    let _ = rustls::crypto::ring::default_provider().install_default();
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: std::path::PathBuf::from("/tmp/test"),
        allowed_roots: vec![std::path::PathBuf::from("/tmp")],
        services: Some(Arc::new(ToolServices {
            cross_nous: None,
            messenger: None,
            note_store: None,
            blackboard_store: None,
            spawn: None,
            planning: None,
            knowledge: None,
            http_client: reqwest::Client::new(),
            lazy_tool_catalog: vec![],
            server_tool_config: ServerToolConfig::default(),
        })),
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

#[test]
fn tool_definitions_are_valid() {
    let scan = issue_scan_def();
    assert_eq!(scan.name.as_str(), "issue_scan");
    assert_eq!(scan.category, ToolCategory::Planning);
    assert!(!scan.auto_activate, "triage tools should be lazy-loaded");

    let triage = issue_triage_def();
    assert_eq!(triage.name.as_str(), "issue_triage");
    assert!(
        triage.input_schema.required.contains(&"repo".to_owned()),
        "repo must be required"
    );
    assert!(
        triage
            .input_schema
            .required
            .contains(&"staging_dir".to_owned()),
        "staging_dir must be required"
    );

    let approve = issue_approve_def();
    assert_eq!(approve.name.as_str(), "issue_approve");
    assert_eq!(approve.input_schema.required.len(), 3);
}

#[test]
fn registration_succeeds() {
    let mut registry = crate::registry::ToolRegistry::new();
    register(&mut registry).expect("registration should succeed");

    let scan_name = ToolName::new("issue_scan").expect("valid");
    let triage_name = ToolName::new("issue_triage").expect("valid");
    let approve_name = ToolName::new("issue_approve").expect("valid");

    assert!(registry.get_def(&scan_name).is_some(), "issue_scan missing");
    assert!(
        registry.get_def(&triage_name).is_some(),
        "issue_triage missing"
    );
    assert!(
        registry.get_def(&approve_name).is_some(),
        "issue_approve missing"
    );
}

#[test]
fn no_duplicate_registration() {
    let mut registry = crate::registry::ToolRegistry::new();
    register(&mut registry).expect("first registration");
    let err = register(&mut registry).expect_err("duplicate should fail");
    assert!(
        err.to_string().contains("duplicate"),
        "error should mention duplicate: {err}"
    );
}

#[test]
fn slugify_basic() {
    assert_eq!(slugify("Hello World"), "hello-world");
    assert_eq!(slugify("fix: memory leak!"), "fix-memory-leak");
    assert_eq!(slugify("---leading---"), "leading");
}

#[test]
fn slugify_truncates_long_titles() {
    let long = "a".repeat(100);
    let slug = slugify(&long);
    assert!(
        slug.len() <= 50,
        "slug should be at most 50 chars: {}",
        slug.len()
    );
}

#[test]
fn format_issue_summary_empty() {
    let summary = format_issue_summary(&[]);
    assert!(summary.contains("No open issues"));
}

#[test]
fn format_issue_summary_with_issues() {
    let issues = vec![GitHubIssue {
        number: 123,
        title: "Test issue".to_owned(),
        body: "body".to_owned(),
        labels: vec!["bug".to_owned()],
        milestone: Some("v1.0".to_owned()),
        author: "alice".to_owned(),
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        priority_label: Some("priority/high".to_owned()),
    }];
    let summary = format_issue_summary(&issues);
    assert!(summary.contains("#123"), "should contain issue number");
    assert!(summary.contains("Test issue"), "should contain title");
    assert!(summary.contains("bug"), "should contain label");
    assert!(summary.contains("v1.0"), "should contain milestone");
    assert!(summary.contains("priority/high"), "should contain priority");
}

#[tokio::test]
async fn approve_rejects_missing_prompt() {
    let ctx = test_ctx();
    let staging = tempfile::tempdir().expect("tempdir");
    let queue = tempfile::tempdir().expect("tempdir");

    let executor = IssueApproveExecutor;
    let input = ToolInput {
        name: ToolName::new("issue_approve").expect("valid"),
        tool_use_id: "toolu_1".to_owned(),
        arguments: serde_json::json!({
            "staging_dir": staging.path().display().to_string(),
            "queue_dir": queue.path().display().to_string(),
            "prompt_id": "nonexistent"
        }),
    };

    let result = executor.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "should error for missing prompt");
    assert!(
        result.content.text_summary().contains("no staged prompt"),
        "error should mention missing: {}",
        result.content.text_summary()
    );
}

#[tokio::test]
async fn approve_moves_staged_prompt_to_queue() {
    let ctx = test_ctx();
    let staging = tempfile::tempdir().expect("tempdir");
    let queue = tempfile::tempdir().expect("tempdir");

    // Create a staged prompt
    let prompt_path = staging.path().join("42-test-issue.md");
    tokio::fs::write(&prompt_path, "# 42: Test issue\n")
        .await
        .expect("write staged prompt");

    let executor = IssueApproveExecutor;
    let input = ToolInput {
        name: ToolName::new("issue_approve").expect("valid"),
        tool_use_id: "toolu_2".to_owned(),
        arguments: serde_json::json!({
            "staging_dir": staging.path().display().to_string(),
            "queue_dir": queue.path().display().to_string(),
            "prompt_id": "42"
        }),
    };

    let result = executor.execute(&input, &ctx).await.expect("execute");
    assert!(!result.is_error, "approval should succeed");

    let summary = result.content.text_summary();
    assert!(
        summary.contains("Approved"),
        "should confirm approval: {summary}"
    );
    assert!(
        summary.contains("test-agent"),
        "should log approver: {summary}"
    );

    // Verify file moved
    assert!(!prompt_path.exists(), "staged file should be removed");
    let queue_path = queue.path().join("42-test-issue.md");
    assert!(queue_path.exists(), "file should exist in queue");
}

#[tokio::test]
async fn scan_requires_services() {
    let ctx = ToolContext {
        nous_id: NousId::new("test").expect("valid"),
        session_id: SessionId::new(),
        workspace: std::path::PathBuf::from("/tmp"),
        allowed_roots: vec![],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    };

    let executor = IssueScanExecutor;
    let input = ToolInput {
        name: ToolName::new("issue_scan").expect("valid"),
        tool_use_id: "toolu_3".to_owned(),
        arguments: serde_json::json!({"repo": "forkwright/aletheia"}),
    };

    let result = executor.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "should error without services");
    assert!(
        result.content.text_summary().contains("not configured"),
        "should say services not configured"
    );
}
