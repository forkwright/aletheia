//! aletheia-oikonomos — per-nous background task runner
//!
//! Oikonomos (οἰκονόμος) — "the steward." The quiet persistent presence that
//! keeps things running in the background. Manages scheduled tasks, periodic
//! attention checks (prosoche), and maintenance cycles for each nous.

pub mod bridge;
pub mod error;
pub mod maintenance;
pub mod prosoche;
pub mod runner;
pub mod schedule;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    assert_impl_all!(super::runner::TaskRunner: Send);
    assert_impl_all!(super::prosoche::ProsocheCheck: Send, Sync);
    assert_impl_all!(super::schedule::TaskDef: Send, Sync);
    assert_impl_all!(super::maintenance::TraceRotator: Send, Sync);
    assert_impl_all!(super::maintenance::DriftDetector: Send, Sync);
    assert_impl_all!(super::maintenance::DbMonitor: Send, Sync);
}
