//! Task registration: builtin task setup, maintenance tasks, cron tasks.

use std::time::Duration;

use crate::schedule::{
    BuiltinTask, Schedule, TaskAction, TaskDef, apply_jitter,
};

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

        // WHY: lesson extraction runs daily to learn from training data
        // independently of knowledge maintenance (no executor required).
        self.register_builtin(
            "lesson-extraction",
            "Lesson extraction from training data",
            Schedule::Cron("0 0 5 * * *".to_owned()),
            BuiltinTask::LessonExtraction,
            true,
        );

        // WHY: operational fact extraction runs on a short interval to keep
        // the knowledge graph current with system health metrics. Agents
        // recall these facts during bootstrap for situational awareness.
        self.register_builtin(
            "ops-fact-extraction",
            "Operational fact extraction",
            Schedule::Interval(Duration::from_mins(15)),
            BuiltinTask::OpsFactExtraction,
            false,
        );

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

        self.register_cron_tasks(&config.cron);
    }

    /// Register cron tasks (evolution, reflection, graph cleanup) based on configuration.
    ///
    /// All cron tasks are disabled by default. Each is registered only if
    /// its `enabled` flag is SET in the configuration.
    fn register_cron_tasks(&mut self, config: &crate::cron::CronConfig) {
        if config.evolution.enabled {
            self.register_builtin(
                "cron-evolution",
                "Evolution: config variant search",
                Schedule::Interval(config.evolution.interval),
                BuiltinTask::EvolutionSearch,
                false,
            );
        }

        if config.reflection.enabled {
            self.register_builtin(
                "cron-reflection",
                "Reflection: self-evaluation",
                Schedule::Interval(config.reflection.interval),
                BuiltinTask::SelfReflection,
                false,
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

    /// Register the 7 knowledge maintenance tasks with their schedules.
    fn register_knowledge_maintenance_tasks(&mut self) {
        let tasks: [(_, _, Schedule, BuiltinTask); 8] = [
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
                "embedding-refresh",
                "Embedding refresh",
                Schedule::Interval(Duration::from_hours(12)),
                BuiltinTask::EmbeddingRefresh,
            ),
            (
                "knowledge-gc",
                "Knowledge garbage collection",
                Schedule::Cron("0 0 4 * * *".to_owned()),
                BuiltinTask::KnowledgeGc,
            ),
            (
                "index-maintenance",
                "Index maintenance",
                Schedule::Cron("0 30 4 * * *".to_owned()),
                BuiltinTask::IndexMaintenance,
            ),
            (
                "graph-health-check",
                "Graph health check",
                Schedule::Cron("0 0 5 * * *".to_owned()),
                BuiltinTask::GraphHealthCheck,
            ),
            (
                "skill-decay",
                "Skill decay and retirement",
                Schedule::Cron("0 0 6 * * *".to_owned()),
                BuiltinTask::SkillDecay,
            ),
        ];

        for (id, name, schedule, task) in tasks {
            self.register_builtin(id, name, schedule, task, true);
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
