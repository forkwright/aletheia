//! Cancel-safety tests for the execute loop and its receipt-ledger journal (#5225).
//!
//! Each test targets one of the boundaries the issue names: before the LLM
//! call resolves, before a requested tool is dispatched, while a tool's
//! side-effecting future is in flight, and after a tool completes but
//! before the turn (and its `TurnResult`) is delivered anywhere. Dropping
//! the `execute()`/`execute_with_deadline()` future at each point must
//! never panic, and — for the tool-execution and post-completion cases —
//! the receipt ledger's journal must end up in the state the boundary
//! implies rather than silently forgetting the call happened.
use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use hermeneus::provider::LlmProvider;
use organon::receipts::ToolJournalState;

use super::*;

/// An `LlmProvider` whose `complete()` future never resolves — models the
/// window between calling the LLM and its response arriving.
struct PendingProvider;

impl LlmProvider for PendingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        Box::pin(pending())
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    fn name(&self) -> &str {
        "pending"
    }
}

/// An `LlmProvider` that answers its first call with `first`, then hangs
/// forever on every subsequent call — models a turn that dispatched a tool
/// and is now waiting on the next LLM round-trip when it gets cancelled.
struct AnswerOnceThenPendingProvider {
    first: std::sync::Mutex<Option<CompletionResponse>>,
}

impl AnswerOnceThenPendingProvider {
    fn new(first: CompletionResponse) -> Self {
        Self {
            first: std::sync::Mutex::new(Some(first)),
        }
    }
}

impl LlmProvider for AnswerOnceThenPendingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        let taken = self
            .first
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match taken {
            Some(response) => Box::pin(async move { Ok(response) }),
            None => Box::pin(pending()),
        }
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    fn name(&self) -> &str {
        "answer-once-then-pending"
    }
}

/// A `ToolExecutor` whose side-effecting future sleeps far longer than the
/// timeouts these tests use, so it is reliably still in flight when the
/// outer future is dropped.
struct SlowExecutor {
    started: Arc<AtomicUsize>,
}

impl ToolExecutor for SlowExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        self.started.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(ToolResult::text("should never observe this"))
        })
    }
}

// WHY 100ms rather than the tighter margin that would suffice on an idle
// machine: every "cancelled" path here races a `tokio::time::timeout`
// against work that never involves real I/O (mock providers, an in-memory
// sleep), so the work side finishes in microseconds under normal load —
// the only failure mode this budget guards against is a CI runner so
// starved that scheduling a single poll takes tens of milliseconds.
const SHORT_TIMEOUT: Duration = Duration::from_millis(100);

/// Cancellation before the LLM call ever resolves: dropping the turn future
/// here has no journal or session-state consequence to reconcile — there is
/// no tool call yet — so the only thing under test is that it does not panic
/// and does not hang past the timeout.
#[tokio::test]
async fn cancel_before_llm_response_does_not_panic() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(PendingProvider));

    let mut config = test_config();
    config.generation.model = "test-model".to_owned();
    let session = test_session();

    let outcome = tokio::time::timeout(
        SHORT_TIMEOUT,
        execute(
            &test_pipeline_ctx(),
            &session,
            &config,
            &providers,
            &ToolRegistry::new(),
            &test_tool_ctx(),
            None,
        ),
    )
    .await;

    assert!(
        outcome.is_err(),
        "the pending provider must still be in flight when the timeout fires"
    );
}

/// Cancellation between the LLM's tool-use response and dispatch, under the
/// default `CancelBeforeToolStart` policy: a disconnected `stream_tx` must
/// stop the turn before the tool the model just requested ever runs.
#[tokio::test]
async fn cancel_before_tool_start_never_dispatches() {
    let executions = Arc::new(AtomicUsize::new(0));
    let tools = make_registry_with(
        "echo",
        Box::new(CountingExecutor::new(Arc::clone(&executions))),
    );

    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_tool_response(
            "echo",
            "toolu_cancel_1",
            serde_json::json!({}),
        )])
        .models(&["test-model"]),
    ));

    let mut config = test_config();
    config.generation.model = "test-model".to_owned();
    // WHY explicit: the test asserts the pre-existing default behavior by
    // name rather than by omission, so a future change to the default does
    // not silently change what this test verifies.
    config.limits.client_disconnect_policy =
        crate::config::ClientDisconnectPolicy::CancelBeforeToolStart;
    let session = test_session();

    // WHY drop the receiver immediately: `mpsc::Sender::is_closed()` becomes
    // true as soon as there is no receiver, with no send required — this is
    // exactly the client-disconnect signal the execute loop already checks.
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    drop(rx);

    let result = execute_with_deadline(
        ExecuteRequest {
            ctx: &test_pipeline_ctx(),
            session: &session,
            config: &config,
            providers: &providers,
            tools: &tools,
            tool_ctx: &test_tool_ctx(),
        },
        ExecuteAdapters {
            stream_tx: Some(&tx),
            ..Default::default()
        },
    )
    .await
    .expect("execute completes even though the client disconnected");

    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "CancelBeforeToolStart must never dispatch a tool requested after disconnect"
    );
    assert_eq!(result.stop_reason, "client_disconnect");
}

