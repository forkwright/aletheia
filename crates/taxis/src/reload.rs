//! Hot-reload classification: which settings need a restart vs live update.

/// Field path prefixes that require a process restart to take effect.
const RESTART_PREFIXES: &[&str] = &[
    "gateway.port",
    "gateway.bind",
    "gateway.tls",
    "gateway.auth.mode",
    "gateway.csrf",
    "gateway.bodyLimit",
    "channels",
];

/// Returns true if changing the given dotted field path requires a restart.
#[must_use]
pub fn requires_restart(field_path: &str) -> bool {
    RESTART_PREFIXES
        .iter()
        .any(|prefix| field_path.starts_with(prefix))
}

/// Returns the list of field path prefixes that require restart.
#[must_use]
pub fn restart_prefixes() -> &'static [&'static str] {
    RESTART_PREFIXES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_port_requires_restart() {
        assert!(requires_restart("gateway.port"));
    }

    #[test]
    fn tls_settings_require_restart() {
        assert!(requires_restart("gateway.tls.enabled"));
        assert!(requires_restart("gateway.tls.certPath"));
    }

    #[test]
    fn channel_settings_require_restart() {
        assert!(requires_restart("channels.signal.enabled"));
    }

    #[test]
    fn agent_defaults_hot_reloadable() {
        assert!(!requires_restart("agents.defaults.timeoutSeconds"));
        assert!(!requires_restart("agents.defaults.maxToolIterations"));
        assert!(!requires_restart("agents.defaults.thinkingBudget"));
    }

    #[test]
    fn maintenance_hot_reloadable() {
        assert!(!requires_restart("maintenance.traceRotation.maxAgeDays"));
        assert!(!requires_restart(
            "maintenance.dbMonitoring.warnThresholdMb"
        ));
    }

    #[test]
    fn embedding_hot_reloadable() {
        assert!(!requires_restart("embedding.provider"));
    }
}
