//! Proskenion live-server contract tests.
//!
//! These tests exercise the HTTP/SSE protocol surface consumed by the desktop
//! client without driving the GTK/WebKit UI.

#![expect(
    clippy::expect_used,
    reason = "contract tests fail fast on setup errors"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "JSON contract assertions intentionally show exact response bodies"
)]

use axum::http::StatusCode;
use hermeneus::test_utils::MockProvider;
use http_body_util::BodyExt;
use integration_tests::harness::{TEST_NOUS_ID, TestHarness, body_json, body_string};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tower::ServiceExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContractCoverage {
    Covered,
    OutOfContract { reason: &'static str },
}

#[derive(Debug, Clone, Copy)]
struct DesktopApiContract {
    method: &'static str,
    source_path: &'static str,
    pylon_route: &'static str,
    expected_shape: &'static str,
    coverage: ContractCoverage,
}

const EXPERIMENTAL_PLANNING_REASON: &str = "planning action surfaces are experimental in docs/GOLDEN-PATH.md and do not yet have Pylon routes";
const LEGACY_TOOL_STATS_REASON: &str = "legacy desktop tool-metrics call has no Pylon route; /api/v1/ops/tools is the contracted ops tool surface";
const CREDENTIAL_MUTATION_REASON: &str = "credential mutations have provider/runtime side effects; the desktop contract pins the list envelope and inventories mutation routes";

const PROSKENION_API_INVENTORY: &[DesktopApiContract] = &[
    DesktopApiContract {
        method: "GET",
        source_path: "/api/health",
        pylon_route: "/api/health",
        expected_shape: "liveness object with string status",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/tool-stats",
        pylon_route: "<out-of-contract>",
        expected_shape: "legacy tool metrics response",
        coverage: ContractCoverage::OutOfContract {
            reason: LEGACY_TOOL_STATS_REASON,
        },
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/config",
        pylon_route: "/api/v1/config",
        expected_shape: "redacted config object",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "PUT",
        source_path: "/api/v1/config/feature_flags",
        pylon_route: "/api/v1/config/{section}",
        expected_shape: "config update object with section/config/restart_required",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/events/subscribe",
        pylon_route: "/api/v1/events/subscribe",
        expected_shape: "text/event-stream domain events with topic event name and JSON data",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/journal",
        pylon_route: "/api/v1/journal",
        expected_shape: "journal response with events and data_unavailable arrays",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/knowledge/entities",
        pylon_route: "/api/v1/knowledge/entities",
        expected_shape: "entities array and numeric total",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/knowledge/entities/{}",
        pylon_route: "/api/v1/knowledge/entities/{id}",
        expected_shape: "entity detail object",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "DELETE",
        source_path: "/api/v1/knowledge/entities/{}",
        pylon_route: "/api/v1/knowledge/entities/{id}",
        expected_shape: "204 no content",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/knowledge/entities/{}/flag",
        pylon_route: "/api/v1/knowledge/entities/{id}/flag",
        expected_shape: "204 no content",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/knowledge/entities/{}/memories",
        pylon_route: "/api/v1/knowledge/entities/{id}/memories",
        expected_shape: "entity memory array",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/knowledge/entities/{}/relationships",
        pylon_route: "/api/v1/knowledge/entities/{id}/relationships",
        expected_shape: "relationships array",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/knowledge/entities/merge",
        pylon_route: "/api/v1/knowledge/entities/merge",
        expected_shape: "204 no content",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/knowledge/facts",
        pylon_route: "/api/v1/knowledge/facts",
        expected_shape: "facts array and numeric total",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "PUT",
        source_path: "/api/v1/knowledge/facts/{}/confidence",
        pylon_route: "/api/v1/knowledge/facts/{id}/confidence",
        expected_shape: "status/id/confidence update object",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/knowledge/facts/{}/forget",
        pylon_route: "/api/v1/knowledge/facts/{id}/forget",
        expected_shape: "204 no content",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/knowledge/facts/{}/restore",
        pylon_route: "/api/v1/knowledge/facts/{id}/restore",
        expected_shape: "204 no content",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "PUT",
        source_path: "/api/v1/knowledge/facts/{}/sensitivity",
        pylon_route: "/api/v1/knowledge/facts/{id}/sensitivity",
        expected_shape: "status/id/sensitivity update object",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/knowledge/timeline",
        pylon_route: "/api/v1/knowledge/timeline",
        expected_shape: "events array and numeric total",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/metrics/agents",
        pylon_route: "/api/v1/metrics/agents",
        expected_shape: "agents array and anomalies array",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/metrics/costs",
        pylon_route: "/api/v1/metrics/costs",
        expected_shape: "cost series and aggregate rows",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/metrics/quality",
        pylon_route: "/api/v1/metrics/quality",
        expected_shape: "quality series object",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/metrics/tokens",
        pylon_route: "/api/v1/metrics/tokens",
        expected_shape: "token series and aggregate rows",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/nous",
        pylon_route: "/api/v1/nous",
        expected_shape: "nous array with agent summaries",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/nous/{}",
        pylon_route: "/api/v1/nous/{id}",
        expected_shape: "nous status object",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "PATCH",
        source_path: "/api/v1/nous/{}",
        pylon_route: "/api/v1/nous/{id}",
        expected_shape: "updated nous summary with toggle flags",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/nous/{}/tools",
        pylon_route: "/api/v1/nous/{id}/tools",
        expected_shape: "tools array with tool summaries",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "PATCH",
        source_path: "/api/v1/nous/{}/tools",
        pylon_route: "/api/v1/nous/{id}/tools",
        expected_shape: "updated tools array with toggle flags",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/ops/tools",
        pylon_route: "/api/v1/ops/tools",
        expected_shape: "ops tool catalog/live/totals envelope",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/planning/projects/{}/checkpoints/{}/action",
        pylon_route: "<out-of-contract>",
        expected_shape: "checkpoint action result",
        coverage: ContractCoverage::OutOfContract {
            reason: EXPERIMENTAL_PLANNING_REASON,
        },
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/planning/projects/{}/discussions/{}/answer",
        pylon_route: "<out-of-contract>",
        expected_shape: "discussion answer result",
        coverage: ContractCoverage::OutOfContract {
            reason: EXPERIMENTAL_PLANNING_REASON,
        },
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/planning/projects/{}/discussions/{}/reopen",
        pylon_route: "<out-of-contract>",
        expected_shape: "discussion reopen result",
        coverage: ContractCoverage::OutOfContract {
            reason: EXPERIMENTAL_PLANNING_REASON,
        },
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/planning/projects/{}/proposals/{}",
        pylon_route: "<out-of-contract>",
        expected_shape: "proposal action result",
        coverage: ContractCoverage::OutOfContract {
            reason: EXPERIMENTAL_PLANNING_REASON,
        },
    },
    DesktopApiContract {
        method: "PUT",
        source_path: "/api/v1/planning/projects/{}/requirements/{}",
        pylon_route: "<out-of-contract>",
        expected_shape: "requirement update result",
        coverage: ContractCoverage::OutOfContract {
            reason: EXPERIMENTAL_PLANNING_REASON,
        },
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/planning/projects/{}/verification",
        pylon_route: "/api/v1/planning/projects/{project_id}/verification",
        expected_shape: "verification result or 404 not-available fallback",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/planning/projects/{}/verification/refresh",
        pylon_route: "/api/v1/planning/projects/{project_id}/verification/refresh",
        expected_shape: "verification result or 404 not-available fallback",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/sessions",
        pylon_route: "/api/v1/sessions",
        expected_shape: "paginated session list",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/sessions/stream",
        pylon_route: "/api/v1/sessions/stream",
        expected_shape: "turn SSE stream",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/sessions/{}/archive",
        pylon_route: "/api/v1/sessions/{id}/archive",
        expected_shape: "204 no content",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/sessions/{}/history",
        pylon_route: "/api/v1/sessions/{id}/history",
        expected_shape: "messages array",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/sessions/{}/unarchive",
        pylon_route: "/api/v1/sessions/{id}/unarchive",
        expected_shape: "204 no content",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/system/credentials",
        pylon_route: "/api/v1/system/credentials",
        expected_shape: "credentials array and optional runtime_effect",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/system/credentials",
        pylon_route: "/api/v1/system/credentials",
        expected_shape: "created credential metadata",
        coverage: ContractCoverage::OutOfContract {
            reason: CREDENTIAL_MUTATION_REASON,
        },
    },
    DesktopApiContract {
        method: "DELETE",
        source_path: "/api/v1/system/credentials/{}",
        pylon_route: "/api/v1/system/credentials/{id}",
        expected_shape: "credential removal runtime effect",
        coverage: ContractCoverage::OutOfContract {
            reason: CREDENTIAL_MUTATION_REASON,
        },
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/system/credentials/{}/validate",
        pylon_route: "/api/v1/system/credentials/{id}/validate",
        expected_shape: "validated credential metadata",
        coverage: ContractCoverage::OutOfContract {
            reason: CREDENTIAL_MUTATION_REASON,
        },
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/system/credentials/rotate",
        pylon_route: "/api/v1/system/credentials/rotate",
        expected_shape: "rotated credentials envelope",
        coverage: ContractCoverage::OutOfContract {
            reason: CREDENTIAL_MUTATION_REASON,
        },
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/system/health",
        pylon_route: "/api/v1/system/health",
        expected_shape: "detailed health response",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/workspace/diff",
        pylon_route: "/api/v1/workspace/diff",
        expected_shape: "text/plain unified diff",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/workspace/files",
        pylon_route: "/api/v1/workspace/files",
        expected_shape: "workspace file entry array",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/workspace/files/content",
        pylon_route: "/api/v1/workspace/files/content",
        expected_shape: "raw file content",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "PUT",
        source_path: "/api/v1/workspace/files/content",
        pylon_route: "/api/v1/workspace/files/content",
        expected_shape: "write response with path/size/mtime_ms",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/workspace/git-status",
        pylon_route: "/api/v1/workspace/git-status",
        expected_shape: "git status entry array",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "POST",
        source_path: "/api/v1/workspace/open",
        pylon_route: "/api/v1/workspace/open",
        expected_shape: "open response or safe-open rejection error",
        coverage: ContractCoverage::Covered,
    },
    DesktopApiContract {
        method: "GET",
        source_path: "/api/v1/workspace/search",
        pylon_route: "/api/v1/workspace/search",
        expected_shape: "workspace search result array",
        coverage: ContractCoverage::Covered,
    },
];

#[derive(Debug)]
struct SseDataEvent {
    event: String,
    data: Value,
}

fn parse_sse_data_events(body: &str) -> Vec<SseDataEvent> {
    let mut events = Vec::new();
    let mut event_name: Option<String> = None;
    let mut data_lines = Vec::new();

    for line in body.lines() {
        if line.is_empty() {
            if data_lines.is_empty() {
                event_name = None;
            } else {
                let raw_data = data_lines.join("\n");
                let data = serde_json::from_str(&raw_data).unwrap_or_else(|err| {
                    panic!(
                        "proskenion SSE contract mismatch: data line was not JSON: {err}; \
                         event={event_name:?}; data={raw_data:?}; full body={body}"
                    )
                });
                events.push(SseDataEvent {
                    event: event_name.take().unwrap_or_else(|| "message".to_owned()),
                    data,
                });
                data_lines.clear();
            }
            continue;
        }

        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_owned());
        }
    }

    if !data_lines.is_empty() {
        let raw_data = data_lines.join("\n");
        let data = serde_json::from_str(&raw_data).unwrap_or_else(|err| {
            panic!(
                "proskenion SSE contract mismatch: trailing data line was not JSON: {err}; \
                 event={event_name:?}; data={raw_data:?}; full body={body}"
            )
        });
        events.push(SseDataEvent {
            event: event_name.unwrap_or_else(|| "message".to_owned()),
            data,
        });
    }

    events
}

