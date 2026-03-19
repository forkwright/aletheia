//! Test support: mock executors and a component spec validation harness.
//!
//! Enabled by the `test-support` Cargo feature. Never compiled into release binaries.
//!
//! # Usage
//!
//! ```ignore
//! use aletheia_organon::testing::{MockToolExecutor, ToolExecutorSpec, make_test_context};
//!
//! let executor = MockToolExecutor::text("hello");
//! let ctx = make_test_context();
//! let spec = ToolExecutorSpec::new(executor.name());
//! let report = spec.validate_async(&executor, &ctx).await;
//! assert!(report.is_passing(), "{:?}", report.failures());
//! ```

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use aletheia_koina::id::{NousId, SessionId, ToolName};

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{ToolContext, ToolInput, ToolResult};

// ── Mock executor ────────────────────────────────────────────────────────────

/// Configurable mock [`ToolExecutor`] for use in tests.
///
/// Implements the same [`ToolExecutor`] trait as production executors.
/// Supports fixed text responses, error injection, and call-count tracking.
///
/// # Examples
///
/// ```ignore
/// let ex = MockToolExecutor::text("ok");
/// let result = ex.execute(&input, &ctx).await.unwrap();
/// assert!(!result.is_error);
/// assert_eq!(ex.call_count(), 1);
/// ```
#[allow(clippy::module_name_repetitions)]
pub struct MockToolExecutor {
    name: ToolName,
    // WHY: std::sync::Mutex — lock never held across .await
    inner: Mutex<MockInner>,
    call_count: AtomicU64,
}

struct MockInner {
    mode: MockMode,
}

enum MockMode {
    Text(String),
    Error(String),
    Sequence(Vec<ToolResult>),
}

impl MockToolExecutor {
    /// Create a mock that always returns the given text as a success result.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "test-support: 'mock' is a known-valid tool name"
    )]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            name: ToolName::new("mock").expect("valid tool name"),
            inner: Mutex::new(MockInner {
                mode: MockMode::Text(text.into()),
            }),
            call_count: AtomicU64::new(0),
        }
    }

    /// Create a mock that returns an error result (not a Rust `Err`).
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "test-support: 'mock' is a known-valid tool name"
    )]
    pub fn tool_error(message: impl Into<String>) -> Self {
        Self {
            name: ToolName::new("mock").expect("valid tool name"),
            inner: Mutex::new(MockInner {
                mode: MockMode::Error(message.into()),
            }),
            call_count: AtomicU64::new(0),
        }
    }

    /// Create a mock that returns results from a sequence (repeats last when exhausted).
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "test-support: 'mock' is a known-valid tool name"
    )]
    pub fn sequence(results: Vec<ToolResult>) -> Self {
        Self {
            name: ToolName::new("mock").expect("valid tool name"),
            inner: Mutex::new(MockInner {
                mode: MockMode::Sequence(results),
            }),
            call_count: AtomicU64::new(0),
        }
    }

    /// Override the tool name reported to the executor.
    #[must_use]
    pub fn named(mut self, name: ToolName) -> Self {
        self.name = name;
        self
    }

    /// The tool name this mock is registered under.
    #[must_use]
    pub fn name(&self) -> ToolName {
        self.name.clone()
    }

    /// Number of times `execute` has been called.
    #[must_use]
    pub fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl ToolExecutor for MockToolExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let result = {
            #[expect(
                clippy::expect_used,
                reason = "test-support: mock mutex is never poisoned in tests"
            )]
            let mut inner = self.inner.lock().expect("mock mutex poisoned");
            match &mut inner.mode {
                MockMode::Text(t) => Ok(ToolResult::text(t.clone())),
                MockMode::Error(e) => Ok(ToolResult::error(e.clone())),
                MockMode::Sequence(seq) => {
                    if seq.len() > 1 {
                        Ok(seq.remove(0))
                    } else {
                        Ok(seq
                            .first()
                            .cloned()
                            .unwrap_or_else(|| ToolResult::text("(empty sequence)")))
                    }
                }
            }
        };
        Box::pin(async move { result })
    }
}

// ── Component spec validation ─────────────────────────────────────────────────

/// Spec contract that any [`ToolExecutor`] implementation must satisfy.
///
/// Use [`ToolExecutorSpec::validate_async`] inside a `#[tokio::test]` to assert
/// the contract. The report separates passed checks from failed ones so test
/// output is easy to diagnose.
pub struct ToolExecutorSpec {
    tool_name: ToolName,
}

impl ToolExecutorSpec {
    /// Declare a spec for the executor registered under `tool_name`.
    #[must_use]
    pub fn new(tool_name: ToolName) -> Self {
        Self { tool_name }
    }

