//! Built-in turn hooks for behavior correction.

mod audit_logging;
mod cost_control;
mod scope_enforcement;

pub(crate) use audit_logging::AuditLoggingHook;
pub(crate) use cost_control::CostControlHook;
pub(crate) use scope_enforcement::ScopeEnforcementHook;

use super::registry::HookRegistry;
use crate::config::HookConfig;

/// Priority constants for built-in hooks.
///
/// WHY: Cost control runs first (10) because budget exhaustion should
/// prevent any further processing. Scope enforcement (20) runs next
/// to block disallowed tools before execution. Audit logging (100)
/// runs last to capture the final state of the turn.
pub(crate) const COST_CONTROL_PRIORITY: i32 = 10;
pub(crate) const SCOPE_ENFORCEMENT_PRIORITY: i32 = 20;
pub(crate) const AUDIT_LOGGING_PRIORITY: i32 = 100;

/// Register all enabled built-in hooks into the given registry.
pub(crate) fn register_builtin_hooks(registry: &mut HookRegistry, config: &HookConfig) {
    if config.cost_control_enabled {
        registry.register(
            COST_CONTROL_PRIORITY,
            Box::new(CostControlHook::new(config.turn_token_budget)),
        );
    }

    if config.scope_enforcement_enabled {
        registry.register(SCOPE_ENFORCEMENT_PRIORITY, Box::new(ScopeEnforcementHook));
    }

    if config.audit_logging_enabled {
        registry.register(AUDIT_LOGGING_PRIORITY, Box::new(AuditLoggingHook));
    }
}