fn string_field<'a>(json: &'a Value, field: &str, context: &str) -> &'a str {
    json.get(field).and_then(Value::as_str).unwrap_or_else(|| {
        panic!(
            "proskenion contract mismatch in {context}: expected string field `{field}`, got {json}"
        )
    })
}

fn array_field<'a>(json: &'a Value, field: &str, context: &str) -> &'a Vec<Value> {
    json.get(field).and_then(Value::as_array).unwrap_or_else(|| {
        panic!(
            "proskenion contract mismatch in {context}: expected array field `{field}`, got {json}"
        )
    })
}

fn object_field<'a>(
    json: &'a Value,
    field: &str,
    context: &str,
) -> &'a serde_json::Map<String, Value> {
    json.get(field).and_then(Value::as_object).unwrap_or_else(|| {
        panic!(
            "proskenion contract mismatch in {context}: expected object field `{field}`, got {json}"
        )
    })
}

fn numeric_field(json: &Value, field: &str, context: &str) {
    assert!(
        json.get(field).is_some_and(Value::is_number),
        "proskenion contract mismatch in {context}: expected numeric field `{field}`, got {json}"
    );
}

fn assert_event_type(event: &SseDataEvent) {
    let event_type = string_field(&event.data, "type", "SSE data envelope");
    assert_eq!(
        event.event, event_type,
        "proskenion SSE contract mismatch: `event:` must match JSON `type`; \
         event line={}; data={}",
        event.event, event.data
    );
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical repo root")
}

fn proskenion_source_root() -> PathBuf {
    repo_root().join("crates/theatron/proskenion/src")
}

fn collect_rust_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).expect("read proskenion source directory") {
        let path = entry.expect("read proskenion source entry").path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn quoted_strings(line: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut current = String::new();

    for ch in line.chars() {
        if in_string {
            if escaped {
                current.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                strings.push(std::mem::take(&mut current));
                in_string = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_string = true;
        }
    }

    strings
}

fn normalize_source_api_path(literal: &str) -> Option<String> {
    let start = literal.find("/api")?;
    let path_and_query = literal
        .get(start..)
        .expect("/api match starts on a UTF-8 boundary");
    let path = path_and_query
        .split(['?', '#'])
        .next()
        .expect("split always returns first segment");
    let normalized = path
        .split('/')
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with('}') {
                "{}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    Some(normalized)
}

fn add_discovered_path(
    discovered: &mut BTreeMap<String, Vec<String>>,
    source_root: &Path,
    file: &Path,
    line_number: usize,
    path: String,
) {
    let relative = file
        .strip_prefix(source_root)
        .expect("proskenion file under source root")
        .display();
    discovered
        .entry(path)
        .or_default()
        .push(format!("{relative}:{line_number}"));
}

fn discover_proskenion_api_paths() -> BTreeMap<String, Vec<String>> {
    let source_root = proskenion_source_root();
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files);

    let mut discovered = BTreeMap::new();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("read proskenion source file");
        let mut cfg_test_pending = false;
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            if cfg_test_pending && trimmed.starts_with("mod tests") {
                break;
            }
            cfg_test_pending = trimmed.starts_with("#[cfg(test)]");

            for literal in quoted_strings(line) {
                if let Some(path) = normalize_source_api_path(&literal) {
                    add_discovered_path(&mut discovered, &source_root, &file, index + 1, path);
                }
            }

            if line.contains("project_verification_url(") {
                add_discovered_path(
                    &mut discovered,
                    &source_root,
                    &file,
                    index + 1,
                    "/api/v1/planning/projects/{}/verification".to_owned(),
                );
            }
            if line.contains("project_verification_refresh_url(") {
                add_discovered_path(
                    &mut discovered,
                    &source_root,
                    &file,
                    index + 1,
                    "/api/v1/planning/projects/{}/verification/refresh".to_owned(),
                );
            }
        }
    }

    discovered
}

