//! Canonical maintenance task registry.

use std::time::Duration;

use crate::schedule::{BuiltinTask, Schedule};

/// Owner subsystem for a maintenance task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaintenanceTaskOwner {
    /// Core daemon maintenance owned by oikonomos.
    Daemon,
    /// Knowledge-graph maintenance owned by the graph executor.
    KnowledgeGraph,
    /// Routing statistics maintenance.
    Routing,
    /// Optional cron task family.
    Cron,
    /// Prosoche self-audit maintenance.
    Prosoche,
    /// Nous self-audit maintenance.
    Nous,
}

/// Configuration section that controls a maintenance task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaintenanceConfigSection {
    /// `maintenance.traceRotation`.
    TraceRotation,
    /// `maintenance.driftDetection`.
    DriftDetection,
    /// `maintenance.dbMonitoring`.
    DbMonitoring,
    /// `maintenance.retention`.
    Retention,
    /// `maintenance.knowledgeMaintenance`.
    KnowledgeMaintenance,
    /// `maintenance.backup`.
    InstanceBackup,
    /// `maintenance.proposeRules`.
    ProposeRules,
    /// `promptAudit`.
    PromptAudit,
    /// Runtime after-action store handle.
    RoutingAfterActionStore,
    /// `maintenance.cronTasks.evolution`.
    CronEvolution,
    /// `maintenance.cronTasks.reflection`.
    CronReflection,
    /// `maintenance.cronTasks.graphCleanup`.
    CronGraphCleanup,
    /// Prosoche audit storage.
    ProsocheAudit,
    /// Nous self-audit defaults.
    NousSelfAudit,
}

/// Implementation state for a registry task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaintenanceTaskImplementationStatus {
    /// The task has an executable implementation.
    Implemented,
    /// The task is intentionally visible in docs/status but is not runnable.
    Planned,
}

/// Manual run target used by the CLI dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManualMaintenanceTask {
    /// Run trace rotation.
    TraceRotation,
    /// Run instance drift detection.
    DriftDetection,
    /// Run database size monitoring.
    DbMonitor,
    /// Run whole-instance backup.
    InstanceBackup,
    /// Run prompt audit log retention.
    PromptAuditRotation,
    /// Run nous self-audit checks.
    NousSelfAudit,
    /// Run prosoche self-audit checks.
    ProsocheSelfAudit,
    /// Refresh temporal decay scores for the knowledge graph.
    DecayRefresh,
    /// Deduplicate entities in the knowledge graph.
    EntityDedup,
    /// Recompute graph-wide scores in the knowledge graph.
    GraphRecompute,
    /// Compute skill decay and retire stale skills.
    SkillDecay,
    /// Materialize derived Datalog facts.
    DerivedFactsMaterialize,
    /// Run serendipity discovery over recent knowledge graph entities.
    SerendipityDiscovery,
    /// Consolidate overflowing facts into summarized knowledge.
    KnowledgeConsolidation,
    /// Rebuild the gnosis code-graph index for the workspace.
    IndexMaintenance,
}

/// Canonical metadata for one maintenance task.
#[derive(Debug, Clone, Copy)]
pub struct MaintenanceTaskDefinition {
    id: &'static str,
    name: &'static str,
    owner: MaintenanceTaskOwner,
    config_section: Option<MaintenanceConfigSection>,
    docs_label: &'static str,
    implementation_status: MaintenanceTaskImplementationStatus,
    metrics: &'static [&'static str],
    manual_run: Option<ManualMaintenanceTask>,
    registration: MaintenanceTaskRegistration,
}

impl MaintenanceTaskDefinition {
    /// Stable task identifier.
    #[must_use]
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Human-readable task name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Subsystem that owns the task.
    #[must_use]
    pub fn owner(&self) -> MaintenanceTaskOwner {
        self.owner
    }

    /// Config section that controls the task, when one exists.
    #[must_use]
    pub fn config_section(&self) -> Option<MaintenanceConfigSection> {
        self.config_section
    }

