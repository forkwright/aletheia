//! Nous actor/manager behavior configuration.

use serde::{Deserialize, Serialize};

/// Nous actor/manager health, restart, GC, and loop-detection thresholds.
///
/// All defaults match the current hardcoded constants in the `nous` crate so
/// that omitting this section from `aletheia.toml` produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct NousBehaviorConfig {
    /// Panics within the window that trigger degraded mode. Default: 5.
    /// Mirrors `nous::actor::DEGRADED_PANIC_THRESHOLD`.
    pub degraded_panic_threshold: u32,
    /// Window in seconds for counting panics toward degraded threshold. Default: 600.
    /// Mirrors `nous::actor::DEGRADED_WINDOW`.
    pub degraded_window_secs: u64,
    /// Actor inbox receive timeout in seconds before a warning is logged. Default: 30.
    /// Mirrors `nous::actor::INBOX_RECV_TIMEOUT`.
    pub inbox_recv_timeout_secs: u64,
    /// Consecutive receive timeouts before a warning log is emitted. Default: 3.
    /// Mirrors `nous::actor::CONSECUTIVE_TIMEOUT_WARN_THRESHOLD`.
    pub consecutive_timeout_warn_threshold: u32,
    /// Actor inbox channel capacity. Default: 32.
    pub inbox_capacity: usize,
    /// Maximum number of concurrently spawned tasks per agent. Default: 8.
    pub max_spawned_tasks: usize,
    /// Maximum number of concurrent sessions across all agents. Default: 1000.
    pub max_sessions: usize,
    /// Completed-task garbage collection interval in seconds. Default: 300.
    /// Mirrors `nous::tasks::gc::DEFAULT_GC_INTERVAL`.
    pub gc_interval_secs: u64,
    /// Consecutive failed pings before marking an agent dead. Default: 3.
    /// Mirrors `nous::manager::DEAD_THRESHOLD`.
    pub manager_dead_threshold: u32,
    /// Cap on exponential restart backoff in seconds. Default: 300.
    /// Mirrors `nous::manager::MAX_RESTART_BACKOFF`.
    pub manager_max_restart_backoff_secs: u64,
    /// Drain timeout in seconds before forcing an agent restart. Default: 30.
    /// Mirrors `nous::manager::RESTART_DRAIN_TIMEOUT`.
    pub manager_restart_drain_timeout_secs: u64,
    /// Window in seconds over which the failure count decays to zero. Default: 3600.
    /// Mirrors `nous::manager::RESTART_DECAY_WINDOW`.
    pub manager_restart_decay_window_secs: u64,
    /// Agent health poll interval in seconds. Default: 30.
    /// Mirrors `nous::manager::DEFAULT_HEALTH_INTERVAL`.
    pub manager_health_interval_secs: u64,
    /// Timeout in seconds for health-ping responses. Default: 5.
    /// Mirrors `nous::manager::DEFAULT_PING_TIMEOUT`.
    pub manager_ping_timeout_secs: u64,
    /// Maximum seconds a turn may be active before the health check considers
    /// the actor stuck. An `active_turn` flag alone cannot distinguish a legitimately
    /// busy actor from one hung on an infinite loop or deadlock. Default: 600 (10 min).
    /// WHY: Without a timeout, a stuck `active_turn` flag prevents the health check
    /// from ever restarting the actor, making a single hung pipeline permanently
    /// block all subsequent messages. (#3254)
    pub stuck_turn_timeout_secs: u64,
    /// Number of recent tool calls scanned for loop detection. Default: 50.
    /// Mirrors `nous::pipeline::DEFAULT_LOOP_WINDOW`.
    pub loop_detection_window: usize,
    /// Maximum sequence length examined for repeating cycles. Default: 10.
    /// Mirrors `nous::pipeline::CYCLE_DETECTION_MAX_LEN`.
    pub cycle_detection_max_len: usize,
    /// Events accumulated before self-audit runs. Default: 50.
    /// Mirrors `nous::self_audit::DEFAULT_EVENT_THRESHOLD`.
    pub self_audit_event_threshold: u32,
    /// TTL in seconds for the bootstrap workspace file cache. Default: 60.
    ///
    /// // WHY: bootstrap files (SOUL.md, USER.md, etc.) change rarely relative
    /// // to turn frequency. Caching their content and token estimates for up
    /// // to this many seconds avoids redundant disk reads per turn (#3388).
    /// // mtime-based invalidation catches operator edits immediately, so the
    /// // TTL is a backstop rather than the primary freshness mechanism.
    /// // Set to 0 to disable the cache.
    pub bootstrap_cache_ttl_secs: u64,
    /// Maximum seconds `NousManager::shutdown_all` waits for actors to finish
    /// their current turn before aborting their tasks. Default: 30.
    ///
    /// WHY: Without a timeout, a long-running turn (e.g. a stuck LLM call or
    /// deadlocked tool) blocks graceful shutdown indefinitely. When the
    /// timeout expires, remaining actor tasks are aborted via
    /// `JoinHandle::abort()` so the process can exit. (#3382)
    pub shutdown_timeout_secs: u64,
}

impl Default for NousBehaviorConfig {
    fn default() -> Self {
        Self {
            degraded_panic_threshold: 5,
            degraded_window_secs: 600,
            inbox_recv_timeout_secs: 30,
            consecutive_timeout_warn_threshold: 3,
            inbox_capacity: 32,
            max_spawned_tasks: 8,
            max_sessions: 1_000,
            gc_interval_secs: 300,
            manager_dead_threshold: 3,
            manager_max_restart_backoff_secs: 300,
            manager_restart_drain_timeout_secs: 30,
            manager_restart_decay_window_secs: 3_600,
            manager_health_interval_secs: 30,
            manager_ping_timeout_secs: 5,
            stuck_turn_timeout_secs: 600,
            loop_detection_window: 50,
            cycle_detection_max_len: 10,
            self_audit_event_threshold: 50,
            bootstrap_cache_ttl_secs: 60,
            shutdown_timeout_secs: 30,
        }
    }
}
