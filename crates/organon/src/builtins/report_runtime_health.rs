//! Poiesis report runtime dependency doctor.
//!
//! `report_runtime_health` probes the external binaries that Poiesis-backed
//! report formats depend on (Pandoc, LaTeX, Chromium) plus the in-process Typst
//! renderer, and returns a structured readiness report.  Operators can use it
//! to predict report-runtime failures before they happen at render time.
//!
//! # Distinguishing optional missing dependencies from enabled-but-broken
//!
//! Each renderer reports:
//!
//! - `required` — whether the renderer is a hard requirement for the default
//!   report configuration.
//! - `configured` — whether the operator has explicitly enabled the renderer
//!   via an environment variable or requested format.
//! - `status` — one of `ok`, `missing`, `too_old`, `timeout`, or `error`.
//!
//! A missing renderer that is **not** configured is reported as `missing` and
//! does not make the overall status unhealthy.  A configured renderer that is
//! missing, too old, or times out is reported as `error` (or the specific
//! failure status) and degrades the overall status.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use indexmap::IndexMap;
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, Reversibility, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput,
    ToolResult, ToolTag,
};

/// Readiness status for a single report renderer.
#[derive(Debug, Clone, serde::Serialize)]
struct RendererHealth {
    name: &'static str,
    status: &'static str,
    required: bool,
    configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    message: String,
}

impl RendererHealth {
    fn ok(
        name: &'static str,
        required: bool,
        configured: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            name,
            status: "ok",
            required,
            configured,
            path: None,
            version: None,
            message: message.into(),
        }
    }

    fn missing(
        name: &'static str,
        required: bool,
        configured: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            name,
            status: "missing",
            required,
            configured,
            path: None,
            version: None,
            message: message.into(),
        }
    }

    #[cfg(test)]
    fn error(
        name: &'static str,
        required: bool,
        configured: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            name,
            status: "error",
            required,
            configured,
            path: None,
            version: None,
            message: message.into(),
        }
    }
}

/// Aggregated report runtime health response.
#[derive(Debug, Clone, serde::Serialize)]
struct ReportRuntimeHealth {
    status: &'static str,
    renderers: Vec<RendererHealth>,
}

/// Candidate Chromium binary names, in search order.
const CHROMIUM_CANDIDATES: &[&str] = &[
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
];

/// Probe for a Chromium binary using `CHROMIUM_PATH` and `PATH`.
///
/// Mirrors the discovery logic in `printer-chromium` so the doctor surface
/// reports the same binary the renderer would select.
fn find_chromium() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CHROMIUM_PATH") {
        let pb = PathBuf::from(&path);
        if pb.exists() {
            return Some(pb);
        }
    }
    for candidate in CHROMIUM_CANDIDATES {
        if let Ok(path) = which::which(candidate) {
            return Some(path);
        }
    }
    None
}

