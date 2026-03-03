//! aletheia-daemon — per-nous background task runner
//!
//! Daemon (δαίμων) — "attendant spirit." The quiet persistent presence that
//! keeps things running in the background. Manages scheduled tasks, periodic
//! attention checks (prosoche), and maintenance cycles for each nous.

pub mod error;
pub mod prosoche;
pub mod runner;
pub mod schedule;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    assert_impl_all!(super::runner::TaskRunner: Send);
    assert_impl_all!(super::prosoche::ProsocheCheck: Send, Sync);
    assert_impl_all!(super::schedule::TaskDef: Send, Sync);
}