fn assert_error_code(json: &Value, expected: &str, context: &str) {
    let error = object_field(json, "error", context);
    assert_eq!(
        error.get("code").and_then(Value::as_str),
        Some(expected),
        "proskenion contract mismatch in {context}: expected error code {expected:?}, got {json}"
    );
}

#[cfg(feature = "knowledge-store")]
fn make_contract_fact(id: &str, content: &str, confidence: f64) -> mneme::knowledge::Fact {
    use mneme::id::FactId;
    use mneme::knowledge::{
        EpistemicTier, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
        Visibility,
    };

    mneme::knowledge::Fact {
        id: FactId::new(id).expect("contract fact id"),
        nous_id: TEST_NOUS_ID.to_owned(),
        fact_type: "knowledge".to_owned(),
        content: content.to_owned(),
        scope: None,
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
        temporal: FactTemporal {
            valid_from: jiff::Timestamp::UNIX_EPOCH,
            valid_to: jiff::Timestamp::UNIX_EPOCH,
            recorded_at: jiff::Timestamp::UNIX_EPOCH,
        },
        provenance: FactProvenance {
            confidence,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: 24.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    }
}

struct RawResponse {
    status: u16,
    body: Vec<u8>,
}

impl RawResponse {
    fn body_json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("proskenion contract JSON body")
    }
}

async fn raw_get(addr: std::net::SocketAddr, path: &str, token: &str) -> RawResponse {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect proskenion contract TCP server");
    let request = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Authorization: Bearer {token}\r\n\
         Connection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write proskenion contract HTTP request");

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .expect("read proskenion contract HTTP response");

    parse_http_response(&buf)
}

fn parse_http_response(bytes: &[u8]) -> RawResponse {
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("HTTP response missing header terminator");
    let head =
        std::str::from_utf8(&bytes[..header_end]).expect("HTTP response headers must be UTF-8");
    let mut lines = head.lines();
    let status_line = lines.next().expect("HTTP response missing status line");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .expect("HTTP response missing status code")
        .parse::<u16>()
        .expect("HTTP response status code must be numeric");

    let encoded_body = &bytes[header_end + 4..];
    let body = if head
        .lines()
        .any(|line| line.eq_ignore_ascii_case("transfer-encoding: chunked"))
    {
        decode_chunked(encoded_body)
    } else {
        encoded_body.to_vec()
    };

    RawResponse { status, body }
}

fn decode_chunked(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let line_end = bytes[index..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .expect("chunk size line terminator");
        let size_str =
            std::str::from_utf8(&bytes[index..index + line_end]).expect("chunk size UTF-8");
        let size = usize::from_str_radix(size_str.trim(), 16).expect("chunk size hex");
        index += line_end + 2;
        if size == 0 {
            break;
        }
        out.extend_from_slice(&bytes[index..index + size]);
        index += size + 2;
    }
    out
}

fn server_addr(base_url: &str) -> std::net::SocketAddr {
    base_url
        .strip_prefix("http://")
        .unwrap_or(base_url)
        .parse()
        .expect("parse proskenion contract server address")
}

async fn authed_get_json(addr: std::net::SocketAddr, token: &str, path: &str) -> Value {
    let resp = raw_get(addr, path, token).await;
    assert_eq!(
        resp.status,
        StatusCode::OK.as_u16(),
        "proskenion contract mismatch: GET {path} must return 200; body={}",
        String::from_utf8_lossy(&resp.body)
    );
    resp.body_json()
}

#[test]
fn proskenion_api_inventory_covers_all_desktop_api_paths() {
    let discovered = discover_proskenion_api_paths();
    let inventoried: BTreeSet<&str> = PROSKENION_API_INVENTORY
        .iter()
        .map(|entry| entry.source_path)
        .collect();

    let missing = discovered
        .iter()
        .filter(|(path, _locations)| !inventoried.contains(path.as_str()))
        .map(|(path, locations)| format!("{path} at {}", locations.join(", ")))
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "proskenion API inventory drift: add every new desktop /api path to \
         PROSKENION_API_INVENTORY with a Pylon route and response shape. Missing:\n{}",
        missing.join("\n")
    );
}

#[test]
fn proskenion_api_inventory_entries_declare_contract_status() {
    let mut invalid = Vec::new();
    for entry in PROSKENION_API_INVENTORY {
        match entry.coverage {
            ContractCoverage::Covered => {
                if entry.pylon_route == "<out-of-contract>" || entry.expected_shape.is_empty() {
                    invalid.push(format!(
                        "{} {} must name a Pylon route and expected shape",
                        entry.method, entry.source_path
                    ));
                }
            }
            ContractCoverage::OutOfContract { reason } => {
                if reason.trim().is_empty() {
                    invalid.push(format!(
                        "{} {} must document why it is out of contract",
                        entry.method, entry.source_path
                    ));
                }
            }
        }
    }

    assert!(
        invalid.is_empty(),
        "proskenion API inventory rows need explicit coverage metadata:\n{}",
        invalid.join("\n")
    );
}