/// Whether the operator has explicitly required LaTeX rendering.
fn latex_required_from_env() -> bool {
    matches!(
        std::env::var("POIESIS_LATEX_REQUIRED").as_deref(),
        Ok("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

/// Probe Pandoc and classify its readiness.
fn check_pandoc() -> RendererHealth {
    // WHY: Pandoc is required for every non-Typst render path, so it is
    // always treated as configured and required.
    match poiesis_doc::PandocProbe::check() {
        poiesis_doc::PandocProbe::Present { path, version } => {
            let version_str = format!("{}.{}.{}", version.0, version.1, version.2);
            RendererHealth {
                name: "pandoc",
                status: "ok",
                required: true,
                configured: true,
                path: Some(path.to_string_lossy().into_owned()),
                version: Some(version_str.clone()),
                message: format!("pandoc {version_str} available at {}", path.display()),
            }
        }
        poiesis_doc::PandocProbe::Missing { searched } => RendererHealth::missing(
            "pandoc",
            true,
            true,
            format!(
                "pandoc not found on PATH (searched: {}); non-Typst report formats are unavailable",
                format_paths(&searched)
            ),
        ),
        poiesis_doc::PandocProbe::TooOld {
            path,
            found,
            required,
        } => RendererHealth {
            name: "pandoc",
            status: "too_old",
            required: true,
            configured: true,
            path: Some(path.to_string_lossy().into_owned()),
            version: Some(format!("{}.{}.{}", found.0, found.1, found.2)),
            message: format!(
                "pandoc at {} is too old (found {}.{}.{}, required {}.{}.{})",
                path.display(),
                found.0,
                found.1,
                found.2,
                required.0,
                required.1,
                required.2
            ),
        },
        poiesis_doc::PandocProbe::TimedOut { path, timeout_secs } => RendererHealth {
            name: "pandoc",
            status: "timeout",
            required: true,
            configured: true,
            path: Some(path.to_string_lossy().into_owned()),
            version: None,
            message: format!(
                "pandoc at {} timed out after {timeout_secs}s while probing --version",
                path.display()
            ),
        },
    }
}

/// Probe LaTeX and classify its readiness.
fn check_latex() -> RendererHealth {
    let required = false;
    let configured = latex_required_from_env();

    match poiesis_doc::LatexProbe::check() {
        poiesis_doc::LatexProbe::Present {
            engine,
            path,
            version,
        } => RendererHealth {
            name: "latex",
            status: "ok",
            required,
            configured,
            path: Some(path.to_string_lossy().into_owned()),
            version: Some(version.clone()),
            message: format!("{engine:?} available at {} ({version})", path.display()),
        },
        poiesis_doc::LatexProbe::Missing { searched } => {
            let status = if configured { "error" } else { "missing" };
            RendererHealth {
                name: "latex",
                status,
                required,
                configured,
                path: None,
                version: None,
                message: format!(
                    "latex engine not found (searched: {}); needed only for LaTeX PDF routes",
                    format_paths(&searched)
                ),
            }
        }
        poiesis_doc::LatexProbe::TimedOut { path, timeout_secs } => RendererHealth {
            name: "latex",
            status: "timeout",
            required,
            configured,
            path: Some(path.to_string_lossy().into_owned()),
            version: None,
            message: format!(
                "latex engine at {} timed out after {timeout_secs}s while probing --version",
                path.display()
            ),
        },
    }
}

/// Probe Chromium and classify its readiness.
fn check_chromium() -> RendererHealth {
    let required = false;
    let configured = std::env::var("CHROMIUM_PATH").is_ok();

    if let Some(path) = find_chromium() {
        RendererHealth {
            name: "chromium",
            status: "ok",
            required,
            configured,
            path: Some(path.to_string_lossy().into_owned()),
            version: None,
            message: format!("chromium available at {}", path.display()),
        }
    } else {
        let status = if configured { "error" } else { "missing" };
        RendererHealth {
            name: "chromium",
            status,
            required,
            configured,
            path: None,
            version: None,
            message: if configured {
                "CHROMIUM_PATH is set but the path does not exist; HTML-to-PDF is broken".to_owned()
            } else {
                "no chromium binary found on PATH; set CHROMIUM_PATH to enable HTML-to-PDF"
                    .to_owned()
            },
        }
    }
}

/// Typst is rendered in-process, so it is always available when the crate is
/// compiled into organon.
fn check_typst() -> RendererHealth {
    RendererHealth::ok("typst", true, true, "in-process Typst renderer available")
}

/// Format a list of paths for human-readable messages.
fn format_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "none".to_owned();
    }
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Compute the aggregate status from individual renderer checks.
///
/// - `unhealthy` if any required renderer is not ok.
/// - `degraded` if any configured (but not required) renderer is not ok.
/// - `healthy` otherwise.
fn aggregate_status(renderers: &[RendererHealth]) -> &'static str {
    let has_required_failure = renderers.iter().any(|r| r.required && r.status != "ok");
    let has_configured_failure = renderers
        .iter()
        .any(|r| r.configured && !r.required && r.status != "ok");

    if has_required_failure {
        "unhealthy"
    } else if has_configured_failure {
        "degraded"
    } else {
        "healthy"
    }
}

struct ReportRuntimeHealthExecutor;

impl ToolExecutor for ReportRuntimeHealthExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let renderers = vec![
                check_pandoc(),
                check_latex(),
                check_chromium(),
                check_typst(),
            ];
            let status = aggregate_status(&renderers);
            let report = ReportRuntimeHealth { status, renderers };

            match serde_json::to_string_pretty(&report) {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(format!(
                    "failed to serialize report runtime health: {e}"
                ))),
            }
        })
    }
}

