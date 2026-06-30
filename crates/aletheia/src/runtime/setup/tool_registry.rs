#[cfg(feature = "energeia")]
use std::sync::Arc;

use snafu::prelude::*;
use tokio_util::sync::CancellationToken;
use tracing::info;

use organon::builtins;
use organon::registry::ToolRegistry;
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

use crate::error::Result;

#[cfg(feature = "energeia")]
struct MechanicalQaGate;

#[cfg(feature = "energeia")]
impl energeia::qa::QaGate for MechanicalQaGate {
    fn evaluate<'a>(
        &'a self,
        prompt: &'a energeia::qa::PromptSpec,
        pr_number: u64,
        diff: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = energeia::error::Result<energeia::types::QaResult>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { Ok(energeia::qa::run_qa(diff, prompt, pr_number, None).await) })
    }

    fn mechanical_check(
        &self,
        diff: &str,
        prompt: &energeia::qa::PromptSpec,
    ) -> Vec<energeia::types::MechanicalIssue> {
        energeia::qa::mechanical::mechanical_check(diff, prompt)
    }
}

/// Built tool registry plus optional energeia services that the rest of the
/// runtime needs to reach (e.g. the cron-driven dispatch executor).
pub(in crate::runtime) struct BuiltToolRegistry {
    pub registry: ToolRegistry,
    #[cfg(feature = "energeia")]
    pub energeia_services: Option<Arc<organon::builtins::energeia::EnergeiaServices>>,
}

pub(in crate::runtime) fn build_tool_registry(
    config: &AletheiaConfig,
    oikos: &Oikos,
    shutdown_token: &CancellationToken,
    after_action_log_dir: Option<std::path::PathBuf>,
) -> Result<BuiltToolRegistry> {
    let mut registry = ToolRegistry::new();
    let sandbox = sandbox_config(config);
    #[cfg(feature = "energeia")]
    let energeia_services = register_builtin_tools(
        &mut registry,
        sandbox,
        config,
        oikos,
        shutdown_token.child_token(),
        after_action_log_dir,
    )?;
    #[cfg(not(feature = "energeia"))]
    register_builtin_tools(
        &mut registry,
        sandbox,
        config,
        oikos,
        shutdown_token.child_token(),
        after_action_log_dir,
    )?;
    info!(count = registry.definitions().len(), "tools registered");
    Ok(BuiltToolRegistry {
        registry,
        #[cfg(feature = "energeia")]
        energeia_services,
    })
}

pub(in crate::runtime) fn sandbox_config(
    config: &AletheiaConfig,
) -> organon::sandbox::SandboxConfig {
    let sandbox_settings = &config.sandbox;
    organon::sandbox::SandboxConfig {
        enabled: sandbox_settings.enabled,
        enforcement: match sandbox_settings.enforcement {
            taxis::config::SandboxEnforcementMode::Enforcing => {
                organon::sandbox::SandboxEnforcement::Enforcing
            }
            _ => organon::sandbox::SandboxEnforcement::Permissive,
        },
        allowed_root: sandbox_settings.allowed_root.clone(),
        extra_read_paths: sandbox_settings.extra_read_paths.clone(),
        extra_write_paths: sandbox_settings.extra_write_paths.clone(),
        extra_exec_paths: sandbox_settings.extra_exec_paths.clone(),
        egress: match sandbox_settings.egress {
            taxis::config::EgressPolicy::Deny => organon::sandbox::EgressPolicy::Deny,
            taxis::config::EgressPolicy::Allowlist => organon::sandbox::EgressPolicy::Allowlist,
            _ => organon::sandbox::EgressPolicy::Allow,
        },
        egress_allowlist: sandbox_settings.egress_allowlist.clone(),
        nproc_limit: sandbox_settings.nproc_limit,
    }
}

#[cfg(feature = "energeia")]
fn register_builtin_tools(
    registry: &mut ToolRegistry,
    sandbox: organon::sandbox::SandboxConfig,
    config: &AletheiaConfig,
    oikos: &Oikos,
    cancel_token: CancellationToken,
    after_action_log_dir: Option<std::path::PathBuf>,
) -> Result<Option<Arc<organon::builtins::energeia::EnergeiaServices>>> {
    let services = build_energeia_services(config, oikos, cancel_token, after_action_log_dir)?;
    builtins::register_all_with_sandbox_and_energeia_services(registry, sandbox, services.as_ref())
        .whatever_context("failed to register builtin tools")?;
    Ok(Some(services))
}

#[cfg(not(feature = "energeia"))]
fn register_builtin_tools(
    registry: &mut ToolRegistry,
    sandbox: organon::sandbox::SandboxConfig,
    config: &AletheiaConfig,
    oikos: &Oikos,
    cancel_token: CancellationToken,
    after_action_log_dir: Option<std::path::PathBuf>,
) -> Result<()> {
    let _ = (config, oikos, cancel_token, after_action_log_dir);
    builtins::register_all_with_sandbox(registry, sandbox)
        .whatever_context("failed to register builtin tools")
}

#[cfg(feature = "energeia")]
fn build_energeia_services(
    config: &AletheiaConfig,
    oikos: &Oikos,
    cancel_token: CancellationToken,
    after_action_log_dir: Option<std::path::PathBuf>,
) -> Result<Arc<organon::builtins::energeia::EnergeiaServices>> {
    let store_path = oikos.data().join("energeia.fjall");
    if let Some(parent) = store_path.parent() {
        std::fs::create_dir_all(parent).with_whatever_context(|_| {
            format!("failed to create energeia data dir {}", parent.display())
        })?;
    }
    let db = fjall::Database::builder(&store_path)
        .open()
        .with_whatever_context(|_| {
            format!("failed to open energeia store at {}", store_path.display())
        })?;
    let store = Arc::new(
        energeia::store::EnergeiaStore::new(&db)
            .whatever_context("failed to initialize energeia store partitions")?,
    );
    let reconciled = store
        .reconcile_stale_running_dispatches(energeia::store::stale_running_dispatch_threshold())
        .whatever_context("failed to reconcile stale energeia dispatches at startup")?;
    info!(
        reconciled,
        "reconciled stale Running energeia dispatches at startup"
    );
    let cron_lock_store = super::super::cron_executor::open_lock_store(oikos)?;
    let cron_task_names = config
        .dispatch
        .cron_tasks
        .iter()
        .map(|task| task.name.clone())
        .collect();

    let default_model = config.agents.defaults.model_defaults.model.primary.as_str();
    let engine: Arc<dyn energeia::engine::DispatchEngine> =
        Arc::new(energeia::http::HttpEngine::new(default_model));
    let qa: Arc<dyn energeia::qa::QaGate> = Arc::new(MechanicalQaGate);
    let orchestrator = Arc::new(
        energeia::orchestrator::Orchestrator::new(
            engine,
            qa,
            energeia::orchestrator::OrchestratorConfig::new(),
        )
        .with_store(Arc::clone(&store))
        .with_cancel_token(cancel_token)
        .with_after_action_log_dir(after_action_log_dir),
    );

    Ok(Arc::new(
        organon::builtins::energeia::EnergeiaServices::new(orchestrator, store)
            .with_cron_lock_store(cron_lock_store, cron_task_names),
    ))
}
