/// Register every metrics-emitting crate's families with the shared registry.
///
/// WHY: `prometheus-client` has no process-wide global registry, so each
/// crate exposes a `register(&mut Registry)` function that installs its
/// metric families. This binary is the only assembly point that imports
/// them all, so wiring lives here (not in pylon, which doesn't depend on
/// every metrics-emitting crate).
pub(super) fn register_all_metrics(registry: &koina::metrics::MetricsRegistry) {
    registry.with_registry(|r| {
        agora::metrics::register(r);
        dianoia::metrics::register(r);
        mneme::metrics::register_knowledge(r);
        mneme::metrics::register_sessions(r);
        hermeneus::metrics::register(r);
        melete::metrics::register(r);
        nous::metrics::register(r);
        oikonomos::metrics::register(r);
        organon::metrics::register(r);
        pylon::metrics::register(r);
        symbolon::metrics::register(r);
        #[cfg(feature = "energeia")]
        energeia::metrics::prometheus::register(r);
    });
}

#[derive(Debug)]
pub(super) struct RuntimeBackupMetricsRecorder;

impl oikonomos::maintenance::BackupMetricsRecorder for RuntimeBackupMetricsRecorder {
    fn record_backup_duration(&self, duration_secs: f64, success: bool) {
        mneme::metrics::record_backup_duration(duration_secs, success);
    }
}

pub(super) fn task_state_component(agent_id: &str) -> String {
    agent_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
