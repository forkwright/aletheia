//! Tests for the prompt audit log.

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

use std::fs;

use jiff::civil::date;
use jiff::{Timestamp, ToSpan};

use super::*;

fn make_record(ts: Timestamp, id: &str) -> PromptAuditRecord {
    PromptAuditRecord {
        timestamp: ts,
        nous_id: "syn".to_owned(),
        session_id: "ses-1".to_owned(),
        turn_id: id.to_owned(),
        provider: "anthropic".to_owned(),
        deployment_target: "cloud".to_owned(),
        model: "claude-opus-4-20250514".to_owned(),
        system_prompt_hash: hash_system_prompt(Some("hello system")),
        system_prompt_bytes: "hello system".len(),
        message_count: 2,
        token_count_estimate: 42,
        fact_ids_included: vec!["fact:1".to_owned(), "fact:2".to_owned()],
        fact_ids_filtered: vec![],
        tool_names: vec!["read".to_owned(), "write".to_owned()],
        tool_surface_hash: "ts1:test".to_owned(),
        request_id: Some("req-abc".to_owned()),
    }
}

#[test]
fn hash_system_prompt_is_sha256_hex() {
    // WHY: canonical SHA-256 of "hello" (ASCII, no trailing newline) keeps the
    // hash format contract tied to a stable reference. If this ever changes,
    // operators need to know — existing audit logs would become unreadable.
    let h = hash_system_prompt(Some("hello"));
    assert_eq!(
        h,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn hash_system_prompt_none_is_empty() {
    assert_eq!(hash_system_prompt(None), "");
}

#[test]
fn record_serialization_roundtrip() {
    let ts = "2026-04-16T12:00:00Z".parse::<Timestamp>().unwrap();
    let record = make_record(ts, "turn-1");
    let json = serde_json::to_string(&record).expect("serialize");
    let decoded: PromptAuditRecord = serde_json::from_str(&json).expect("decode");
    assert_eq!(decoded.nous_id, "syn");
    assert_eq!(decoded.fact_ids_included.len(), 2);
    assert_eq!(decoded.tool_names, vec!["read", "write"]);
    assert_eq!(decoded.tool_surface_hash, "ts1:test");
    assert_eq!(decoded.request_id.as_deref(), Some("req-abc"));
    assert_eq!(decoded.system_prompt_bytes, 12);
}

// WHY(#4853): these three tests used to exercise `build_audit_record`, a
// `#[cfg(test)]`-only helper that duplicated the recall-fact-filtering logic
// of the real production path, `build_audit_record_for_request`, but derived
// its tool surface/provider from a live `organon::types::ToolContext` +
// `ToolRegistry` + `ProviderRegistry` -- machinery no production caller of
// the helper ever exercised (`build_audit_record`'s only callers were these
// tests). It also carried a stale `TODO(#4853): thread request_id from
// pylon middleware` even though `SessionState::request_id` already existed
// and the real production path already read it. The helper and its
// `PromptAuditRecordInput` type are deleted, not threaded, per the ratified
// decision on #4853; these tests are rewritten to call
// `build_audit_record_for_request` directly so the recall-filtering
// coverage lands on the function actually running in production instead of
// on its retired stand-in.
#[test]
fn build_audit_record_for_request_includes_filtered_recall_facts() {
    let config = crate::config::NousConfig::default();
    let session = crate::session::SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);
    let ctx = crate::pipeline::PipelineContext {
        recall_result: Some(crate::recall::RecallStageResult {
            candidates_found: 2,
            results_injected: 1,
            tokens_consumed: 4,
            recall_section: Some("## Recalled Knowledge\n- public".to_owned()),
            fact_ids: vec!["fact-public".to_owned()],
            deployment_target: hermeneus::provider::DeploymentTarget::Cloud,
            filtered_facts: vec![crate::recall::RecallFilteredFact {
                id: "fact-internal".to_owned(),
                sensitivity: mneme::knowledge::FactSensitivity::Internal,
            }],
            selected_context: Vec::new(),
            excluded_context: Vec::new(),
        }),
        ..crate::pipeline::PipelineContext::default()
    };
    let request = hermeneus::types::CompletionRequest::default();

    let record = build_audit_record_for_request(PromptAuditRequestRecordInput {
        ctx: &ctx,
        session: &session,
        model: &config.generation.model,
        request: &request,
        provider: "anthropic",
        deployment_target: "cloud",
        surface_hash: "ts1:test",
        options: PromptAuditRecordOptions::default(),
        chars_per_token: 4,
        request_id: None,
    });

    assert_eq!(record.deployment_target, "cloud");
    assert_eq!(record.tool_surface_hash, "ts1:test");
    assert_eq!(record.fact_ids_included, vec!["fact-public"]);
    let filtered = record
        .fact_ids_filtered
        .first()
        .expect("one filtered fact should be present");
    assert_eq!(filtered.id, "fact-internal");
    assert_eq!(filtered.sensitivity, "internal");
}

#[test]
fn build_audit_record_for_request_omits_filtered_recall_facts_when_disabled() {
    let config = crate::config::NousConfig::default();
    let session = crate::session::SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);
    let ctx = crate::pipeline::PipelineContext {
        recall_result: Some(crate::recall::RecallStageResult {
            candidates_found: 2,
            results_injected: 1,
            tokens_consumed: 4,
            recall_section: Some("## Recalled Knowledge\n- public".to_owned()),
            fact_ids: vec!["fact-public".to_owned()],
            deployment_target: hermeneus::provider::DeploymentTarget::Cloud,
            filtered_facts: vec![crate::recall::RecallFilteredFact {
                id: "fact-internal".to_owned(),
                sensitivity: mneme::knowledge::FactSensitivity::Internal,
            }],
            selected_context: Vec::new(),
            excluded_context: Vec::new(),
        }),
        ..crate::pipeline::PipelineContext::default()
    };
    let request = hermeneus::types::CompletionRequest::default();

    let record = build_audit_record_for_request(PromptAuditRequestRecordInput {
        ctx: &ctx,
        session: &session,
        model: &config.generation.model,
        request: &request,
        provider: "anthropic",
        deployment_target: "cloud",
        surface_hash: "ts1:test",
        options: PromptAuditRecordOptions {
            include_filtered_ids: false,
        },
        chars_per_token: 4,
        request_id: None,
    });

    assert_eq!(record.fact_ids_included, vec!["fact-public"]);
    assert!(record.fact_ids_filtered.is_empty());
}