fn report_runtime_health_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("report_runtime_health"), // kanon:ignore RUST/expect
        description: "Probe Poiesis report runtime dependencies (Pandoc, LaTeX, Chromium, \
                      Typst) and return a structured readiness report distinguishing optional \
                      missing dependencies from enabled-but-broken renderers."
            .to_owned(),
        extended_description: Some(
            "Returns JSON with a top-level `status` (`healthy`, `degraded`, or `unhealthy`) \
             and a `renderers` array. Each renderer includes `status`, `required`, `configured`, \
             `path`, `version`, and `message`. Pandoc is always required; LaTeX and Chromium are \
             optional unless explicitly enabled via `POIESIS_LATEX_REQUIRED` or `CHROMIUM_PATH`. \
             Typst is in-process and always reported as available."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

/// Register the `report_runtime_health` doctor tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(
        report_runtime_health_def(),
        Box::new(ReportRuntimeHealthExecutor),
    )?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    unsafe_code,
    reason = "WHY(#4655): set_var/remove_var are unsafe in edition 2024; ENV_LOCK serializes access"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions — missing key is a test bug"
)]
mod tests {
    use std::collections::HashSet;
    use std::ffi::OsStr;
    use std::sync::{Arc, Mutex, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use super::*;
    use crate::types::{ToolContext, ToolInput};

    /// Serializes tests that mutate process environment variables.
    // WHY: `std::env::set_var` is process-global; holding this lock while a
    // guard is alive prevents concurrent tests from seeing stale values.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Temporarily override an environment variable, restoring the previous
    /// value (or removing it) when dropped.
    struct EnvGuard {
        key: &'static str,
        old_value: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &OsStr) -> Self {
            let lock = ENV_LOCK.lock().expect("env lock poisoned");
            let old_value = std::env::var(key).ok();
            // SAFETY(#4655): ENV_LOCK is held for the duration of this guard; no
            // concurrent test can observe an inconsistent env state.
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                old_value,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY(#4655): ENV_LOCK is held while this guard is alive; drop
            // restores the original value atomically within the lock window.
            match &self.old_value {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn tool_input() -> ToolInput {
        ToolInput {
            name: ToolName::new("report_runtime_health").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({}),
        }
    }

    fn parse_result(result: &ToolResult) -> serde_json::Value {
        assert!(!result.is_error, "tool must succeed: {:?}", result.content);
        let text = result.content.text_summary();
        serde_json::from_str(&text).expect("output must be valid JSON")
    }

    fn renderer<'a>(json: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        json["renderers"]
            .as_array()
            .expect("renderers array")
            .iter()
            .find(|r| r["name"].as_str() == Some(name))
            .expect("missing renderer")
    }

    #[tokio::test]
    async fn report_runtime_health_returns_structured_json() {
        let ctx = test_ctx();
        let result = ReportRuntimeHealthExecutor
            .execute(&tool_input(), &ctx)
            .await
            .expect("exec");
        let json = parse_result(&result);

        let status = json["status"].as_str().expect("status");
        assert!(
            ["healthy", "degraded", "unhealthy"].contains(&status),
            "unexpected status: {status}"
        );

        for name in ["pandoc", "latex", "chromium", "typst"] {
            let r = renderer(&json, name);
            assert!(r["status"].is_string(), "{name} must have status");
            assert!(r["required"].is_boolean(), "{name} must have required");
            assert!(r["configured"].is_boolean(), "{name} must have configured");
            assert!(r["message"].is_string(), "{name} must have message");
        }
    }

    #[tokio::test]
    async fn typst_is_always_ok_and_required() {
        let ctx = test_ctx();
        let result = ReportRuntimeHealthExecutor
            .execute(&tool_input(), &ctx)
            .await
            .expect("exec");
        let json = parse_result(&result);
        let typst = renderer(&json, "typst");

        assert_eq!(typst["status"], "ok");
        assert_eq!(typst["required"], true);
        assert_eq!(typst["configured"], true);
    }

    #[tokio::test]
    async fn configured_missing_chromium_is_error() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let missing_path = dir.path().join("definitely-not-chromium");

        let _guard = EnvGuard::set("CHROMIUM_PATH", missing_path.as_os_str());

        let ctx = test_ctx();
        let result = ReportRuntimeHealthExecutor
            .execute(&tool_input(), &ctx)
            .await
            .expect("exec");
        let json = parse_result(&result);
        let chromium = renderer(&json, "chromium");

        assert_eq!(chromium["status"], "error");
        assert_eq!(chromium["configured"], true);
        assert!(
            chromium["message"]
                .as_str()
                .expect("message")
                .contains("CHROMIUM_PATH is set but the path does not exist"),
            "unexpected message: {}",
            chromium["message"]
        );
    }

    #[test]
    fn aggregate_status_distinguishes_required_from_configured_failures() {
        let optional_missing = vec![
            RendererHealth::ok("typst", true, true, "ok"),
            RendererHealth::missing("latex", false, false, "optional missing"),
        ];
        assert_eq!(aggregate_status(&optional_missing), "healthy");

        let enabled_broken = vec![
            RendererHealth::ok("typst", true, true, "ok"),
            RendererHealth::error("chromium", false, true, "enabled but broken"),
        ];
        assert_eq!(aggregate_status(&enabled_broken), "degraded");

        let required_broken = vec![
            RendererHealth::error("pandoc", true, true, "required broken"),
            RendererHealth::ok("typst", true, true, "ok"),
        ];
        assert_eq!(aggregate_status(&required_broken), "unhealthy");
    }
}