#[tokio::test]
async fn proskenion_contract_nous_surfaces_match_desktop() {
    let harness = TestHarness::build_with_provider_and_tools(
        Box::new(MockProvider::new("Hello from mock!").models(&["mock-model"])),
        true,
    )
    .await;
    let (base_url, token, _harness) = harness.start_tcp_server().await;
    let addr = server_addr(&base_url);

    let listed = authed_get_json(addr, &token, "/api/v1/nous").await;
    let nous = array_field(&listed, "nous", "nous list");
    let agent = nous
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(TEST_NOUS_ID))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: nous list did not include {TEST_NOUS_ID}; \
                 body={listed}"
            )
        });
    assert_eq!(
        string_field(agent, "name", "nous list item"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: list item should expose display name; item={agent}"
    );
    assert_eq!(
        string_field(agent, "model", "nous list item"),
        "mock-model",
        "proskenion contract mismatch: list item should expose model; item={agent}"
    );
    assert_eq!(
        string_field(agent, "status", "nous list item"),
        "active",
        "proskenion contract mismatch: list item should expose active status; item={agent}"
    );

    let status = authed_get_json(addr, &token, &format!("/api/v1/nous/{TEST_NOUS_ID}")).await;
    assert_eq!(
        string_field(&status, "id", "nous status"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: status should keep requested nous id; body={status}"
    );
    assert_eq!(
        string_field(&status, "model", "nous status"),
        "mock-model",
        "proskenion contract mismatch: status should expose model; body={status}"
    );
    numeric_field(&status, "context_window", "nous status");
    numeric_field(&status, "max_output_tokens", "nous status");
    numeric_field(&status, "thinking_budget", "nous status");
    numeric_field(&status, "max_tool_iterations", "nous status");
    assert!(
        status
            .get("thinking_enabled")
            .and_then(Value::as_bool)
            .is_some(),
        "proskenion contract mismatch: status should expose boolean thinking_enabled; body={status}"
    );
    assert!(
        status.get("status").and_then(Value::as_str).is_some(),
        "proskenion contract mismatch: status should expose lifecycle status string; body={status}"
    );

    let tools = authed_get_json(addr, &token, &format!("/api/v1/nous/{TEST_NOUS_ID}/tools")).await;
    let tool_items = array_field(&tools, "tools", "nous tools");
    assert!(
        !tool_items.is_empty(),
        "proskenion contract mismatch: tools response should include registered built-in tools; body={tools}"
    );
    let first_tool = &tool_items[0];
    for field in ["name", "description", "category"] {
        assert!(
            first_tool.get(field).and_then(Value::as_str).is_some(),
            "proskenion contract mismatch: tool item missing string `{field}`; item={first_tool}"
        );
    }
    assert!(
        first_tool
            .get("auto_activate")
            .and_then(Value::as_bool)
            .is_some(),
        "proskenion contract mismatch: tool item should expose boolean auto_activate; item={first_tool}"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn proskenion_contract_knowledge_browse_surfaces_match_desktop() {
    let harness = TestHarness::build_with_knowledge_store().await;
    let (base_url, token, _harness) = harness.start_tcp_server().await;
    let addr = server_addr(&base_url);

    let facts = authed_get_json(addr, &token, "/api/v1/knowledge/facts?limit=25").await;
    array_field(&facts, "facts", "knowledge facts");
    numeric_field(&facts, "total", "knowledge facts");

    let entities = authed_get_json(addr, &token, "/api/v1/knowledge/entities?limit=25").await;
    array_field(&entities, "entities", "knowledge entities");
    numeric_field(&entities, "total", "knowledge entities");

    let timeline = authed_get_json(addr, &token, "/api/v1/knowledge/timeline?limit=25").await;
    array_field(&timeline, "events", "knowledge timeline");
    numeric_field(&timeline, "total", "knowledge timeline");

    let relationships = authed_get_json(
        addr,
        &token,
        "/api/v1/knowledge/entities/missing-entity/relationships",
    )
    .await;
    array_field(
        &relationships,
        "relationships",
        "knowledge entity relationships",
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "WHY(#4514): contract test keeps related workspace endpoint assertions together"
)]
#[tokio::test]
async fn proskenion_contract_workspace_surfaces_match_desktop() {
    let harness = TestHarness::build().await;
    let unsafe_open_target = harness.state.workspace_root.join("contract-script.sh");
    tokio::fs::write(&unsafe_open_target, "echo contract\n")
        .await
        .expect("write unsafe open fixture");
    let git_init = tokio::process::Command::new("git")
        .arg("-C")
        .arg(&harness.state.workspace_root)
        .arg("init")
        .output()
        .await
        .expect("git init workspace fixture");
    assert!(
        git_init.status.success(),
        "workspace git fixture init failed: {}",
        String::from_utf8_lossy(&git_init.stderr)
    );
    let router = harness.router();

    let req = harness.authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({
            "path": "contract-note.md",
            "content": "Alice records the desktop contract inventory.\n"
        })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("PUT /api/v1/workspace/files/content");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: workspace write must return 200"
    );
    let written = body_json(resp).await;
    assert_eq!(
        string_field(&written, "path", "workspace write"),
        "contract-note.md",
        "proskenion contract mismatch: write response should keep path; body={written}"
    );
    numeric_field(&written, "size", "workspace write");
    numeric_field(&written, "mtime_ms", "workspace write");

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/workspace/files"))
        .await
        .expect("GET /api/v1/workspace/files");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: workspace root list must return 200"
    );
    let files = body_json(resp).await;
    let files = files.as_array().unwrap_or_else(|| {
        panic!("proskenion contract mismatch: workspace files should be an array, got {files}")
    });
    let note = files
        .iter()
        .find(|item| item.get("path").and_then(Value::as_str) == Some("contract-note.md"))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: workspace list did not include note; body={files:?}"
            )
        });
    for field in ["name", "path"] {
        assert!(
            note.get(field).and_then(Value::as_str).is_some(),
            "proskenion contract mismatch: file entry missing string `{field}`; item={note}"
        );
    }
    assert!(
        note.get("is_dir").and_then(Value::as_bool).is_some(),
        "proskenion contract mismatch: file entry missing bool is_dir; item={note}"
    );
    numeric_field(note, "size", "workspace file entry");

    let content = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/workspace/files/content?path=contract-note.md"))
        .await
        .expect("GET /api/v1/workspace/files/content");
    assert_eq!(
        content.status(),
        StatusCode::OK,
        "proskenion contract mismatch: workspace content read must return 200"
    );
    let content = body_string(content).await;
    assert!(
        content.contains("desktop contract inventory"),
        "proskenion contract mismatch: content response should be raw file text; body={content:?}"
    );

    let git_status = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/workspace/git-status"))
        .await
        .expect("GET /api/v1/workspace/git-status");
    assert_eq!(
        git_status.status(),
        StatusCode::OK,
        "proskenion contract mismatch: workspace git status must return 200"
    );
    let git_status = body_json(git_status).await;
    assert!(
        git_status.as_array().is_some(),
        "proskenion contract mismatch: git status should be an array, got {git_status}"
    );

    let diff = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/workspace/diff?path=contract-note.md"))
        .await
        .expect("GET /api/v1/workspace/diff");
    assert_eq!(
        diff.status(),
        StatusCode::OK,
        "proskenion contract mismatch: workspace diff must return 200"
    );
    let _diff_text = body_string(diff).await;

    let search = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/workspace/search?q=contract&limit=50"))
        .await
        .expect("GET /api/v1/workspace/search");
    assert_eq!(
        search.status(),
        StatusCode::OK,
        "proskenion contract mismatch: workspace search must return 200"
    );
    let search = body_json(search).await;
    let results = search.as_array().unwrap_or_else(|| {
        panic!("proskenion contract mismatch: workspace search should be an array, got {search}")
    });
    let result = results
        .iter()
        .find(|item| item.get("path").and_then(Value::as_str) == Some("contract-note.md"))
        .unwrap_or_else(|| {
            panic!("proskenion contract mismatch: search did not include note; body={results:?}")
        });
    numeric_field(result, "line", "workspace search result");
    string_field(result, "snippet", "workspace search result");

    let open = router
        .oneshot(harness.authed_request(
            "POST",
            "/api/v1/workspace/open",
            Some(serde_json::json!({ "path": "contract-script.sh" })),
        ))
        .await
        .expect("POST /api/v1/workspace/open");
    assert_eq!(
        open.status(),
        StatusCode::BAD_REQUEST,
        "proskenion contract mismatch: unsafe workspace open should be rejected before xdg-open"
    );
    let open_error = body_json(open).await;
    assert_error_code(&open_error, "bad_request", "workspace open rejection");
}

