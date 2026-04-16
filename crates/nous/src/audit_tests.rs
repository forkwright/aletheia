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
    assert_eq!(decoded.request_id.as_deref(), Some("req-abc"));
    assert_eq!(decoded.system_prompt_bytes, 12);
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
