//! Nous actor/manager behavior configuration.

use serde::{Deserialize, Serialize};

/// Nous actor/manager health, restart, GC, and loop-detection thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct NousBehaviorConfig {
    /// Panics within the window that trigger degraded mode. Default: 5.
    pub degraded_panic_threshold: u32,
    /// Window in seconds for counting panics toward degraded threshold. Default: 600.
    pub degraded_window_secs: u64,
    /// Actor inbox receive timeout in seconds before a warning is logged. Default: 30.
    pub inbox_recv_timeout_secs: u64,
    /// Consecutive receive timeouts before a warning log is emitted. Default: 3.
    pub consecutive_timeout_warn_threshold: u32,
    /// Maximum number of concurrently spawned tasks per agent. Default: 8.
    pub max_spawned_tasks: usize,
    /// Completed-task garbage collection interval in seconds. Default: 300.
    pub gc_interval_secs: u64,
    /// Consecutive failed pings before marking an agent dead. Default: 3.
    pub manager_dead_threshold: u32,
    /// Cap on exponential restart backoff in seconds. Default: 300.
    pub manager_max_restart_backoff_secs: u64,
    /// Drain timeout in seconds before forcing an agent restart. Default: 30.
    pub manager_restart_drain_timeout_secs: u64,
    /// Window in seconds over which the failure count decays to zero. Default: 3600.
    pub manager_restart_decay_window_secs: u64,
    /// Agent health poll interval in seconds. Default: 30.
    pub manager_health_interval_secs: u64,
    /// Timeout in seconds for health-ping responses. Default: 5.
    pub manager_ping_timeout_secs: u64,
    /// Maximum seconds a turn may be active before the health check considers
    /// the actor stuck. An `active_turn` flag alone cannot distinguish a legitimately
    /// busy actor from one hung on an infinite loop or deadlock. Default: 600 (10 min).
    /// WHY: Without a timeout, a stuck `active_turn` flag prevents the health check
    /// from ever restarting the actor, making a single hung pipeline permanently
    /// block all subsequent messages. (#3254)
    pub stuck_turn_timeout_secs: u64,
    /// Number of recent tool calls scanned for loop detection. Default: 50.
    pub loop_detection_window: usize,
    /// Maximum sequence length examined for repeating cycles. Default: 10.
    pub cycle_detection_max_len: usize,
    /// Events accumulated before self-audit runs. Default: 50.
    pub self_audit_event_threshold: u32,
    /// TTL in seconds for the bootstrap workspace file cache. Default: 60.
    ///
    /// Bootstrap files (SOUL.md, USER.md, etc.) change rarely relative to
    /// turn frequency; caching their content and token estimates avoids
    /// redundant disk reads per turn. mtime-based invalidation catches
    /// operator edits immediately, so the TTL is a backstop rather than the
    /// primary freshness mechanism. Set to 0 to disable the cache.
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
            max_spawned_tasks: 8,
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
