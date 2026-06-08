//! Task registration: builtin task setup, maintenance tasks, cron tasks.

use std::time::Duration;

use crate::schedule::{BuiltinTask, Schedule, TaskAction, TaskDef, apply_jitter};

use super::{RegisteredTask, TaskRunner};

impl TaskRunner {
    /// Register a builtin task with standard defaults, binding it to this runner's `nous_id`.
    fn register_builtin(
        &mut self,
        id: &str,
        name: &str,
        schedule: Schedule,
        task: BuiltinTask,
        catch_up: bool,
    ) {
        self.register(TaskDef {
            id: id.to_owned(),
            name: name.to_owned(),
            nous_id: self.nous_id.clone(),
            schedule,
            action: TaskAction::Builtin(task),
            enabled: true,
            catch_up,
            ..TaskDef::default()
        });
    }

    /// Register default maintenance tasks based on configuration.
    ///
    /// Skips disabled tasks and retention when no executor is provided.
    pub fn register_maintenance_tasks(&mut self) {
        let Some(config) = self.maintenance.clone() else {
            return;
        };
        let has_executor = self.retention_executor.is_some();

        if config.trace_rotation.enabled {
            self.register_builtin(
                "trace-rotation",
                "Trace rotation",
                Schedule::Cron("0 0 3 * * *".to_owned()),
                BuiltinTask::TraceRotation,
                true,
            );
        }

        if config.drift_detection.enabled {
            self.register_builtin(
                "drift-detection",
                "Instance drift detection",
                Schedule::Cron("0 0 4 * * *".to_owned()),
                BuiltinTask::DriftDetection,
                true,
            );
        }

        if config.db_monitoring.enabled {
            self.register_builtin(
                "db-monitor",
                "Database size monitor",
                Schedule::Interval(Duration::from_hours(6)),
                BuiltinTask::DbSizeMonitor,
                true,
            );
        }

        if config.after_action_store.is_some() {
            // WHY: ten minutes keeps empirical dispatch routing fresh without
            // competing with per-turn writes or daily log rotation.
            self.register_builtin(
                "routing-store-refresh",
                "Routing after-action store refresh",
                Schedule::Interval(Duration::from_mins(10)),
                BuiltinTask::RoutingStoreRefresh,
                false,
            );
        }

        if config.retention.enabled && has_executor {
            self.register_builtin(
                "retention-execution",
                "Data retention cleanup",
                Schedule::Cron("0 30 3 * * *".to_owned()),
                BuiltinTask::RetentionExecution,
                true,
            );
        }

        if config.knowledge_maintenance.enabled && self.knowledge_executor.is_some() {
            self.register_knowledge_maintenance_tasks();
        }

        if self.knowledge_executor.is_some() {
            // WHY: lesson extraction produces durable facts; without a
            // knowledge executor it cannot satisfy its persistence contract.
            self.register_builtin(
                "lesson-extraction",
                "Lesson extraction from training data",
                Schedule::Cron("0 0 5 * * *".to_owned()),
                BuiltinTask::LessonExtraction,
                true,
            );

            // WHY: operational facts must be retrievable from the knowledge
            // graph after extraction, not just logged as transient metrics.
            self.register_builtin(
                "ops-fact-extraction",
                "Operational fact extraction",
                Schedule::Interval(Duration::from_mins(15)),
                BuiltinTask::OpsFactExtraction,
                false,
            );
        }

        if config.fjall_backup.enabled {
            self.register_builtin(
                "fjall-backup",
                "Fjall knowledge store backup",
                Schedule::Interval(Duration::from_hours(config.fjall_backup.interval_hours)),
                BuiltinTask::FjallBackup,
                true,
            );
        }

        if config.propose_rules.enabled {
            // WHY: weekly cadence balances freshness with noise — daily would flood
            // the operator with near-identical proposals.
            self.register_builtin(
                "propose-rules",
                "Rule proposal generation from observed patterns",
                Schedule::Cron("0 0 3 * * SUN".to_owned()),
                BuiltinTask::ProposeRules,
                false,
            );
        }

        if config.prompt_audit.enabled {
            // WHY: daily cadence matches the log's per-day filenames; pruning
            // more often wastes IO. Fires at 02:00 UTC to avoid overlapping
            // with trace rotation (03:00) and drift detection (04:00).
            self.register_builtin(
                "prompt-audit-rotation",
                "Prompt audit log retention",
                Schedule::Cron("0 0 2 * * *".to_owned()),
                BuiltinTask::PromptAuditRotation,
                true,
            );
        }

        self.register_cron_tasks(&config.cron);
    }