#[test]
fn disabled_log_is_noop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = PromptAuditLog::new(dir.path().to_path_buf(), false);
    let ts = "2026-04-16T12:00:00Z".parse::<Timestamp>().unwrap();
    log.log_request(&make_record(ts, "t1")).expect("noop ok");
    // WHY: disabled log must not create any files, so operators can turn the
    // feature off without leaving empty directories behind.
    let entries = fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(entries, 0, "disabled log should not create files");
}

#[test]
fn log_appends_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = PromptAuditLog::new(dir.path().join("prompt-audit"), true);
    let ts = "2026-04-16T12:00:00Z".parse::<Timestamp>().unwrap();
    log.log_request(&make_record(ts, "t1")).expect("write");
    log.log_request(&make_record(ts, "t2")).expect("write");

    let path = dir.path().join("prompt-audit").join("2026-04-16.jsonl");
    let content = fs::read_to_string(&path).expect("read");
    let lines: Vec<_> = content.lines().collect();
    assert_eq!(lines.len(), 2, "two records written as JSONL");
    for line in &lines {
        let _: PromptAuditRecord = serde_json::from_str(line).expect("valid JSONL");
    }
}

#[test]
fn daily_rotation_boundary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = PromptAuditLog::new(dir.path().to_path_buf(), true);

    // 23:59 on 2026-04-16 → 2026-04-16.jsonl
    let t_late = "2026-04-16T23:59:00Z".parse::<Timestamp>().unwrap();
    // 00:01 on 2026-04-17 → 2026-04-17.jsonl
    let t_next = "2026-04-17T00:01:00Z".parse::<Timestamp>().unwrap();

    log.log_request(&make_record(t_late, "late")).unwrap();
    log.log_request(&make_record(t_next, "next")).unwrap();

    let d1 = dir.path().join("2026-04-16.jsonl");
    let d2 = dir.path().join("2026-04-17.jsonl");
    assert!(d1.exists(), "late record goes in day N file");
    assert!(d2.exists(), "next record goes in day N+1 file");

    let c1 = fs::read_to_string(&d1).unwrap();
    let c2 = fs::read_to_string(&d2).unwrap();
    assert_eq!(c1.lines().count(), 1);
    assert_eq!(c2.lines().count(), 1);
    assert!(c1.contains("\"turn_id\":\"late\""));
    assert!(c2.contains("\"turn_id\":\"next\""));
}

#[test]
fn log_does_not_contain_system_prompt_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = PromptAuditLog::new(dir.path().to_path_buf(), true);
    let ts = "2026-04-16T12:00:00Z".parse::<Timestamp>().unwrap();

    let sentinel = "SECRET_SOUL_MD_CONTENT_DO_NOT_LEAK";
    let mut record = make_record(ts, "t1");
    record.system_prompt_hash = hash_system_prompt(Some(sentinel));
    record.system_prompt_bytes = sentinel.len();
    log.log_request(&record).expect("write");

    let path = dir.path().join("2026-04-16.jsonl");
    let content = fs::read_to_string(&path).unwrap();
    // WHY: the sovereignty contract is that system prompt content is hashed,
    // not logged. If the sentinel ever appears in the log, the contract is
    // broken.
    assert!(
        !content.contains(sentinel),
        "log must not contain system prompt content"
    );
    assert!(
        content.contains(&hash_system_prompt(Some(sentinel))),
        "log must contain the hash"
    );
}

#[test]
fn rotation_after_seven_days() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = PromptAuditLog::new(dir.path().to_path_buf(), true);

    let base = date(2026, 4, 10).at(12, 0, 0, 0).in_tz("UTC").unwrap();
    for i in 0..7 {
        let ts = base.checked_add(i.days()).unwrap().timestamp();
        log.log_request(&make_record(ts, &format!("turn-{i}")))
            .unwrap();
    }

    let files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            std::path::Path::new(n)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
        })
        .collect();
    assert_eq!(files.len(), 7, "one file per day");
}