#[cfg(feature = "knowledge-store")]
#[expect(
    clippy::too_many_lines,
    reason = "WHY(#4514): contract test keeps related memory detail and mutation assertions together"
)]
#[tokio::test]
async fn proskenion_contract_memory_detail_and_actions_match_desktop() {
    use mneme::id::{EntityId, FactId};
    use mneme::knowledge::{Entity, Relationship};

    let harness = TestHarness::build_with_knowledge_store().await;
    let store = harness.knowledge_store();
    let now = jiff::Timestamp::UNIX_EPOCH;
    let alice = EntityId::new("entity-alice").expect("alice entity id");
    let bob = EntityId::new("entity-bob").expect("bob entity id");
    let temp = EntityId::new("entity-temp").expect("temp entity id");

    for entity in [
        Entity {
            id: alice.clone(),
            name: "Alice".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec!["A".to_owned()],
            created_at: now,
            updated_at: now,
        },
        Entity {
            id: bob.clone(),
            name: "Bob".to_owned(),
            entity_type: "person".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
        Entity {
            id: temp,
            name: "Temporary".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        },
    ] {
        store
            .insert_entity(&entity)
            .expect("insert contract entity");
    }

    store
        .insert_relationship(&Relationship {
            src: alice.clone(),
            dst: bob.clone(),
            relation: "collaborates_with".to_owned(),
            weight: 0.8,
            created_at: now,
        })
        .expect("insert contract relationship");

    let fact_alice = make_contract_fact(
        "fact-alice-contract",
        "Alice maintains the desktop contract inventory.",
        0.95,
    );
    let fact_bob = make_contract_fact("fact-bob-contract", "Bob reviews contract coverage.", 0.9);
    store.insert_fact(&fact_alice).expect("insert alice fact");
    store.insert_fact(&fact_bob).expect("insert bob fact");
    store
        .insert_fact_entity(
            &FactId::new("fact-alice-contract").expect("alice fact id"),
            &alice,
        )
        .expect("link alice fact");
    store
        .insert_fact_entity(
            &FactId::new("fact-bob-contract").expect("bob fact id"),
            &bob,
        )
        .expect("link bob fact");

    let router = harness.router();

    let entity = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/knowledge/entities/entity-alice"))
        .await
        .expect("GET /api/v1/knowledge/entities/{id}");
    assert_eq!(
        entity.status(),
        StatusCode::OK,
        "proskenion contract mismatch: entity detail must return 200"
    );
    let entity = body_json(entity).await;
    assert_eq!(
        string_field(&entity, "name", "knowledge entity detail"),
        "Alice",
        "proskenion contract mismatch: entity detail should expose name; body={entity}"
    );
    string_field(&entity, "entity_type", "knowledge entity detail");

    let relationships = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/knowledge/entities/entity-alice/relationships"))
        .await
        .expect("GET /api/v1/knowledge/entities/{id}/relationships");
    assert_eq!(
        relationships.status(),
        StatusCode::OK,
        "proskenion contract mismatch: entity relationships must return 200"
    );
    let relationships = body_json(relationships).await;
    let relationships = array_field(&relationships, "relationships", "entity relationships");
    assert!(
        relationships.iter().any(|item| {
            item.get("entity_id").and_then(Value::as_str) == Some("entity-bob")
                && item.get("relationship_type").and_then(Value::as_str)
                    == Some("collaborates_with")
        }),
        "proskenion contract mismatch: relationships should include Bob edge; body={relationships:?}"
    );

    let memories = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/knowledge/entities/entity-alice/memories"))
        .await
        .expect("GET /api/v1/knowledge/entities/{id}/memories");
    assert_eq!(
        memories.status(),
        StatusCode::OK,
        "proskenion contract mismatch: entity memories must return 200"
    );
    let memories = body_json(memories).await;
    let memories = memories.as_array().unwrap_or_else(|| {
        panic!("proskenion contract mismatch: entity memories should be an array, got {memories}")
    });
    assert!(
        memories.iter().any(|item| {
            item.get("id").and_then(Value::as_str) == Some("fact-alice-contract")
                && item
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|content| content.contains("desktop contract inventory"))
        }),
        "proskenion contract mismatch: memories should include linked fact; body={memories:?}"
    );

    let flag = router
        .clone()
        .oneshot(harness.authed_request(
            "POST",
            "/api/v1/knowledge/entities/entity-alice/flag",
            Some(serde_json::json!({ "reason": "review merge target", "severity": "low" })),
        ))
        .await
        .expect("POST /api/v1/knowledge/entities/{id}/flag");
    assert_eq!(
        flag.status(),
        StatusCode::NO_CONTENT,
        "proskenion contract mismatch: entity flag should return 204"
    );

    let merge = router
        .clone()
        .oneshot(harness.authed_request(
            "POST",
            "/api/v1/knowledge/entities/merge",
            Some(serde_json::json!({
                "canonical_id": "entity-alice",
                "merged_id": "entity-bob"
            })),
        ))
        .await
        .expect("POST /api/v1/knowledge/entities/merge");
    assert_eq!(
        merge.status(),
        StatusCode::NO_CONTENT,
        "proskenion contract mismatch: entity merge should return 204"
    );

    let delete = router
        .clone()
        .oneshot(harness.authed_request("DELETE", "/api/v1/knowledge/entities/entity-temp", None))
        .await
        .expect("DELETE /api/v1/knowledge/entities/{id}");
    assert_eq!(
        delete.status(),
        StatusCode::NO_CONTENT,
        "proskenion contract mismatch: entity delete should return 204"
    );

    let forget = router
        .clone()
        .oneshot(harness.authed_request(
            "POST",
            "/api/v1/knowledge/facts/fact-alice-contract/forget",
            Some(serde_json::json!({ "reason": "user_requested" })),
        ))
        .await
        .expect("POST /api/v1/knowledge/facts/{id}/forget");
    assert_eq!(
        forget.status(),
        StatusCode::NO_CONTENT,
        "proskenion contract mismatch: fact forget should return 204"
    );

    let restore = router
        .clone()
        .oneshot(harness.authed_request(
            "POST",
            "/api/v1/knowledge/facts/fact-alice-contract/restore",
            None,
        ))
        .await
        .expect("POST /api/v1/knowledge/facts/{id}/restore");
    assert_eq!(
        restore.status(),
        StatusCode::NO_CONTENT,
        "proskenion contract mismatch: fact restore should return 204"
    );

    let confidence = router
        .clone()
        .oneshot(harness.authed_request(
            "PUT",
            "/api/v1/knowledge/facts/fact-alice-contract/confidence",
            Some(serde_json::json!({ "confidence": 0.88 })),
        ))
        .await
        .expect("PUT /api/v1/knowledge/facts/{id}/confidence");
    assert_eq!(
        confidence.status(),
        StatusCode::OK,
        "proskenion contract mismatch: confidence update should return 200"
    );
    let confidence = body_json(confidence).await;
    assert_eq!(
        string_field(&confidence, "status", "confidence update"),
        "updated",
        "proskenion contract mismatch: confidence update should expose status; body={confidence}"
    );
    numeric_field(&confidence, "confidence", "confidence update");

    let sensitivity = router
        .oneshot(harness.authed_request(
            "PUT",
            "/api/v1/knowledge/facts/fact-alice-contract/sensitivity",
            Some(serde_json::json!({ "sensitivity": "internal" })),
        ))
        .await
        .expect("PUT /api/v1/knowledge/facts/{id}/sensitivity");
    assert_eq!(
        sensitivity.status(),
        StatusCode::OK,
        "proskenion contract mismatch: sensitivity update should return 200"
    );
    let sensitivity = body_json(sensitivity).await;
    assert_eq!(
        string_field(&sensitivity, "sensitivity", "sensitivity update"),
        "internal",
        "proskenion contract mismatch: sensitivity update should expose new value; body={sensitivity}"
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "contract test keeps related metrics endpoint assertions together"
)]
#[tokio::test]
async fn proskenion_contract_metrics_surfaces_match_desktop() {
    let harness = TestHarness::build().await;
    let (base_url, token, _harness) = harness.start_tcp_server().await;
    let addr = server_addr(&base_url);

    let agents = authed_get_json(addr, &token, "/api/v1/metrics/agents").await;
    let agent_metrics = array_field(&agents, "agents", "agent metrics");
    assert!(
        agents.get("anomalies").and_then(Value::as_array).is_some(),
        "proskenion contract mismatch: agent metrics should expose anomalies array; body={agents}"
    );
    let test_agent = agent_metrics
        .iter()
        .find(|item| item.get("agent_id").and_then(Value::as_str) == Some(TEST_NOUS_ID))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: metrics agents did not include {TEST_NOUS_ID}; \
                 body={agents}"
            )
        });
    assert_eq!(
        string_field(test_agent, "agent_name", "agent metrics item"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: agent metrics should expose agent name; item={test_agent}"
    );
    for field in [
        "avg_tokens_per_response",
        "tool_calls_per_session",
        "tool_success_rate",
        "distillation_frequency",
        "avg_context_before_distill",
        "messages_per_session",
        "sessions_per_day",
        "errors_per_session",
    ] {
        numeric_field(test_agent, field, "agent metrics item");
    }

    let quality = authed_get_json(addr, &token, "/api/v1/metrics/quality").await;
    let series = object_field(&quality, "series", "quality metrics");
    for field in [
        "avg_turn_length",
        "response_to_question_ratio",
        "tool_call_density",
        "thinking_time_ratio",
    ] {
        assert!(
            series.get(field).and_then(Value::as_array).is_some(),
            "proskenion contract mismatch: quality series missing array `{field}`; body={quality}"
        );
    }

    let tokens = authed_get_json(
        addr,
        &token,
        "/api/v1/metrics/tokens?granularity=daily&from=2026-01-01&to=2026-12-31",
    )
    .await;
    array_field(&tokens, "series", "token metrics");
    array_field(&tokens, "agents", "token metrics");
    array_field(&tokens, "models", "token metrics");
    for field in [
        "today_input",
        "today_output",
        "week_input",
        "week_output",
        "month_input",
        "month_output",
        "prev_today_input",
        "prev_today_output",
        "prev_week_input",
        "prev_week_output",
        "prev_month_input",
        "prev_month_output",
    ] {
        numeric_field(&tokens, field, "token metrics");
    }
    let token_agents = array_field(&tokens, "agents", "token metrics");
    let token_agent = token_agents
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(TEST_NOUS_ID))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: token metrics should include desktop agent row \
                 with id/name/input_tokens/output_tokens/session_count; body={tokens}"
            )
        });
    assert_eq!(
        string_field(token_agent, "name", "token metrics agent row"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: token metrics agent row should expose display name; \
         row={token_agent}"
    );
    for field in ["input_tokens", "output_tokens", "session_count"] {
        numeric_field(token_agent, field, "token metrics agent row");
    }
    let token_models = array_field(&tokens, "models", "token metrics");
    let token_model = token_models
        .iter()
        .find(|item| item.get("model").and_then(Value::as_str) == Some("mock-model"))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: token metrics should include desktop model row \
                 with model/input_tokens/output_tokens/session_count; body={tokens}"
            )
        });
    for field in ["input_tokens", "output_tokens", "session_count"] {
        numeric_field(token_model, field, "token metrics model row");
    }

    let costs = authed_get_json(
        addr,
        &token,
        "/api/v1/metrics/costs?granularity=daily&from=2026-01-01&to=2026-12-31",
    )
    .await;
    array_field(&costs, "series", "cost metrics");
    let cost_agents = array_field(&costs, "agents", "cost metrics");
    for field in [
        "today_cost",
        "week_cost",
        "month_cost",
        "prev_today_cost",
        "prev_week_cost",
        "prev_month_cost",
    ] {
        numeric_field(&costs, field, "cost metrics");
    }
    let cost_agent = cost_agents
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(TEST_NOUS_ID))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: cost metrics should include desktop agent row \
                 with id/name/total_cost/message_count/session_count/output_tokens/prev_period_cost; \
                 body={costs}"
            )
        });
    assert_eq!(
        string_field(cost_agent, "name", "cost metrics agent row"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: cost metrics agent row should expose display name; \
         row={cost_agent}"
    );
    for field in [
        "total_cost",
        "message_count",
        "session_count",
        "output_tokens",
        "prev_period_cost",
    ] {
        numeric_field(cost_agent, field, "cost metrics agent row");
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "WHY(#4514): contract test keeps related ops/config/toggle assertions together"
)]
#[tokio::test]
async fn proskenion_contract_ops_config_and_toggle_surfaces_match_desktop() {
    let harness = TestHarness::build_with_provider_and_tools(
        Box::new(MockProvider::new("Hello from mock!").models(&["mock-model"])),
        true,
    )
    .await;
    let router = harness.router();

    let health = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/system/health"))
        .await
        .expect("GET /api/v1/system/health");
    assert!(
        matches!(
            health.status(),
            StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE
        ),
        "proskenion contract mismatch: system health should return readiness status, got {}",
        health.status()
    );
    let health = body_json(health).await;
    string_field(&health, "status", "system health");
    string_field(&health, "version", "system health");
    numeric_field(&health, "uptime_seconds", "system health");
    array_field(&health, "checks", "system health");
    string_field(&health, "data_dir", "system health");

    let config = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/config"))
        .await
        .expect("GET /api/v1/config");
    assert_eq!(
        config.status(),
        StatusCode::OK,
        "proskenion contract mismatch: config read must return 200"
    );
    let config = body_json(config).await;
    assert!(
        config.get("agents").is_some() && config.get("gateway").is_some(),
        "proskenion contract mismatch: config response should expose redacted config sections; body={config}"
    );

    let credentials = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/system/credentials"))
        .await
        .expect("GET /api/v1/system/credentials");
    assert_eq!(
        credentials.status(),
        StatusCode::OK,
        "proskenion contract mismatch: credentials list must return 200"
    );
    let credentials = body_json(credentials).await;
    array_field(&credentials, "credentials", "credentials list");

    let tools = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/ops/tools"))
        .await
        .expect("GET /api/v1/ops/tools");
    assert_eq!(
        tools.status(),
        StatusCode::OK,
        "proskenion contract mismatch: ops tools must return 200"
    );
    let tools = body_json(tools).await;
    let catalog = array_field(&tools, "catalog", "ops tools");
    assert!(
        !catalog.is_empty(),
        "proskenion contract mismatch: ops tools catalog should include registered tools; body={tools}"
    );
    array_field(&tools, "live_invocations", "ops tools");
    numeric_field(&tools, "total_calls", "ops tools");
    numeric_field(&tools, "total_errors", "ops tools");
    assert!(
        tools
            .get("history_unavailable")
            .and_then(Value::as_bool)
            .is_some(),
        "proskenion contract mismatch: ops tools should expose bool history_unavailable; body={tools}"
    );

    let agent_tools = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/nous/{TEST_NOUS_ID}/tools")))
        .await
        .expect("GET /api/v1/nous/{id}/tools");
    assert_eq!(
        agent_tools.status(),
        StatusCode::OK,
        "proskenion contract mismatch: agent tools must return 200"
    );
    let agent_tools = body_json(agent_tools).await;
    let agent_tools_array = array_field(&agent_tools, "tools", "agent tools");
    let tool_name = string_field(
        agent_tools_array
            .first()
            .unwrap_or_else(|| panic!("agent tools should not be empty; body={agent_tools}")),
        "name",
        "agent tool item",
    )
    .to_owned();

    let agent_toggle = router
        .clone()
        .oneshot(harness.authed_request(
            "PATCH",
            &format!("/api/v1/nous/{TEST_NOUS_ID}"),
            Some(serde_json::json!({ "enabled": false })),
        ))
        .await
        .expect("PATCH /api/v1/nous/{id}");
    assert_eq!(
        agent_toggle.status(),
        StatusCode::OK,
        "proskenion contract mismatch: agent toggle must return 200"
    );
    let agent_toggle = body_json(agent_toggle).await;
    assert_eq!(
        string_field(&agent_toggle, "id", "agent toggle"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: agent toggle should keep id; body={agent_toggle}"
    );
    assert_eq!(
        agent_toggle.get("enabled").and_then(Value::as_bool),
        Some(false),
        "proskenion contract mismatch: agent toggle should expose new enabled state; body={agent_toggle}"
    );
    for field in [
        "config_applied",
        "live_applied",
        "reload_required",
        "restart_required",
    ] {
        assert!(
            agent_toggle.get(field).and_then(Value::as_bool).is_some(),
            "proskenion contract mismatch: agent toggle missing bool `{field}`; body={agent_toggle}"
        );
    }

    let tool_toggle = router
        .clone()
        .oneshot(harness.authed_request(
            "PATCH",
            &format!("/api/v1/nous/{TEST_NOUS_ID}/tools"),
            Some(serde_json::json!({ "tool": tool_name, "enabled": false })),
        ))
        .await
        .expect("PATCH /api/v1/nous/{id}/tools");
    assert_eq!(
        tool_toggle.status(),
        StatusCode::OK,
        "proskenion contract mismatch: tool toggle must return 200"
    );
    let tool_toggle = body_json(tool_toggle).await;
    let toggled_tools = array_field(&tool_toggle, "tools", "tool toggle");
    assert!(
        toggled_tools.iter().any(|item| {
            item.get("name").and_then(Value::as_str) == Some(tool_name.as_str())
                && item.get("enabled").and_then(Value::as_bool) == Some(false)
        }),
        "proskenion contract mismatch: tool toggle response should include disabled tool; body={tool_toggle}"
    );
    for field in [
        "config_applied",
        "live_applied",
        "reload_required",
        "restart_required",
    ] {
        assert!(
            tool_toggle.get(field).and_then(Value::as_bool).is_some(),
            "proskenion contract mismatch: tool toggle missing bool `{field}`; body={tool_toggle}"
        );
    }

    let feature_flags = router
        .clone()
        .oneshot(harness.authed_request(
            "PUT",
            "/api/v1/config/feature_flags",
            Some(serde_json::json!([])),
        ))
        .await
        .expect("PUT /api/v1/config/feature_flags");
    assert_eq!(
        feature_flags.status(),
        StatusCode::OK,
        "proskenion contract mismatch: feature flag config update must return 200"
    );
    let feature_flags = body_json(feature_flags).await;
    assert_eq!(
        string_field(&feature_flags, "section", "feature flags update"),
        "feature_flags",
        "proskenion contract mismatch: feature flags update should name section; body={feature_flags}"
    );
    assert!(
        feature_flags
            .get("config")
            .and_then(Value::as_array)
            .is_some(),
        "proskenion contract mismatch: feature flags update should expose updated array; body={feature_flags}"
    );
    array_field(&feature_flags, "restart_required", "feature flags update");

    let journal = router
        .oneshot(harness.authed_get("/api/v1/journal"))
        .await
        .expect("GET /api/v1/journal");
    assert_eq!(
        journal.status(),
        StatusCode::OK,
        "proskenion contract mismatch: journal endpoint must return 200"
    );
    let journal = body_json(journal).await;
    array_field(&journal, "events", "journal");
    array_field(&journal, "data_unavailable", "journal");
}

#[tokio::test]
async fn proskenion_contract_planning_verification_fallback_matches_desktop() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let verification = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/planning/projects/missing-project/verification"))
        .await
        .expect("GET /api/v1/planning/projects/{id}/verification");
    assert_eq!(
        verification.status(),
        StatusCode::NOT_FOUND,
        "proskenion contract mismatch: missing planning verification should return 404 for desktop NotAvailable fallback"
    );
    let verification = body_json(verification).await;
    assert_error_code(
        &verification,
        "not_found",
        "planning verification missing project",
    );

    let refresh = router
        .oneshot(harness.authed_request(
            "POST",
            "/api/v1/planning/projects/missing-project/verification/refresh",
            Some(serde_json::json!({ "criteria": [] })),
        ))
        .await
        .expect("POST /api/v1/planning/projects/{id}/verification/refresh");
    assert_eq!(
        refresh.status(),
        StatusCode::NOT_FOUND,
        "proskenion contract mismatch: missing planning refresh should return 404 for desktop fallback"
    );
    let refresh = body_json(refresh).await;
    assert_error_code(
        &refresh,
        "not_found",
        "planning verification refresh missing project",
    );
}

