//! Built-in turn hooks for behavior correction.

mod audit_logging;
pub(crate) mod correction;
mod cost_control;
mod scope_enforcement;

pub(crate) use audit_logging::AuditLoggingHook;
pub(crate) use correction::{CorrectionDetector, CorrectionInjector};
pub(crate) use cost_control::CostControlHook;
pub(crate) use scope_enforcement::ScopeEnforcementHook;

use std::path::Path;

use super::registry::HookRegistry;
use crate::config::HookConfig;

/// Priority constants for built-in hooks.
///
/// WHY: Cost control runs first (10) because budget exhaustion should
/// prevent any further processing. Scope enforcement (20) runs next
/// to block disallowed tools before execution. Correction injector (30)
/// runs after scope enforcement so corrections cannot bypass scope rules.
/// Correction detector (90) runs late in `on_turn_complete` since it only
/// logs. Audit logging (100) runs last to capture the final state of the turn.
pub(crate) const COST_CONTROL_PRIORITY: i32 = 10;
pub(crate) const SCOPE_ENFORCEMENT_PRIORITY: i32 = 20;
pub(crate) const CORRECTION_INJECTOR_PRIORITY: i32 = 30;
pub(crate) const CORRECTION_DETECTOR_PRIORITY: i32 = 90;
pub(crate) const AUDIT_LOGGING_PRIORITY: i32 = 100;

/// Register all enabled built-in hooks into the given registry.
///
/// `workspace` is the agent's workspace directory (e.g. `oikos.nous_dir(id)`).
/// Required for hooks that persist state to the filesystem.
pub(crate) fn register_builtin_hooks(
    registry: &mut HookRegistry,
    config: &HookConfig,
    workspace: &Path,
) {
    if config.cost_control_enabled {
        registry.register(
            COST_CONTROL_PRIORITY,
            Box::new(CostControlHook::new(config.turn_token_budget)),
        );
    }

    if config.scope_enforcement_enabled {
        registry.register(SCOPE_ENFORCEMENT_PRIORITY, Box::new(ScopeEnforcementHook));
    }

    if config.correction_hooks_enabled {
        registry.register(
            CORRECTION_INJECTOR_PRIORITY,
            Box::new(CorrectionInjector::new(workspace.to_path_buf())),
        );
        registry.register(
            CORRECTION_DETECTOR_PRIORITY,
            Box::new(CorrectionDetector::new(workspace.to_path_buf())),
        );
    }

    if config.audit_logging_enabled {
        registry.register(AUDIT_LOGGING_PRIORITY, Box::new(AuditLoggingHook));
    }
}