    /// Documentation label for operator-facing tables.
    #[must_use]
    pub fn docs_label(&self) -> &'static str {
        self.docs_label
    }

    /// Implementation status for the task.
    #[must_use]
    pub fn implementation_status(&self) -> MaintenanceTaskImplementationStatus {
        self.implementation_status
    }

    /// Metrics emitted by the task execution path.
    #[must_use]
    pub fn metrics(&self) -> &'static [&'static str] {
        self.metrics
    }

    /// Manual run handler for this task, when supported.
    #[must_use]
    pub fn manual_run(&self) -> Option<ManualMaintenanceTask> {
        self.manual_run
    }

    /// Built-in task variant attached to this registry row, when one exists.
    #[must_use]
    pub fn builtin(&self) -> Option<BuiltinTask> {
        match self.registration {
            MaintenanceTaskRegistration::Scheduled { builtin, .. }
            | MaintenanceTaskRegistration::Planned { builtin } => Some(builtin),
            MaintenanceTaskRegistration::ManualOnly => None,
        }
    }

    pub(crate) fn scheduled_task(
        &self,
        config: &super::MaintenanceConfig,
        capabilities: MaintenanceRuntimeCapabilities,
    ) -> Option<ScheduledMaintenanceTask> {
        let MaintenanceTaskRegistration::Scheduled {
            builtin,
            schedule,
            catch_up,
            condition,
        } = self.registration
        else {
            return None;
        };

        if self.implementation_status != MaintenanceTaskImplementationStatus::Implemented
            || !condition.is_met(config, capabilities)
        {
            return None;
        }

        Some(ScheduledMaintenanceTask {
            id: self.id,
            name: self.name,
            schedule: schedule.build(config),
            builtin,
            catch_up,
        })
    }

    /// Return the structured reason this task is unavailable for the current runtime.
    pub fn skipped_warning(
        &self,
        config: &super::MaintenanceConfig,
        capabilities: MaintenanceRuntimeCapabilities,
    ) -> Option<SkippedMaintenanceWarning> {
        let MaintenanceTaskRegistration::Scheduled { condition, .. } = self.registration else {
            return None;
        };
        condition.skipped_warning(self.id, config, capabilities)
    }
}

/// Runtime capabilities that affect scheduled task registration.
#[derive(Debug, Clone, Copy, Default)]
pub struct MaintenanceRuntimeCapabilities {
    /// A retention executor is available.
    pub has_retention_executor: bool,
    /// A knowledge maintenance executor is available.
    pub has_knowledge_executor: bool,
    /// A daemon bridge is available.
    pub has_bridge: bool,
}

/// Fully resolved scheduled maintenance task.
#[derive(Debug, Clone)]
pub(crate) struct ScheduledMaintenanceTask {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) schedule: Schedule,
    pub(crate) builtin: BuiltinTask,
    pub(crate) catch_up: bool,
}