#[tokio::test]
async fn proskenion_contract_global_events_sse_matches_desktop() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let subscribe = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/events/subscribe?topics=nous.lifecycle"))
        .await
        .expect("GET /api/v1/events/subscribe");
    assert_eq!(
        subscribe.status(),
        StatusCode::OK,
        "proskenion contract mismatch: global events subscribe must return 200"
    );
    let content_type = subscribe
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        content_type.starts_with("text/event-stream"),
        "proskenion contract mismatch: global events should return text/event-stream, got {content_type:?}"
    );

    let mut body = subscribe.into_body();
    let create = router
        .oneshot(harness.authed_request(
            "POST",
            "/api/v1/nous",
            Some(serde_json::json!({
                "id": "contract-event-nous",
                "name": "Contract Event Nous",
                "model": "mock-model"
            })),
        ))
        .await
        .expect("POST /api/v1/nous");
    assert_eq!(
        create.status(),
        StatusCode::CREATED,
        "proskenion contract mismatch: event fixture agent creation must return 201"
    );

    let event_text = tokio::time::timeout(Duration::from_secs(3), async {
        let mut collected = Vec::new();
        loop {
            let maybe_frame = body.frame().await.expect("global events body frame");
            let frame = maybe_frame.expect("global events stream should stay open");
            if let Some(chunk) = frame.data_ref() {
                collected.extend_from_slice(chunk);
                let text = String::from_utf8(collected.clone()).expect("global event frame UTF-8");
                if text.contains("\n\n") {
                    return text;
                }
            }
        }
    })
    .await
    .expect("global event frame timed out");

    let events = parse_sse_data_events(&event_text);
    let event = events
        .iter()
        .find(|event| event.event == "nous.lifecycle")
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: expected nous.lifecycle global event; \
                 events={events:?}; body={event_text}"
            )
        });
    assert_eq!(
        string_field(&event.data, "topic", "global event"),
        "nous.lifecycle",
        "proskenion contract mismatch: domain event should keep topic; data={}",
        event.data
    );
    let payload = object_field(&event.data, "payload", "global event");
    assert_eq!(
        payload.get("nous_id").and_then(Value::as_str),
        Some("contract-event-nous"),
        "proskenion contract mismatch: domain event payload should expose nous_id; data={}",
        event.data
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "contract test keeps create/list/history assertions in one scenario"
)]
#[tokio::test]
async fn proskenion_contract_session_create_list_history_matches_desktop() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": TEST_NOUS_ID,
            "session_key": "proskenion-contract"
        })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("POST /api/v1/sessions");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "proskenion contract mismatch: session create must return 201"
    );
    let created = body_json(resp).await;
    let session_id = string_field(&created, "id", "session create");
    assert_eq!(
        string_field(&created, "nous_id", "session create"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: created session should keep requested nous_id; body={created}"
    );
    assert_eq!(
        string_field(&created, "session_key", "session create"),
        "proskenion-contract",
        "proskenion contract mismatch: created session should keep requested session_key; body={created}"
    );
    assert_eq!(
        string_field(&created, "status", "session create"),
        "active",
        "proskenion contract mismatch: created sessions should be active; body={created}"
    );
    assert!(
        created
            .get("message_count")
            .and_then(Value::as_i64)
            .is_some(),
        "proskenion contract mismatch: create response must expose numeric message_count; body={created}"
    );
    assert!(
        created.get("created_at").and_then(Value::as_str).is_some()
            && created.get("updated_at").and_then(Value::as_str).is_some(),
        "proskenion contract mismatch: create response must expose created_at and updated_at; body={created}"
    );

    let req = harness.authed_get("/api/v1/sessions?limit=25");
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("GET /api/v1/sessions");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: session list must return 200"
    );
    let listed = body_json(resp).await;
    let items = array_field(&listed, "items", "session list");
    assert!(
        listed.get("has_more").and_then(Value::as_bool).is_some(),
        "proskenion contract mismatch: session list must expose boolean has_more; body={listed}"
    );
    let listed_session = items
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(session_id))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: session list did not include created session \
                 id={session_id}; body={listed}"
            )
        });
    assert_eq!(
        string_field(listed_session, "session_key", "session list item"),
        "proskenion-contract",
        "proskenion contract mismatch: list item must expose session_key; item={listed_session}"
    );

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{session_id}/messages"),
        Some(serde_json::json!({ "content": "hello from proskenion contract" })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("POST /api/v1/sessions/{id}/messages");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: message send must return 200"
    );
    let _stream_body = body_string(resp).await;

    let req = harness.authed_get(&format!("/api/v1/sessions/{session_id}/history"));
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("GET /api/v1/sessions/{id}/history");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: history must return 200"
    );
    let history = body_json(resp).await;
    let messages = array_field(&history, "messages", "history");
    assert!(
        messages.iter().any(|message| {
            message.get("role").and_then(Value::as_str) == Some("user")
                && message.get("content").and_then(Value::as_str)
                    == Some("hello from proskenion contract")
        }),
        "proskenion contract mismatch: history must contain the user message as \
         {{role, content}} strings; body={history}"
    );
    assert!(
        messages.iter().any(|message| {
            message.get("role").and_then(Value::as_str) == Some("assistant")
                && message.get("content").and_then(Value::as_str).is_some()
        }),
        "proskenion contract mismatch: history must contain an assistant message \
         with string content; body={history}"
    );

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{session_id}/archive"),
        None,
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("POST /api/v1/sessions/{id}/archive");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "proskenion contract mismatch: archive action must return 204"
    );

    let req = harness.authed_get("/api/v1/sessions?status=archived&limit=25");
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("GET /api/v1/sessions?status=archived");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: archived session list must return 200"
    );
    let archived = body_json(resp).await;
    let archived_items = array_field(&archived, "items", "archived session list");
    let archived_session = archived_items
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(session_id))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: archived session list did not include \
                 archived session id={session_id}; body={archived}"
            )
        });
    assert_eq!(
        string_field(archived_session, "status", "archived session list item"),
        "archived",
        "proskenion contract mismatch: archived list item should expose archived status; \
         item={archived_session}"
    );

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{session_id}/unarchive"),
        None,
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("POST /api/v1/sessions/{id}/unarchive");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "proskenion contract mismatch: unarchive action must return 204"
    );

    let req = harness.authed_get("/api/v1/sessions?status=active&limit=25");
    let resp = router
        .oneshot(req)
        .await
        .expect("GET /api/v1/sessions?status=active");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: active session list must return 200 after unarchive"
    );
    let active = body_json(resp).await;
    let active_items = array_field(&active, "items", "active session list");
    assert!(
        active_items.iter().any(
            |item| item.get("id").and_then(Value::as_str) == Some(session_id)
                && item.get("status").and_then(Value::as_str) == Some("active")
        ),
        "proskenion contract mismatch: active list should include unarchived session; body={active}"
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "contract test keeps SSE protocol assertions in one scenario"
)]
#[tokio::test]
async fn proskenion_contract_chat_stream_sse_matches_desktop() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": TEST_NOUS_ID,
            "session_key": "proskenion-stream-contract",
            "message": "stream this for desktop"
        })),
    );
    let resp = router
        .oneshot(req)
        .await
        .expect("POST /api/v1/sessions/stream");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion SSE contract mismatch: stream endpoint must return 200"
    );
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        content_type.starts_with("text/event-stream"),
        "proskenion SSE contract mismatch: stream endpoint must return \
         text/event-stream content-type, got {content_type:?}"
    );
    let body = body_string(resp).await;
    let events = parse_sse_data_events(&body);
    assert!(
        !events.is_empty(),
        "proskenion SSE contract mismatch: stream returned no data events; body={body}"
    );
    for event in &events {
        assert_event_type(event);
    }

    let start_index = events
        .iter()
        .position(|event| event.event == "message_start")
        .unwrap_or_else(|| {
            panic!(
                "proskenion SSE contract mismatch: expected message_start event \
                 before deltas; events={events:?}; body={body}"
            )
        });
    let start = &events[start_index];
    assert!(
        start
            .data
            .get("session_id")
            .and_then(Value::as_str)
            .is_some()
            && start.data.get("nous_id").and_then(Value::as_str) == Some(TEST_NOUS_ID)
            && start.data.get("turn_id").and_then(Value::as_str).is_some(),
        "proskenion SSE contract mismatch: message_start must expose \
         session_id, nous_id, and turn_id strings; data={}",
        start.data
    );

    let complete_index = events
        .iter()
        .position(|event| event.event == "message_complete")
        .unwrap_or_else(|| {
            panic!(
                "proskenion SSE contract mismatch: expected terminal message_complete; \
                 events={events:?}; body={body}"
            )
        });
    assert_eq!(
        complete_index,
        events.len() - 1,
        "proskenion SSE contract mismatch: message_complete must be the final \
         terminal event because desktop clients stop after terminal events; \
         events={events:?}; body={body}"
    );
    assert!(
        start_index < complete_index,
        "proskenion SSE contract mismatch: message_start must precede \
         message_complete; events={events:?}; body={body}"
    );
    let complete = &events[complete_index];
    let outcome = complete
        .data
        .get("outcome")
        .and_then(Value::as_object)
        .unwrap_or_else(|| {
            panic!(
                "proskenion SSE contract mismatch: message_complete must carry \
                 object outcome; data={}",
                complete.data
            )
        });
    for field in [
        "text",
        "nous_id",
        "session_id",
        "tool_calls",
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
    ] {
        assert!(
            outcome.contains_key(field),
            "proskenion SSE contract mismatch: outcome missing `{field}`; \
             outcome={outcome:?}; complete={}",
            complete.data
        );
    }
    assert!(
        outcome
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty()),
        "proskenion SSE contract mismatch: mock stream may complete without \
         intermediate text_delta, but terminal outcome.text must be non-empty; \
         outcome={outcome:?}; events={events:?}; body={body}"
    );
}