/// Cancellation while a tool's side-effecting future is in flight: the
/// journal entry the executor's `Started` write left behind must survive
/// the drop (it lives on `SessionState::receipt_ledger`, not on the
/// dropped future), and the very next turn run against the same session
/// must reconcile it as `Interrupted` rather than losing it.
#[tokio::test]
async fn cancel_during_tool_execution_is_reconciled_next_turn() {
    let started = Arc::new(AtomicUsize::new(0));
    let tools = make_registry_with(
        "slow",
        Box::new(SlowExecutor {
            started: Arc::clone(&started),
        }),
    );

    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_tool_response(
            "slow",
            "toolu_slow_1",
            serde_json::json!({}),
        )])
        .models(&["test-model"]),
    ));

    let mut config = test_config();
    config.generation.model = "test-model".to_owned();
    let session = test_session();

    let outcome = tokio::time::timeout(
        SHORT_TIMEOUT,
        execute(
            &test_pipeline_ctx(),
            &session,
            &config,
            &providers,
            &tools,
            &test_tool_ctx(),
            None,
        ),
    )
    .await;

    assert!(
        outcome.is_err(),
        "the slow executor's 1-hour sleep must still be in flight when the timeout fires"
    );
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "the tool's side-effecting future must have actually started"
    );
    assert_eq!(
        session
            .receipt_ledger
            .lock()
            .expect("ledger lock")
            .journal_state("toolu_slow_1"),
        Some(ToolJournalState::Started),
        "a call cut short mid-execution stays Started until the next turn reconciles it — \
         this ledger must not silently forget the side effect was attempted"
    );

    // WHY reuse `session`: reconciliation is keyed to the receipt ledger
    // living on `SessionState`, which the actor keeps across turns for as
    // long as it runs — the same object that survived the cancelled future
    // above is what the next turn's `reconcile_interrupted` call reads.
    let mut providers2 = ProviderRegistry::new();
    providers2.register(Box::new(
        MockProvider::with_responses(vec![make_text_response("no more tools needed")])
            .models(&["test-model"]),
    ));

    let result = execute(
        &test_pipeline_ctx(),
        &session,
        &config,
        &providers2,
        &ToolRegistry::new(),
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("second turn against the same session completes");

    assert_eq!(
        session
            .receipt_ledger
            .lock()
            .expect("ledger lock")
            .journal_state("toolu_slow_1"),
        Some(ToolJournalState::Interrupted),
        "the next turn must reconcile the abandoned call to Interrupted"
    );
    let reconciled = result
        .tool_calls
        .iter()
        .find(|call| call.id == "toolu_slow_1")
        .expect("the reconciled call must be folded into this turn's durable tool_calls record");
    assert!(reconciled.is_error);
    assert!(
        reconciled
            .result
            .as_deref()
            .is_some_and(|text| text.contains("interrupted")),
        "the persisted record must say the outcome is unknown, not report false success"
    );
}

/// Cancellation after a tool has fully completed but before the turn (and
/// its `TurnResult`) is ever delivered: the journal must already read
/// `Completed`, and a later reconciliation pass must never regress a
/// completed call back to `Interrupted` — only entries still `Started` are
/// ever touched.
#[tokio::test]
async fn cancel_after_tool_completion_keeps_completed_state() {
    let executions = Arc::new(AtomicUsize::new(0));
    let tools = make_registry_with(
        "echo",
        Box::new(CountingExecutor::new(Arc::clone(&executions))),
    );

    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(AnswerOnceThenPendingProvider::new(
        make_tool_response("echo", "toolu_done_1", serde_json::json!({})),
    )));

    let mut config = test_config();
    config.generation.model = "test-model".to_owned();
    let session = test_session();

    let outcome = tokio::time::timeout(
        SHORT_TIMEOUT,
        execute(
            &test_pipeline_ctx(),
            &session,
            &config,
            &providers,
            &tools,
            &test_tool_ctx(),
            None,
        ),
    )
    .await;

    assert!(
        outcome.is_err(),
        "the second LLM call must still be pending when the timeout fires"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "the tool itself must have run to completion before the turn was cut short"
    );
    assert_eq!(
        session
            .receipt_ledger
            .lock()
            .expect("ledger lock")
            .journal_state("toolu_done_1"),
        Some(ToolJournalState::Completed),
        "a call that finished before cancellation must read Completed, not Interrupted"
    );

    assert!(
        session
            .receipt_ledger
            .lock()
            .expect("ledger lock")
            .reconcile_interrupted(jiff::Timestamp::now())
            .is_empty(),
        "reconciliation must never regress an already-Completed call"
    );
}