/// Warning emitted when an enabled task cannot be registered.
#[derive(Debug, Clone, Copy)]
pub struct SkippedMaintenanceWarning {
    /// Stable task identifier.
    pub task_id: &'static str,
    /// Human-readable reason the task could not be registered.
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum MaintenanceTaskRegistration {
    Scheduled {
        builtin: BuiltinTask,
        schedule: ScheduleSource,
        catch_up: bool,
        condition: RegistrationCondition,
    },
    ManualOnly,
    Planned {
        builtin: BuiltinTask,
    },
}

#[derive(Debug, Clone, Copy)]
enum ScheduleSource {
    Cron(&'static str),
    FixedIntervalSecs(u64),
    InstanceBackupIntervalHours,
    DerivedRulesMaterializationInterval,
    GnosisRebuildInterval,
    SerendipityCadence,
    CronEvolutionInterval,
    CronReflectionInterval,
    CronGraphCleanupInterval,
}

impl ScheduleSource {
    fn build(self, config: &super::MaintenanceConfig) -> Schedule {
        match self {
            Self::Cron(expr) => Schedule::Cron(expr.to_owned()),
            Self::FixedIntervalSecs(secs) => Schedule::Interval(Duration::from_secs(secs)),
            Self::InstanceBackupIntervalHours => {
                Schedule::Interval(Duration::from_hours(config.instance_backup.interval_hours))
            }
            Self::DerivedRulesMaterializationInterval => Schedule::Interval(
                config
                    .knowledge_maintenance
                    .derived_rules
                    .materialization_interval,
            ),
            Self::GnosisRebuildInterval => {
                Schedule::Interval(config.knowledge_maintenance.index_maintenance_interval)
            }
            Self::SerendipityCadence => {
                Schedule::Cron(config.knowledge_maintenance.serendipity.cadence.clone())
            }
            Self::CronEvolutionInterval => Schedule::Interval(config.cron.evolution.interval),
            Self::CronReflectionInterval => Schedule::Interval(config.cron.reflection.interval),
            Self::CronGraphCleanupInterval => {
                Schedule::Interval(config.cron.graph_cleanup.interval)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RegistrationCondition {
    ConfigEnabled(MaintenanceConfigSection),
    AfterActionStoreConfigured,
    RetentionEnabledWithExecutor,
    KnowledgeMaintenanceEnabledWithExecutor,
    KnowledgeExecutorConfigured,
    CronEvolutionEnabledWithBridge,
    CronReflectionEnabledWithBridge,
    CronGraphCleanupEnabledWithKnowledge,
    SerendipityEnabledWithKnowledge,
}

impl RegistrationCondition {
    fn is_met(
        self,
        config: &super::MaintenanceConfig,
        capabilities: MaintenanceRuntimeCapabilities,
    ) -> bool {
        match self {
            Self::ConfigEnabled(section) => section.is_enabled(config),
            Self::AfterActionStoreConfigured => config.after_action_store.is_some(),
            Self::RetentionEnabledWithExecutor => {
                config.retention.enabled && capabilities.has_retention_executor
            }
            Self::KnowledgeMaintenanceEnabledWithExecutor => {
                config.knowledge_maintenance.enabled && capabilities.has_knowledge_executor
            }
            Self::KnowledgeExecutorConfigured => capabilities.has_knowledge_executor,
            Self::CronEvolutionEnabledWithBridge => {
                config.cron.evolution.enabled && capabilities.has_bridge
            }
            Self::CronReflectionEnabledWithBridge => {
                config.cron.reflection.enabled && capabilities.has_bridge
            }
            Self::CronGraphCleanupEnabledWithKnowledge => {
                config.cron.graph_cleanup.enabled && capabilities.has_knowledge_executor
            }
            Self::SerendipityEnabledWithKnowledge => {
                config.knowledge_maintenance.enabled
                    && config.knowledge_maintenance.serendipity.enabled
                    && capabilities.has_knowledge_executor
            }
        }
    }

    fn skipped_warning(
        self,
        task_id: &'static str,
        config: &super::MaintenanceConfig,
        capabilities: MaintenanceRuntimeCapabilities,
    ) -> Option<SkippedMaintenanceWarning> {
        match self {
            Self::CronEvolutionEnabledWithBridge
                if config.cron.evolution.enabled && !capabilities.has_bridge =>
            {
                Some(SkippedMaintenanceWarning {
                    task_id,
                    reason: "no daemon bridge is configured",
                })
            }
            Self::CronReflectionEnabledWithBridge
                if config.cron.reflection.enabled && !capabilities.has_bridge =>
            {
                Some(SkippedMaintenanceWarning {
                    task_id,
                    reason: "no daemon bridge is configured",
                })
            }
            _ => None,
        }
    }
}

impl MaintenanceConfigSection {
    fn is_enabled(self, config: &super::MaintenanceConfig) -> bool {
        match self {
            Self::TraceRotation => config.trace_rotation.enabled,
            Self::DriftDetection => config.drift_detection.enabled,
            Self::DbMonitoring => config.db_monitoring.enabled,
            Self::Retention => config.retention.enabled,
            Self::KnowledgeMaintenance => config.knowledge_maintenance.enabled,
            Self::InstanceBackup => config.instance_backup.enabled,
            Self::ProposeRules => config.propose_rules.enabled,
            Self::PromptAudit => config.prompt_audit.enabled,
            Self::RoutingAfterActionStore
            | Self::CronEvolution
            | Self::CronReflection
            | Self::CronGraphCleanup
            | Self::ProsocheAudit
            | Self::NousSelfAudit => false,
        }
    }
}

const CRON_METRICS: &[&str] = &["aletheia_cron_executions", "aletheia_cron_duration_seconds"];
const NO_METRICS: &[&str] = &[];

const TASKS: &[MaintenanceTaskDefinition] = &[
    task(
        "trace-rotation",
        "Trace rotation",
        MaintenanceTaskOwner::Daemon,
        Some(MaintenanceConfigSection::TraceRotation),
        "Trace rotation",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::TraceRotation),
        scheduled(
            BuiltinTask::TraceRotation,
            ScheduleSource::Cron("0 0 3 * * *"),
            true,
            RegistrationCondition::ConfigEnabled(MaintenanceConfigSection::TraceRotation),
        ),
    ),
    task(
        "drift-detection",
        "Instance drift detection",
        MaintenanceTaskOwner::Daemon,
        Some(MaintenanceConfigSection::DriftDetection),
        "Instance drift detection",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::DriftDetection),
        scheduled(
            BuiltinTask::DriftDetection,
            ScheduleSource::Cron("0 0 4 * * *"),
            true,
            RegistrationCondition::ConfigEnabled(MaintenanceConfigSection::DriftDetection),
        ),
    ),
    task(
        "db-monitor",
        "Database size monitor",
        MaintenanceTaskOwner::Daemon,
        Some(MaintenanceConfigSection::DbMonitoring),
        "Database size monitor",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::DbMonitor),
        scheduled(
            BuiltinTask::DbSizeMonitor,
            ScheduleSource::FixedIntervalSecs(6 * 60 * 60),
            true,
            RegistrationCondition::ConfigEnabled(MaintenanceConfigSection::DbMonitoring),
        ),
    ),
    task(
        "routing-store-refresh",
        "Routing after-action store refresh",
        MaintenanceTaskOwner::Routing,
        Some(MaintenanceConfigSection::RoutingAfterActionStore),
        "Routing after-action store refresh",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        None,
        scheduled(
            BuiltinTask::RoutingStoreRefresh,
            ScheduleSource::FixedIntervalSecs(10 * 60),
            false,
            RegistrationCondition::AfterActionStoreConfigured,
        ),
    ),
    task(
        "retention-execution",
        "Data retention cleanup",
        MaintenanceTaskOwner::Daemon,
        Some(MaintenanceConfigSection::Retention),
        "Data retention cleanup",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        None,
        scheduled(
            BuiltinTask::RetentionExecution,
            ScheduleSource::Cron("0 30 3 * * *"),
            true,
            RegistrationCondition::RetentionEnabledWithExecutor,
        ),
    ),
    task(
        "lesson-extraction",
        "Lesson extraction from training data",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Lesson extraction",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        None,
        scheduled(
            BuiltinTask::LessonExtraction,
            ScheduleSource::Cron("0 0 5 * * *"),
            true,
            RegistrationCondition::KnowledgeExecutorConfigured,
        ),
    ),
    task(
        "ops-fact-extraction",
        "Operational fact extraction",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Operational fact extraction",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        None,
        scheduled(
            BuiltinTask::OpsFactExtraction,
            ScheduleSource::FixedIntervalSecs(15 * 60),
            false,
            RegistrationCondition::KnowledgeExecutorConfigured,
        ),
    ),
    task(
        "instance-backup",
        "Whole-instance backup",
        MaintenanceTaskOwner::Daemon,
        Some(MaintenanceConfigSection::InstanceBackup),
        "Whole-instance backup",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::InstanceBackup),
        scheduled(
            BuiltinTask::InstanceBackup,
            ScheduleSource::InstanceBackupIntervalHours,
            true,
            RegistrationCondition::ConfigEnabled(MaintenanceConfigSection::InstanceBackup),
        ),
    ),
    task(
        "propose-rules",
        "Rule proposal generation from observed patterns",
        MaintenanceTaskOwner::Daemon,
        Some(MaintenanceConfigSection::ProposeRules),
        "Rule proposal generation",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        None,
        scheduled(
            BuiltinTask::ProposeRules,
            ScheduleSource::Cron("0 0 3 * * SUN"),
            false,
            RegistrationCondition::ConfigEnabled(MaintenanceConfigSection::ProposeRules),
        ),
    ),
    task(
        "prompt-audit-rotation",
        "Prompt audit log retention",
        MaintenanceTaskOwner::Daemon,
        Some(MaintenanceConfigSection::PromptAudit),
        "Prompt audit log retention",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::PromptAuditRotation),
        scheduled(
            BuiltinTask::PromptAuditRotation,
            ScheduleSource::Cron("0 0 2 * * *"),
            true,
            RegistrationCondition::ConfigEnabled(MaintenanceConfigSection::PromptAudit),
        ),
    ),
    task(
        "decay-refresh",
        "Decay score refresh",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Decay score refresh",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::DecayRefresh),
        scheduled(
            BuiltinTask::DecayRefresh,
            ScheduleSource::FixedIntervalSecs(4 * 60 * 60),
            true,
            RegistrationCondition::KnowledgeMaintenanceEnabledWithExecutor,
        ),
    ),
    task(
        "entity-dedup",
        "Entity deduplication",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Entity deduplication",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::EntityDedup),
        scheduled(
            BuiltinTask::EntityDedup,
            ScheduleSource::FixedIntervalSecs(6 * 60 * 60),
            true,
            RegistrationCondition::KnowledgeMaintenanceEnabledWithExecutor,
        ),
    ),
    task(
        "graph-recompute",
        "Graph score recomputation",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Graph score recomputation",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::GraphRecompute),
        scheduled(
            BuiltinTask::GraphRecompute,
            ScheduleSource::FixedIntervalSecs(8 * 60 * 60),
            true,
            RegistrationCondition::KnowledgeMaintenanceEnabledWithExecutor,
        ),
    ),
    task(
        "skill-decay",
        "Skill decay and retirement",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Skill decay and retirement",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::SkillDecay),
        scheduled(
            BuiltinTask::SkillDecay,
            ScheduleSource::Cron("0 0 6 * * *"),
            true,
            RegistrationCondition::KnowledgeMaintenanceEnabledWithExecutor,
        ),
    ),
    task(
        "derived-facts-materialize",
        "Derived Datalog rule materialization",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Derived Datalog rule materialization",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::DerivedFactsMaterialize),
        scheduled(
            BuiltinTask::DerivedFactsMaterialize,
            ScheduleSource::DerivedRulesMaterializationInterval,
            true,
            RegistrationCondition::KnowledgeMaintenanceEnabledWithExecutor,
        ),
    ),
    task(
        "serendipity-discovery",
        "Serendipity discovery",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Serendipity discovery",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::SerendipityDiscovery),
        scheduled(
            BuiltinTask::SerendipityDiscovery,
            ScheduleSource::SerendipityCadence,
            true,
            RegistrationCondition::SerendipityEnabledWithKnowledge,
        ),
    ),
    task(
        "knowledge-consolidation",
        "Knowledge consolidation",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Knowledge consolidation",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::KnowledgeConsolidation),
        scheduled(
            BuiltinTask::KnowledgeConsolidation,
            ScheduleSource::FixedIntervalSecs(4 * 60 * 60),
            true,
            RegistrationCondition::KnowledgeMaintenanceEnabledWithExecutor,
        ),
    ),
    task(
        "cron-evolution",
        "Evolution: config variant search",
        MaintenanceTaskOwner::Cron,
        Some(MaintenanceConfigSection::CronEvolution),
        "Evolution config search",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        None,
        scheduled(
            BuiltinTask::EvolutionSearch,
            ScheduleSource::CronEvolutionInterval,
            false,
            RegistrationCondition::CronEvolutionEnabledWithBridge,
        ),
    ),
    task(
        "cron-reflection",
        "Reflection: self-evaluation",
        MaintenanceTaskOwner::Cron,
        Some(MaintenanceConfigSection::CronReflection),
        "Reflection self-evaluation",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        None,
        scheduled(
            BuiltinTask::SelfReflection,
            ScheduleSource::CronReflectionInterval,
            false,
            RegistrationCondition::CronReflectionEnabledWithBridge,
        ),
    ),
    task(
        "cron-graph-cleanup",
        "Graph cleanup: orphan removal",
        MaintenanceTaskOwner::Cron,
        Some(MaintenanceConfigSection::CronGraphCleanup),
        "Graph cleanup",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        None,
        scheduled(
            BuiltinTask::GraphCleanup,
            ScheduleSource::CronGraphCleanupInterval,
            false,
            RegistrationCondition::CronGraphCleanupEnabledWithKnowledge,
        ),
    ),
    planned(
        "embedding-refresh",
        "Embedding refresh",
        MaintenanceTaskOwner::KnowledgeGraph,
        MaintenanceConfigSection::KnowledgeMaintenance,
        "Embedding refresh",
        BuiltinTask::EmbeddingRefresh,
    ),
    planned(
        "knowledge-gc",
        "Knowledge garbage collection",
        MaintenanceTaskOwner::KnowledgeGraph,
        MaintenanceConfigSection::KnowledgeMaintenance,
        "Knowledge garbage collection",
        BuiltinTask::KnowledgeGc,
    ),
    task(
        "index-maintenance",
        "Gnosis code-graph index rebuild",
        MaintenanceTaskOwner::KnowledgeGraph,
        Some(MaintenanceConfigSection::KnowledgeMaintenance),
        "Gnosis code-graph index rebuild",
        MaintenanceTaskImplementationStatus::Implemented,
        CRON_METRICS,
        Some(ManualMaintenanceTask::IndexMaintenance),
        scheduled(
            BuiltinTask::IndexMaintenance,
            ScheduleSource::GnosisRebuildInterval,
            true,
            RegistrationCondition::KnowledgeMaintenanceEnabledWithExecutor,
        ),
    ),
    planned(
        "graph-health-check",
        "Graph health check",
        MaintenanceTaskOwner::KnowledgeGraph,
        MaintenanceConfigSection::KnowledgeMaintenance,
        "Graph health check",
        BuiltinTask::GraphHealthCheck,
    ),
    task(
        "self-audit",
        "Nous self-audit",
        MaintenanceTaskOwner::Nous,
        Some(MaintenanceConfigSection::NousSelfAudit),
        "Nous self-audit",
        MaintenanceTaskImplementationStatus::Implemented,
        NO_METRICS,
        Some(ManualMaintenanceTask::NousSelfAudit),
        MaintenanceTaskRegistration::ManualOnly,
    ),
    task(
        "prosoche-self-audit",
        "Prosoche self-audit",
        MaintenanceTaskOwner::Prosoche,
        Some(MaintenanceConfigSection::ProsocheAudit),
        "Prosoche self-audit",
        MaintenanceTaskImplementationStatus::Implemented,
        NO_METRICS,
        Some(ManualMaintenanceTask::ProsocheSelfAudit),
        MaintenanceTaskRegistration::ManualOnly,
    ),
];

#[expect(
    clippy::too_many_arguments,
    reason = "registry rows are declarative and keeping fields adjacent prevents drift"
)]
const fn task(
    id: &'static str,
    name: &'static str,
    owner: MaintenanceTaskOwner,
    config_section: Option<MaintenanceConfigSection>,
    docs_label: &'static str,
    implementation_status: MaintenanceTaskImplementationStatus,
    metrics: &'static [&'static str],
    manual_run: Option<ManualMaintenanceTask>,
    registration: MaintenanceTaskRegistration,
) -> MaintenanceTaskDefinition {
    MaintenanceTaskDefinition {
        id,
        name,
        owner,
        config_section,
        docs_label,
        implementation_status,
        metrics,
        manual_run,
        registration,
    }
}

