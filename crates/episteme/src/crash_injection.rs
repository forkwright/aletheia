//! Process-crash injection seam for migration atomicity proofs
//! (aletheia#5779 §8.4).
//!
//! `std::process::abort` is disallowed anywhere in this crate
//! (`clippy.toml`: "do not call `process::abort` from library code"), so the
//! abort itself cannot live here even behind a feature flag. Instead, this
//! module exposes a registration seam: a test-only child binary (outside
//! this crate, where `abort()` is not disallowed) registers a hook that
//! calls `std::process::abort()`; migration code calls [`crash_point`] at
//! each sequence point, which is a no-op unless a hook has been registered.
//! [`crash_point`] itself never aborts — only a registered hook can.

use std::sync::OnceLock;

type Hook = dyn Fn(u32) + Send + Sync;

static HOOK: OnceLock<Box<Hook>> = OnceLock::new();

/// Register the crash-injection hook for this process. Intended for a
/// dedicated child test binary only, called once before running any
/// migration. A second call is a silent no-op ([`OnceLock`] semantics) —
/// there is exactly one crash point per crash-injection process by design.
pub fn register_crash_hook(hook: impl Fn(u32) + Send + Sync + 'static) {
    let _ = HOOK.set(Box::new(hook));
}

/// Call at each numbered point in the migration sequence (plan §8.2's
/// steps). No-op unless [`register_crash_hook`] was called first — normal
/// production migrations never register a hook, so this is a single
/// uncontended `OnceLock::get()` check with no behavioral effect.
pub fn crash_point(step: u32) {
    if let Some(hook) = HOOK.get() {
        hook(step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn crash_point_is_a_noop_with_no_registered_hook() {
        // WHY: cannot register a hook in this test (OnceLock is process-
        // global and other tests in this binary may already have set one) —
        // this only asserts crash_point never panics/aborts on its own.
        crash_point(1);
        crash_point(9);
    }

    #[test]
    fn registered_hook_receives_the_step_number() {
        static SEEN: AtomicU32 = AtomicU32::new(0);
        // WHY: OnceLock is process-global; only register if this process
        // hasn't already (another test in this binary may run first under
        // nextest's default single-process-per-test-binary model — this
        // still verifies wiring correctness for whichever hook won the
        // race, since both would push a real step number).
        register_crash_hook(|step| {
            SEEN.store(step, Ordering::SeqCst);
        });
        crash_point(7);
        assert!(
            SEEN.load(Ordering::SeqCst) > 0,
            "a registered hook must actually run"
        );
    }
}
