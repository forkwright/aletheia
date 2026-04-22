//! Tests for the new hook points: after_tool, session_start, before_compact, after_compact.

use super::*;

// -- Test hook implementations for new hook points --

struct MinimalHookForAfterTool;

impl TurnHook for MinimalHookForAfterTool {
    fn name(&self) -> &'static str {
        "minimal_after_tool"
    }
}

struct MinimalHookForSessionStart;

impl TurnHook for MinimalHookForSessionStart {
    fn name(&self) -> &'static str {
        "minimal_session_start"
    }
}

struct MinimalHookForBeforeCompact;

impl TurnHook for MinimalHookForBeforeCompact {
    fn name(&self) -> &'static str {
        "minimal_before_compact"
    }
}

struct MinimalHookForAfterCompact;

impl TurnHook for MinimalHookForAfterCompact {
    fn name(&self) -> &'static str {
        "minimal_after_compact"
    }
}

struct CountingAfterToolHook {
    count: Arc<AtomicU32>,
}

impl TurnHook for CountingAfterToolHook {
    fn name(&self) -> &'static str {
        "counting_after_tool"
    }

    fn after_tool<'a>(
        &'a self,
        _context: &'a AfterToolContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        let c = Arc::clone(&self.count);
        Box::pin(async move {
            c.fetch_add(1, Ordering::Relaxed);
            HookResult::Continue
        })
    }
}

struct AbortingSessionHook;

impl TurnHook for AbortingSessionHook {
    fn name(&self) -> &'static str {
        "aborting_session"
    }

    fn session_start<'a>(
        &'a self,
        _context: &'a SessionStartContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Abort {
            reason: "session initialization failed".to_owned(),
        }))
    }
}

struct CountingBeforeCompactHook {
    count: Arc<AtomicU32>,
}

impl TurnHook for CountingBeforeCompactHook {
    fn name(&self) -> &'static str {
        "counting_before_compact"
    }

    fn before_compact<'a>(
        &'a self,
        _context: &'a CompactionContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        let c = Arc::clone(&self.count);
        Box::pin(async move {
            c.fetch_add(1, Ordering::Relaxed);
            HookResult::Continue
        })
    }
}

struct Hook1AfterCompact {
    called: Arc<AtomicU32>,
}

impl TurnHook for Hook1AfterCompact {
    fn name(&self) -> &'static str {
        "hook1_after_compact"
    }

    fn after_compact<'a>(
        &'a self,
        _context: &'a CompactionContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        let c = Arc::clone(&self.called);
        Box::pin(async move {
            c.fetch_add(1, Ordering::Relaxed);
            HookResult::Abort {
                reason: "abort requested".to_owned(),
            }
        })
    }
}

struct Hook2AfterCompact {
    called: Arc<AtomicU32>,
}

impl TurnHook for Hook2AfterCompact {
    fn name(&self) -> &'static str {
        "hook2_after_compact"
    }

    fn after_compact<'a>(
        &'a self,
        _context: &'a CompactionContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        let c = Arc::clone(&self.called);
        Box::pin(async move {
            c.fetch_add(1, Ordering::Relaxed);
            HookResult::Continue
        })
    }
}

// -- Tests --

#[test]
fn default_after_tool_returns_continue() {
    let hook = MinimalHookForAfterTool;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let input = serde_json::json!({});
    let ctx = test_after_tool_context(&input);
    let result = rt.block_on(hook.after_tool(&ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default after_tool should return Continue"
    );
}

#[test]
fn default_session_start_returns_continue() {
    let hook = MinimalHookForSessionStart;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let ctx = test_session_start_context();
    let result = rt.block_on(hook.session_start(&ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default session_start should return Continue"
    );
}

#[test]
fn default_before_compact_returns_continue() {
    let hook = MinimalHookForBeforeCompact;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let ctx = test_compaction_context();
    let result = rt.block_on(hook.before_compact(&ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default before_compact should return Continue"
    );
}

#[test]
fn default_after_compact_returns_continue() {
    let hook = MinimalHookForAfterCompact;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let ctx = test_compaction_context();
    let result = rt.block_on(hook.after_compact(&ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default after_compact should return Continue"
    );
}

#[test]
fn after_tool_hook_fires_for_each_tool() {
    let count = Arc::new(AtomicU32::new(0));
    let count_clone = Arc::clone(&count);

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");

    let mut registry = HookRegistry::new();
    registry.register(0, Box::new(CountingAfterToolHook { count: count_clone }));

    let input = serde_json::json!({});
    let ctx1 = test_after_tool_context(&input);
    rt.block_on(registry.run_after_tool(&ctx1));
    assert_eq!(count.load(Ordering::Relaxed), 1);

    let ctx2 = test_after_tool_context(&input);
    rt.block_on(registry.run_after_tool(&ctx2));
    assert_eq!(count.load(Ordering::Relaxed), 2);
}

#[test]
fn session_start_hook_can_abort() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");

    let mut registry = HookRegistry::new();
    registry.register(0, Box::new(AbortingSessionHook));

    let ctx = test_session_start_context();
    let result = rt.block_on(registry.run_session_start(&ctx));
    match result {
        HookResult::Abort { reason } => {
            assert_eq!(reason, "session initialization failed");
        }
        _ => panic!("expected Abort"),
    }
}

#[test]
fn before_compact_hook_fires_before_distillation() {
    let count = Arc::new(AtomicU32::new(0));
    let count_clone = Arc::clone(&count);

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");

    let mut registry = HookRegistry::new();
    registry.register(
        0,
        Box::new(CountingBeforeCompactHook { count: count_clone }),
    );

    assert_eq!(count.load(Ordering::Relaxed), 0);
    let ctx = test_compaction_context();
    rt.block_on(registry.run_before_compact(&ctx));
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn after_compact_hook_does_not_short_circuit() {
    let hook1_called = Arc::new(AtomicU32::new(0));
    let hook2_called = Arc::new(AtomicU32::new(0));

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");

    let mut registry = HookRegistry::new();
    registry.register(
        0,
        Box::new(Hook1AfterCompact {
            called: Arc::clone(&hook1_called),
        }),
    );
    registry.register(
        1,
        Box::new(Hook2AfterCompact {
            called: Arc::clone(&hook2_called),
        }),
    );

    let ctx = test_compaction_context();
    rt.block_on(registry.run_after_compact(&ctx));

    // Both hooks should fire even though Hook1 returned Abort
    assert_eq!(hook1_called.load(Ordering::Relaxed), 1);
    assert_eq!(hook2_called.load(Ordering::Relaxed), 1);
}