const fn scheduled(
    builtin: BuiltinTask,
    schedule: ScheduleSource,
    catch_up: bool,
    condition: RegistrationCondition,
) -> MaintenanceTaskRegistration {
    MaintenanceTaskRegistration::Scheduled {
        builtin,
        schedule,
        catch_up,
        condition,
    }
}

const fn planned(
    id: &'static str,
    name: &'static str,
    owner: MaintenanceTaskOwner,
    config_section: MaintenanceConfigSection,
    docs_label: &'static str,
    builtin: BuiltinTask,
) -> MaintenanceTaskDefinition {
    task(
        id,
        name,
        owner,
        Some(config_section),
        docs_label,
        MaintenanceTaskImplementationStatus::Planned,
        NO_METRICS,
        None,
        MaintenanceTaskRegistration::Planned { builtin },
    )
}

/// Return the canonical maintenance task registry.
#[must_use]
pub fn maintenance_task_registry() -> &'static [MaintenanceTaskDefinition] {
    TASKS
}

/// Return all manual-run maintenance tasks.
pub fn manual_maintenance_tasks() -> impl Iterator<Item = &'static MaintenanceTaskDefinition> {
    TASKS.iter().filter(|task| task.manual_run.is_some())
}

/// Return all manual-run maintenance task identifiers.
#[must_use]
pub fn manual_maintenance_task_ids() -> Vec<&'static str> {
    manual_maintenance_tasks()
        .map(MaintenanceTaskDefinition::id)
        .collect()
}

