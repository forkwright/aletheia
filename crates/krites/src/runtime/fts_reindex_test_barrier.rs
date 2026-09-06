//! Test-only rendezvous point for the FTS reindex batch loop (#6987).
//!
//! WHY: a test that kills a running FTS reindex from another thread cannot
//! reliably catch it in flight by polling -- a reindex over a few hundred
//! small rows can finish before a polling test thread manages to issue
//! `::kill`, so the test observes a *successful* outcome for a build it
//! killed (a race, not a logic bug). Arming this barrier makes the reindex
//! block at the first checkpoint of its batch loop until the test has
//! observed the kill land, so cancellation is deterministic under repetition
//! instead of a race against how fast the batch loop runs.
//!
//! This module is `#[cfg(test)]`-gated in `runtime::mod` and compiles only
//! for this crate's own test binary, never for a consumer's build.

use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender};

use crate::error::InternalResult as Result;
use crate::runtime::db::Poison;

thread_local! {
    /// Set by [`arm`] on the thread that will run the FTS build, consumed by
    /// the first [`wait`] call on that same thread. `None` (the default) makes
    /// `wait` a no-op, so every test that does not call `arm` is unaffected.
    static BARRIER: RefCell<Option<(Sender<()>, Receiver<()>)>> = const { RefCell::new(None) };
}

/// Arm the barrier for the *current* thread: `wait` will signal `reached`
/// then block on `proceed` before returning. Must be called on the thread
/// that runs the FTS build (inside the spawned closure, before invoking it),
/// since thread-local state does not cross threads.
pub(crate) fn arm(reached: Sender<()>, proceed: Receiver<()>) {
    BARRIER.with(|cell| *cell.borrow_mut() = Some((reached, proceed)));
}

/// If this thread armed the barrier, signal that the build has reached it and
/// block until the test signals `proceed`, then report the poison's state --
/// by construction the test only sends `proceed` after issuing `::kill`, so
/// this returns the kill error rather than racing the batch loop's own pace.
/// A no-op, disarming itself, on a thread that never armed the barrier.
#[expect(
    clippy::result_large_err,
    reason = "mirrors check_poison_at_row's own signature -- the engine's error size is not a test-hook concern"
)]
pub(crate) fn wait(poison: Option<&Poison>) -> Result<()> {
    let Some((reached, proceed)) = BARRIER.with(|cell| cell.borrow_mut().take()) else {
        return Ok(());
    };
    // WHY: the receiving end (the test) may already be gone if it timed out
    // and panicked -- a send/recv error here just means "don't block",
    // leaving the ordinary per-row poison check to decide the outcome.
    let _ = reached.send(());
    let _ = proceed.recv();
    match poison {
        Some(poison) => poison.check(),
        None => Ok(()),
    }
}