    /// Run all spec checks against `executor` in the given context.
    ///
    /// Returns a [`SpecReport`] containing the names of all passed and failed checks.
    pub async fn validate_async<E: ToolExecutor>(
        &self,
        executor: &E,
        ctx: &ToolContext,
    ) -> SpecReport {
        let mut report = SpecReport::default();
        let input = make_tool_input(&self.tool_name);

        // Check 1: valid input returns Ok
        match executor.execute(&input, ctx).await {
            Ok(_) => report.pass("valid-input-returns-ok"),
            Err(e) => report.fail(
                "valid-input-returns-ok",
                &format!("executor returned Err: {e}"),
            ),
        }

        // Check 2: success result is not marked as error
        if let Ok(result) = executor.execute(&input, ctx).await {
            if result.is_error {
                report.fail(
                    "success-result-not-marked-error",
                    "ToolResult::is_error is true for a successful execution",
                );
            } else {
                report.pass("success-result-not-marked-error");
            }
        }

        // Check 3: executor is callable multiple times (not single-use)
        let first = executor.execute(&input, ctx).await;
        let second = executor.execute(&input, ctx).await;
        if first.is_ok() && second.is_ok() {
            report.pass("executor-is-reusable");
        } else {
            report.fail(
                "executor-is-reusable",
                "second call failed after first succeeded",
            );
        }

        // Check 4: returned content is non-empty for success results
        if let Ok(result) = executor.execute(&input, ctx).await
            && !result.is_error
        {
            use crate::types::ToolResultContent;
            let non_empty = match &result.content {
                ToolResultContent::Text(t) => !t.is_empty(),
                ToolResultContent::Blocks(b) => !b.is_empty(),
                // ToolResultContent is #[non_exhaustive]: treat unknown variants as non-empty
                _ => true,
            };
            if non_empty {
                report.pass("success-result-has-content");
            } else {
                report.fail(
                    "success-result-has-content",
                    "success result content is empty",
                );
            }
        }

        report
    }
}

/// Outcome of running a [`ToolExecutorSpec`] validation.
#[derive(Default, Debug)]
pub struct SpecReport {
    passed: Vec<String>,
    failed: Vec<(String, String)>,
}

impl SpecReport {
    fn pass(&mut self, check: &str) {
        self.passed.push(check.to_owned());
    }

    fn fail(&mut self, check: &str, reason: &str) {
        self.failed.push((check.to_owned(), reason.to_owned()));
    }

    /// `true` if all checks passed (no failures).
    #[must_use]
    pub fn is_passing(&self) -> bool {
        self.failed.is_empty()
    }

    /// Names of checks that passed.
    #[must_use]
    pub fn passes(&self) -> &[String] {
        &self.passed
    }

    /// Checks that failed, paired with a diagnostic reason.
    #[must_use]
    pub fn failures(&self) -> &[(String, String)] {
        &self.failed
    }
}

// ── Test context helpers ──────────────────────────────────────────────────────

/// Build a minimal [`ToolContext`] for use in tests.
///
/// Uses synthetic identities (`alice`, a fresh `SessionId`) and a
/// `tempdir`-friendly workspace path. No runtime services are attached.
#[must_use]
#[expect(
    clippy::expect_used,
    reason = "test-support: 'alice' is a known-valid NousId in synthetic test data"
)]
pub fn make_test_context() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("alice").expect("valid nous id"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/aletheia-test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(std::collections::HashSet::new())),
    }
}

/// Build a [`ToolInput`] for the given tool name with an empty arguments object.
#[must_use]
pub fn make_tool_input(name: &ToolName) -> ToolInput {
    ToolInput {
        name: name.clone(),
        tool_use_id: "tu_test_00000".to_owned(),
        arguments: serde_json::json!({}),
    }
}

// ── Compile-time trait check ──────────────────────────────────────────────────

const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn check() {
        assert_send_sync::<MockToolExecutor>();
    }
    let _ = check;
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_executor_text_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockToolExecutor>();
    }

    #[test]
    fn spec_report_passes_when_empty_failures() {
        let report = SpecReport::default();
        assert!(
            report.is_passing(),
            "fresh report with no failures must pass"
        );
    }

    #[test]
    fn spec_report_fails_when_failure_added() {
        let mut report = SpecReport::default();
        report.fail("some-check", "something went wrong");
        assert!(!report.is_passing(), "report with a failure must not pass");
        assert_eq!(
            report.failures().len(),
            1,
            "exactly one failure should be recorded"
        );
    }

    #[test]
    fn make_tool_input_uses_given_name() {
        #[expect(clippy::expect_used, reason = "test: known-valid tool name")]
        let name = ToolName::new("my_tool").expect("valid name");
        let input = make_tool_input(&name);
        assert_eq!(
            input.name.as_str(),
            "my_tool",
            "input name must match the supplied ToolName"
        );
    }
}