/// Look up a maintenance task by id.
#[must_use]
pub fn maintenance_task_by_id(id: &str) -> Option<&'static MaintenanceTaskDefinition> {
    TASKS
        .iter()
        .find(|task| task.id == id)
        .or_else(|| match id {
            "fjall-backup" => TASKS.iter().find(|task| task.id == "instance-backup"),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn registry_task_ids_are_unique() {
        let mut seen = BTreeSet::new();
        for task in maintenance_task_registry() {
            assert!(seen.insert(task.id()), "duplicate task id {}", task.id());
        }
    }

    #[test]
    fn manual_tasks_are_implemented() {
        for task in manual_maintenance_tasks() {
            assert_eq!(
                task.implementation_status(),
                MaintenanceTaskImplementationStatus::Implemented,
                "manual task {} must be implemented",
                task.id()
            );
        }
    }

    #[test]
    fn planned_tasks_are_not_runnable() {
        let config = super::super::MaintenanceConfig::default();
        let capabilities = MaintenanceRuntimeCapabilities::default();

        for task in maintenance_task_registry() {
            if task.implementation_status() == MaintenanceTaskImplementationStatus::Planned {
                assert!(
                    task.manual_run().is_none(),
                    "planned task {} must not support manual run",
                    task.id()
                );
                assert!(
                    task.scheduled_task(&config, capabilities).is_none(),
                    "planned task {} must not schedule",
                    task.id()
                );
            }
        }
    }

    #[test]
    fn index_maintenance_is_implemented_and_schedulable() {
        let Some(definition) = maintenance_task_by_id("index-maintenance") else {
            panic!("index-maintenance should be present in registry");
        };
        assert_eq!(definition.id(), "index-maintenance");
        assert_eq!(
            definition.implementation_status(),
            MaintenanceTaskImplementationStatus::Implemented,
            "index-maintenance must be implemented for #5963"
        );
        assert_eq!(
            definition.manual_run(),
            Some(ManualMaintenanceTask::IndexMaintenance)
        );
        assert_eq!(definition.builtin(), Some(BuiltinTask::IndexMaintenance));

        let mut config = super::super::MaintenanceConfig::default();
        config.knowledge_maintenance.enabled = true;
        let capabilities = MaintenanceRuntimeCapabilities {
            has_knowledge_executor: true,
            ..MaintenanceRuntimeCapabilities::default()
        };
        let scheduled = definition
            .scheduled_task(&config, capabilities)
            .expect("index-maintenance should schedule when knowledge maintenance is enabled");
        assert_eq!(scheduled.id, "index-maintenance");
    }

    #[test]
    fn canonical_backup_task_id_is_instance_backup() {
        let Some(definition) = maintenance_task_by_id("instance-backup") else {
            panic!("instance-backup should be present in registry");
        };
        assert_eq!(definition.id(), "instance-backup");
        assert_eq!(definition.name(), "Whole-instance backup");
        assert_eq!(
            definition.manual_run(),
            Some(ManualMaintenanceTask::InstanceBackup)
        );
        assert_eq!(definition.builtin(), Some(BuiltinTask::InstanceBackup));
    }

    #[test]
    fn legacy_fjall_backup_lookup_resolves_to_canonical_definition() {
        let Some(canonical) = maintenance_task_by_id("instance-backup") else {
            panic!("canonical instance-backup should exist");
        };
        let Some(legacy) = maintenance_task_by_id("fjall-backup") else {
            panic!("legacy fjall-backup alias should resolve");
        };
        assert_eq!(
            legacy.id(),
            canonical.id(),
            "fjall-backup alias must return the canonical instance-backup definition"
        );
    }

    #[test]
    fn manual_maintenance_task_ids_include_only_canonical_backup() {
        let ids = manual_maintenance_task_ids();
        assert!(
            ids.contains(&"instance-backup"),
            "manual ids should include canonical instance-backup"
        );
        assert!(
            !ids.contains(&"fjall-backup"),
            "manual ids should not include legacy fjall-backup"
        );
    }
}