    /// Register cron tasks (evolution, reflection, graph cleanup) based on configuration.
    ///
    /// All cron tasks are disabled by default. Each is registered only if
    /// its `enabled` flag is SET in the configuration.
    fn register_cron_tasks(&mut self, config: &crate::cron::CronConfig) {
        let has_bridge = self.bridge.is_some();

        if config.evolution.enabled && has_bridge {
            self.register_builtin(
                "cron-evolution",
                "Evolution: config variant search",
                Schedule::Interval(config.evolution.interval),
                BuiltinTask::EvolutionSearch,
                false,
            );
        } else if config.evolution.enabled {
            tracing::warn!(
                task = "cron-evolution",
                "skipping bridge-dependent cron task because no daemon bridge is configured"
            );
        }

        if config.reflection.enabled && has_bridge {
            self.register_builtin(
                "cron-reflection",
                "Reflection: self-evaluation",
                Schedule::Interval(config.reflection.interval),
                BuiltinTask::SelfReflection,
                false,
            );
        } else if config.reflection.enabled {
            tracing::warn!(
                task = "cron-reflection",
                "skipping bridge-dependent cron task because no daemon bridge is configured"
            );
        }

        if config.graph_cleanup.enabled && self.knowledge_executor.is_some() {
            self.register_builtin(
                "cron-graph-cleanup",
                "Graph cleanup: orphan removal",
                Schedule::Interval(config.graph_cleanup.interval),
                BuiltinTask::GraphCleanup,
                false,
            );
        }
    }

    /// Register implemented knowledge maintenance tasks with their schedules.
    fn register_knowledge_maintenance_tasks(&mut self) {
        let (serendipity_enabled, serendipity_cadence) = {
            let Some(config) = self.maintenance.as_ref() else {
                return;
            };
            (
                config.knowledge_maintenance.serendipity.enabled,
                config.knowledge_maintenance.serendipity.cadence.clone(),
            )
        };
        let tasks: [(_, _, Schedule, BuiltinTask); 5] = [
            (
                "decay-refresh",
                "Decay score refresh",
                Schedule::Interval(Duration::from_hours(4)),
                BuiltinTask::DecayRefresh,
            ),
            (
                "entity-dedup",
                "Entity deduplication",
                Schedule::Interval(Duration::from_hours(6)),
                BuiltinTask::EntityDedup,
            ),
            (
                "graph-recompute",
                "Graph score recomputation",
                Schedule::Interval(Duration::from_hours(8)),
                BuiltinTask::GraphRecompute,
            ),
            (
                "skill-decay",
                "Skill decay and retirement",
                Schedule::Cron("0 0 6 * * *".to_owned()),
                BuiltinTask::SkillDecay,
            ),
            (
                "derived-facts-materialize",
                "Derived Datalog rule materialization",
                // WHY: every 6 hours balances freshness of IS-A closure / causal chains
                // against the cost of a full Datalog fixpoint pass. Aligned between
                // graph-recompute (8h) and entity-dedup (6h) to share warm cache state.
                Schedule::Interval(Duration::from_hours(6)),
                BuiltinTask::DerivedFactsMaterialize,
            ),
        ];

        for (id, name, schedule, task) in tasks {
            self.register_builtin(id, name, schedule, task, true);
        }

        if serendipity_enabled {
            self.register_builtin(
                "serendipity-discovery",
                "Serendipity discovery",
                Schedule::Cron(serendipity_cadence),
                BuiltinTask::SerendipityDiscovery,
                true,
            );
        }
    }

    /// Register a task. Startup tasks are marked for immediate execution.
    ///
    /// If the task has jitter configured, it is applied to the initial `next_run`.
    pub fn register(&mut self, task: TaskDef) {
        let base_next_run = match &task.schedule {
            Schedule::Startup => Some(jiff::Timestamp::now()),
            other => other.next_run().unwrap_or(None),
        };

        // WHY: apply jitter to spread task executions that share the same schedule.
        let next_run = apply_jitter(base_next_run, &task.id, task.jitter).or(base_next_run);

        tracing::info!(
            nous_id = %self.nous_id,
            task_id = %task.id,
            task_name = %task.name,
            "registered task"
        );

        self.tasks.push(RegisteredTask {
            def: task,
            next_run,
            last_run: None,
            run_count: 0,
            consecutive_failures: 0,
            backoff_until: None,
            last_error: None,
        });
    }
}
