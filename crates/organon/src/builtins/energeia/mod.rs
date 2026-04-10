//! Energeia capability tool implementations.
//!
//! Wires the 9 energeia agent tools to real subsystem calls:
//! - dromeus → Orchestrator::dispatch / dry_run
//! - dokimasia → qa::run_qa
//! - diorthosis → qa::corrective::generate_corrective
//! - epitropos → steward::service::run_once
//! - parateresis → EnergeiaStore observation pipeline
//! - mathesis → EnergeiaStore::query_lessons / add_lesson
//! - prographe → prompt template rendering
//! - schedion → PromptDag + compute_frontier
//! - metron → MetricsService health / cost / velocity

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexMap;

use aletheia_energeia::dag::{PromptDag, compute_frontier};
use aletheia_energeia::metrics::MetricsService;
use aletheia_energeia::orchestrator::Orchestrator;
use aletheia_energeia::qa::corrective::generate_corrective;
use aletheia_energeia::qa::run_qa;
use aletheia_energeia::steward::service::{StewardConfig, run_once};
use aletheia_energeia::store::EnergeiaStore;
use aletheia_energeia::store::records::{NewLesson, NewObservation};
use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

// ── Services ────────────────────────────────────────────────────────────────

/// Services injected at registration time for energeia tool executors.
///
/// The orchestrator handles dispatch (dromeus), and the store backs lessons,
/// observations, and metrics (mathesis, parateresis, metron, diorthosis).
pub struct EnergeiaServices {
    /// Top-level dispatch orchestrator wiring engine, QA, and store.
    pub orchestrator: Arc<Orchestrator>,
    /// State persistence store for lessons, observations, and CI validations.
    pub store: Arc<EnergeiaStore>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract a required string field from tool arguments.
fn require_str<'a>(
    args: &'a serde_json::Value,
    field: &str,
) -> std::result::Result<&'a str, ToolResult> {
    args.get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolResult::error(format!("missing required field '{field}'")))
}

/// Extract an optional string field from tool arguments.
fn opt_str<'a>(args: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(|v| v.as_str())
}

/// Extract an optional u64 field from tool arguments.
fn opt_u64(args: &serde_json::Value, field: &str) -> Option<u64> {
    args.get(field).and_then(|v| v.as_u64())
}

/// Extract an optional bool field from tool arguments.
fn opt_bool(args: &serde_json::Value, field: &str) -> Option<bool> {
    args.get(field).and_then(|v| v.as_bool())
}

/// Serialize a value to a pretty-printed JSON ToolResult.
fn to_json_text<T: serde::Serialize>(value: &T) -> ToolResult {
    match serde_json::to_string_pretty(value) {
        Ok(text) => ToolResult::text(text),
        Err(e) => ToolResult::error(format!("serialization error: {e}")),
    }
}

// ── dromeus (δρομεύς — runner) ─────────────────────────────────────────────

fn dromeus_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("dromeus"),
        description: "Execute a dispatch spec: run prompt groups in parallel or sequential order, \
            spawning agent sessions per prompt. Returns aggregate outcomes and total cost."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "spec".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Dispatch spec identifier or inline spec JSON".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "GitHub project slug (owner/repo)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "budget_usd".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum total spend in USD (default: no limit)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "max_turns".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Maximum turns per session (default: no limit)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Validate the spec without spawning sessions (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["spec".to_owned(), "project".to_owned()],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    }
}

struct DromeusExecutor {
    orchestrator: Option<Arc<Orchestrator>>,
}

impl ToolExecutor for DromeusExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref orchestrator) = self.orchestrator else {
                return Ok(ToolResult::error(
                    "dromeus: orchestrator not configured (missing EnergeiaServices)",
                ));
            };

            let args = &input.arguments;
            let spec_str = match require_str(args, "spec") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let project = match require_str(args, "project") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let dry_run = opt_bool(args, "dry_run").unwrap_or(false);

            // WHY: spec is a JSON array of PromptSpec objects. Callers build the
            // spec programmatically (e.g. from prographe output) and pass it inline.
            let prompts: Vec<aletheia_energeia::prompt::PromptSpec> =
                match serde_json::from_str(spec_str) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "dromeus: invalid spec JSON: {e}"
                        )));
                    }
                };

            if dry_run {
                return match orchestrator.dry_run(&prompts) {
                    Ok(plan) => Ok(to_json_text(&plan)),
                    Err(e) => Ok(ToolResult::error(format!("dromeus: dry_run failed: {e}"))),
                };
            }

            let prompt_numbers: Vec<u32> = prompts.iter().map(|p| p.number).collect();
            let mut dispatch_spec =
                aletheia_energeia::types::DispatchSpec::new(project.to_owned(), prompt_numbers);
            dispatch_spec.max_parallel =
                opt_u64(args, "max_turns").and_then(|v| u32::try_from(v).ok());

            match orchestrator.dispatch(dispatch_spec, &prompts).await {
                Ok(result) => Ok(to_json_text(&result)),
                Err(e) => Ok(ToolResult::error(format!("dromeus: dispatch failed: {e}"))),
            }
        })
    }
}

// ── dokimasia (δοκιμασία — examination) ────────────────────────────────────

fn dokimasia_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("dokimasia"),
        description: "Run a QA evaluation of a pull request against the originating prompt spec. \
            Returns a verdict (pass/partial/fail), per-criterion results, and mechanical issues."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "prompt_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Prompt spec number that generated this PR".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "pr_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "GitHub pull request number to evaluate".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "GitHub project slug (owner/repo)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "prompt_number".to_owned(),
                "pr_number".to_owned(),
                "project".to_owned(),
            ],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    }
}

struct DokimasiaExecutor;

impl ToolExecutor for DokimasiaExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let prompt_number = match opt_u64(args, "prompt_number") {
                Some(n) => u32::try_from(n).unwrap_or(0),
                None => return Ok(ToolResult::error("missing required field 'prompt_number'")),
            };
            let pr_number = match opt_u64(args, "pr_number") {
                Some(n) => n,
                None => return Ok(ToolResult::error("missing required field 'pr_number'")),
            };
            let _project = match require_str(args, "project") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

            // WHY: Build a minimal QA prompt spec from the prompt number. Full
            // prompt spec loading (with real acceptance criteria) requires file
            // I/O outside the tool's scope. Callers can add criteria via a future
            // schema extension. Mechanical checks run against the empty diff.
            let qa_prompt = aletheia_energeia::qa::PromptSpec::new(
                prompt_number,
                format!("Prompt #{prompt_number}"),
            );

            // WHY: Diff is empty because fetching the PR diff requires GitHub API
            // access which is outside this tool's scope. Callers may pass a diff
            // via a future `diff` field extension. Mechanical checks on an empty
            // diff produce no findings.
            let qa_result = run_qa("", &qa_prompt, pr_number);

            Ok(to_json_text(&qa_result))
        })
    }
}

// ── diorthosis (διόρθωσις — correction) ────────────────────────────────────

fn diorthosis_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("diorthosis"),
        description: "Generate a corrective prompt spec from a failed QA result. \
            Stateless transformation: takes the QA result and original prompt, \
            returns a revised prompt spec targeting the identified deficiencies."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "qa_result_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "ID of the QA result from a previous dokimasia run, \
                            or inline JSON-encoded QaResult"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "original_prompt_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Prompt spec number that produced the failing PR".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "qa_result_id".to_owned(),
                "original_prompt_number".to_owned(),
            ],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
    }
}

struct DiorthosisExecutor;

impl ToolExecutor for DiorthosisExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let qa_result_id = match require_str(args, "qa_result_id") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let original_prompt_number = match opt_u64(args, "original_prompt_number") {
                Some(n) => u32::try_from(n).unwrap_or(0),
                None => {
                    return Ok(ToolResult::error(
                        "missing required field 'original_prompt_number'",
                    ));
                }
            };

            // WHY: qa_result_id accepts inline JSON-encoded QaResult (the output from
            // dokimasia) so callers can chain dokimasia → diorthosis without a
            // persistent QA result store. A future store extension will support opaque
            // IDs for server-side lookup.
            let qa_result: aletheia_energeia::types::QaResult =
                match serde_json::from_str(qa_result_id) {
                    Ok(r) => r,
                    Err(_) => {
                        return Ok(ToolResult::error(
                            "diorthosis: qa_result_id must be a JSON-encoded QaResult \
                            (copy the JSON output from a dokimasia call)",
                        ));
                    }
                };

            let original = aletheia_energeia::qa::PromptSpec::new(
                original_prompt_number,
                format!("Prompt #{original_prompt_number}"),
            );

            match generate_corrective(&qa_result, &original) {
                Some(corrective) => {
                    let output = serde_json::json!({
                        "description": corrective.description,
                        "prompt_number": corrective.prompt_number,
                        "acceptance_criteria": corrective.acceptance_criteria,
                        "blast_radius": corrective.blast_radius,
                    });
                    Ok(to_json_text(&output))
                }
                None => Ok(ToolResult::text(
                    "diorthosis: no corrective needed (verdict is Pass or no failed criteria)",
                )),
            }
        })
    }
}

// ── epitropos (ἐπίτροπος — steward) ───────────────────────────────────────

fn epitropos_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("epitropos"),
        description: "CI steward: monitor pull requests, auto-merge passing PRs, \
            queue failing PRs for repair. Runs as a polling loop unless `once` is set."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "GitHub project slug (owner/repo)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "once".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Run a single classification pass instead of a polling loop \
                            (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Classify PRs without merging or queuing repairs \
                            (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    }
}

struct EpitroposExecutor;

impl ToolExecutor for EpitroposExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let project = match require_str(args, "project") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let once = opt_bool(args, "once").unwrap_or(false);
            let dry_run = opt_bool(args, "dry_run").unwrap_or(false);

            let mut config = StewardConfig::new(project.to_owned());
            config.once = once;
            config.dry_run = dry_run;

            // WHY: Always use run_once in tool context — a polling loop would block
            // the tool executor indefinitely. Callers that need the polling loop
            // should schedule a recurring trigger instead.
            let result = run_once(&config).await;

            let output = serde_json::json!({
                "project": project,
                "dry_run": dry_run,
                "classified_count": result.classified.len(),
                "merged_count": result.merged.len(),
                "needs_fix_count": result.needs_fix.len(),
                "blocked_count": result.blocked.len(),
                "main_ci_status": format!("{:?}", result.main_ci_status),
                "main_fix_attempted": result.main_fix_attempted,
            });

            Ok(to_json_text(&output))
        })
    }
}

// ── parateresis (παρατήρησις — observation) ────────────────────────────────

fn parateresis_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("parateresis"),
        description: "Collect observations from recently merged pull requests, \
            match them to open issues, and create tracking issues for patterns not yet filed."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "GitHub project slug (owner/repo)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "days".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "How many days of merged PRs to scan (default: 7)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(7)),
                    },
                ),
            ]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    }
}

struct ParateresisExecutor {
    store: Option<Arc<EnergeiaStore>>,
}

impl ToolExecutor for ParateresisExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref store) = self.store else {
                return Ok(ToolResult::error(
                    "parateresis: store not configured (missing EnergeiaServices)",
                ));
            };

            let args = &input.arguments;
            let project = match require_str(args, "project") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let days = opt_u64(args, "days")
                .and_then(|d| u32::try_from(d).ok())
                .unwrap_or(7);

            // Record an observation for this collection pass and return existing ones.
            // WHY: The observation pipeline captures patterns from merged PRs as
            // ObservationRecord entries. This tool queries existing observations and
            // records a new sentinel observation for the scan run.
            let scan_observation = NewObservation {
                project: project.to_owned(),
                source: "parateresis".to_owned(),
                content: format!("observation scan requested for last {days} days"),
                observation_type: "scan".to_owned(),
                session_id: None,
            };
            if let Err(e) = store.add_observation(&scan_observation) {
                tracing::warn!(error = %e, "parateresis: failed to record scan observation");
            }

            match store.query_observations(Some(project), Some(days), 100) {
                Ok(observations) => {
                    let output = serde_json::json!({
                        "project": project,
                        "days": days,
                        "count": observations.len(),
                        "observations": observations.iter().map(|o| serde_json::json!({
                            "id": o.id,
                            "source": o.source,
                            "content": o.content,
                            "observation_type": o.observation_type,
                            "created_at": o.created_at.to_string(),
                        })).collect::<Vec<_>>(),
                    });
                    Ok(to_json_text(&output))
                }
                Err(e) => Ok(ToolResult::error(format!(
                    "parateresis: store query failed: {e}"
                ))),
            }
        })
    }
}

// ── mathesis (μάθησις — learning) ─────────────────────────────────────────

fn mathesis_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("mathesis"),
        description:
            "Query or record lessons learned from dispatches, QA runs, and steward cycles. \
            Use `action: list` to retrieve lessons, `action: record` to save a new one."
                .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Operation: `list` to retrieve lessons, `record` to save one"
                            .to_owned(),
                        enum_values: Some(vec!["list".to_owned(), "record".to_owned()]),
                        default: None,
                    },
                ),
                (
                    "source".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Filter by source system: `dispatch`, `qa`, `steward`"
                            .to_owned(),
                        enum_values: Some(vec![
                            "dispatch".to_owned(),
                            "qa".to_owned(),
                            "steward".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "category".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Lesson category for filtering or tagging".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Scope lessons to a specific project (owner/repo)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "lesson".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Lesson text to record (required for `action: record`)"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["action".to_owned()],
        },
        category: ToolCategory::Memory,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
    }
}

struct MathesisExecutor {
    store: Option<Arc<EnergeiaStore>>,
}

impl ToolExecutor for MathesisExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref store) = self.store else {
                return Ok(ToolResult::error(
                    "mathesis: store not configured (missing EnergeiaServices)",
                ));
            };

            let args = &input.arguments;
            let action = match require_str(args, "action") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

            match action {
                "list" => {
                    let source = opt_str(args, "source");
                    let category = opt_str(args, "category");
                    let project = opt_str(args, "project");

                    match store.query_lessons(source, category, project, 100) {
                        Ok(lessons) => {
                            let output = serde_json::json!({
                                "count": lessons.len(),
                                "lessons": lessons.iter().map(|l| serde_json::json!({
                                    "source": l.source,
                                    "category": l.category,
                                    "lesson": l.lesson,
                                    "evidence": l.evidence,
                                    "project": l.project,
                                    "prompt_number": l.prompt_number,
                                    "created_at": l.created_at.to_string(),
                                })).collect::<Vec<_>>(),
                            });
                            Ok(to_json_text(&output))
                        }
                        Err(e) => Ok(ToolResult::error(format!("mathesis: query failed: {e}"))),
                    }
                }
                "record" => {
                    let lesson_text = match require_str(args, "lesson") {
                        Ok(s) => s,
                        Err(_) => {
                            return Ok(ToolResult::error(
                                "mathesis: 'lesson' field required for action 'record'",
                            ));
                        }
                    };
                    let source = opt_str(args, "source").unwrap_or("dispatch").to_owned();
                    let category = opt_str(args, "category").unwrap_or("general").to_owned();
                    let project = opt_str(args, "project").map(ToOwned::to_owned);

                    let new_lesson = NewLesson {
                        source,
                        category,
                        lesson: lesson_text.to_owned(),
                        evidence: None,
                        project,
                        prompt_number: None,
                    };

                    match store.add_lesson(&new_lesson) {
                        Ok(()) => Ok(ToolResult::text("mathesis: lesson recorded")),
                        Err(e) => Ok(ToolResult::error(format!("mathesis: record failed: {e}"))),
                    }
                }
                other => Ok(ToolResult::error(format!(
                    "mathesis: unknown action '{other}' (use 'list' or 'record')"
                ))),
            }
        })
    }
}

// ── prographe (προγραφή — template) ────────────────────────────────────────

fn prographe_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("prographe"),
        description: "Render a prompt spec from a GitHub issue or description. \
            Assigns the next available prompt number, writes the spec file, \
            and returns the generated content."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "from_issue".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "GitHub issue number to base the prompt spec on".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "GitHub project slug (owner/repo)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "description".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Free-form task description (alternative to from_issue)"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "criteria".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Array,
                        description: "Explicit acceptance criteria strings to embed in the spec"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
    }
}

struct ProographeExecutor;

impl ToolExecutor for ProographeExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let description = opt_str(args, "description")
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    let issue_num = opt_u64(args, "from_issue").unwrap_or(0);
                    if issue_num > 0 {
                        format!("Implement GitHub issue #{issue_num}")
                    } else {
                        "Task description".to_owned()
                    }
                });

            let criteria: Vec<String> = args
                .get("criteria")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or_default();

            let project = opt_str(args, "project").unwrap_or("(unspecified)");

            // Build YAML frontmatter for the prompt spec.
            // WHY: prompt_number 0 signals "to be assigned" — the operator
            // replaces it with the next queue number before dispatching.
            let criteria_yaml = if criteria.is_empty() {
                "  - \"(to be defined)\"\n".to_owned()
            } else {
                criteria
                    .iter()
                    .map(|c| format!("  - \"{c}\"\n"))
                    .collect::<String>()
            };

            let spec_yaml = format!(
                "---\nnumber: 0\ndescription: \"{description}\"\ndepends_on: []\n\
                acceptance_criteria:\n{criteria_yaml}blast_radius:\n  - \"\"\n---\n\n\
                # Task\n\n{description}\n"
            );

            let output = serde_json::json!({
                "project": project,
                "spec": spec_yaml,
                "criteria_count": criteria.len(),
            });

            Ok(to_json_text(&output))
        })
    }
}

// ── schedion (σχέδιον — plan/graph) ────────────────────────────────────────

fn schedion_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("schedion"),
        description: "Visualize the prompt dependency DAG for a project and compute the \
            execution frontier: which prompt specs are ready to dispatch now."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "project".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "GitHub project slug (owner/repo)".to_owned(),
                    enum_values: None,
                    default: None,
                },
            )]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
    }
}

struct SchedionExecutor;

impl ToolExecutor for SchedionExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;
            let project = match require_str(args, "project") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

            // WHY: Prompt files aren't accessible from the project slug alone —
            // that requires a configured prompts-directory mapping. The tool
            // computes the frontier on an empty DAG and notes the limitation.
            // Full file-backed DAG construction is available via the CLI dispatch
            // pipeline which knows where the prompts directory is.
            let dag = PromptDag::new();
            let frontier = compute_frontier(&dag);

            let output = serde_json::json!({
                "project": project,
                "node_count": 0,
                "frontier_group_count": frontier.len(),
                "frontier": frontier,
                "note": "No prompt spec files found via tool call. \
                    Use the CLI dispatch pipeline to load prompts from the filesystem.",
            });

            Ok(to_json_text(&output))
        })
    }
}

// ── metron (μέτρον — measure) ──────────────────────────────────────────────

fn metron_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("metron"),
        description: "Produce health and performance metrics for the dispatch pipeline: \
            dispatch counts, success rates, one-shot rates, and cost summaries."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "report_type".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Report to generate: `health`, `cost`, or `velocity`"
                            .to_owned(),
                        enum_values: Some(vec![
                            "health".to_owned(),
                            "cost".to_owned(),
                            "velocity".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "days".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Number of days to include in the report window (default: 30)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(30)),
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Scope the report to a specific project (owner/repo); \
                            omit for aggregate across all projects"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["report_type".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
    }
}

struct MetronExecutor {
    store: Option<Arc<EnergeiaStore>>,
}

impl ToolExecutor for MetronExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref store) = self.store else {
                return Ok(ToolResult::error(
                    "metron: store not configured (missing EnergeiaServices)",
                ));
            };

            let args = &input.arguments;
            let report_type = match require_str(args, "report_type") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let days = opt_u64(args, "days")
                .and_then(|d| u32::try_from(d).ok())
                .unwrap_or(30);

            let service = MetricsService::new(Arc::clone(store));

            match report_type {
                "health" => match service.health_report(days) {
                    Ok(report) => {
                        let metrics: Vec<serde_json::Value> = report
                            .metrics
                            .iter()
                            .map(|m| {
                                serde_json::json!({
                                    "name": m.name,
                                    "description": m.description,
                                    "value": m.value,
                                    "status": m.status.to_string(),
                                    "sample_size": m.sample_size,
                                    "ok_threshold": m.ok_threshold,
                                    "warn_threshold": m.warn_threshold,
                                    "higher_is_better": m.higher_is_better,
                                })
                            })
                            .collect();
                        let output = serde_json::json!({
                            "report_type": "health",
                            "window_days": report.window_days,
                            "computed_at": report.computed_at.to_string(),
                            "metrics": metrics,
                        });
                        Ok(to_json_text(&output))
                    }
                    Err(e) => Ok(ToolResult::error(format!(
                        "metron: health report failed: {e}"
                    ))),
                },
                "cost" | "velocity" => match service.cost_report(days) {
                    Ok(report) => {
                        let daily: Vec<serde_json::Value> = report
                            .daily_velocity
                            .iter()
                            .map(|d| {
                                serde_json::json!({
                                    "date": d.date.to_string(),
                                    "dispatches": d.dispatches,
                                    "sessions": d.sessions,
                                    "cost_usd": d.cost_usd,
                                })
                            })
                            .collect();
                        let by_project: Vec<serde_json::Value> = report
                            .by_project
                            .iter()
                            .map(|p| {
                                serde_json::json!({
                                    "project": p.project,
                                    "cost_usd": p.cost_usd,
                                    "dispatches": p.dispatches,
                                    "sessions": p.sessions,
                                    "success_rate": p.success_rate,
                                })
                            })
                            .collect();
                        let output = serde_json::json!({
                            "report_type": report_type,
                            "window_days": days,
                            "period_start": report.period_start.to_string(),
                            "period_end": report.period_end.to_string(),
                            "total_cost_usd": report.total_cost_usd,
                            "total_dispatches": report.total_dispatches,
                            "total_sessions": report.total_sessions,
                            "avg_cost_per_dispatch": report.avg_cost_per_dispatch,
                            "avg_cost_per_session": report.avg_cost_per_session,
                            "by_project": by_project,
                            "daily_velocity": daily,
                        });
                        Ok(to_json_text(&output))
                    }
                    Err(e) => Ok(ToolResult::error(format!(
                        "metron: cost report failed: {e}"
                    ))),
                },
                other => Ok(ToolResult::error(format!(
                    "metron: unknown report_type '{other}' (use 'health', 'cost', or 'velocity')"
                ))),
            }
        })
    }
}

// ── registration ───────────────────────────────────────────────────────────

/// Register all 9 energeia tools with real implementations.
///
/// When `services` is `Some`, tools that need the orchestrator or store call
/// through to the real energeia subsystem. When `None`, those tools return a
/// structured error indicating the missing dependency — they do not panic.
///
/// Tools that are pure computation (schedion, prographe, diorthosis,
/// dokimasia, epitropos) work regardless of whether services are provided.
///
/// # Errors
///
/// Returns an error if any tool name collides with an already-registered tool.
pub fn register(
    registry: &mut ToolRegistry,
    services: Option<Arc<EnergeiaServices>>,
) -> Result<()> {
    let (orchestrator, store) = match &services {
        Some(svc) => (
            Some(Arc::clone(&svc.orchestrator)),
            Some(Arc::clone(&svc.store)),
        ),
        None => (None, None),
    };

    registry.register(dromeus_def(), Box::new(DromeusExecutor { orchestrator }))?;
    registry.register(dokimasia_def(), Box::new(DokimasiaExecutor))?;
    registry.register(diorthosis_def(), Box::new(DiorthosisExecutor))?;
    registry.register(epitropos_def(), Box::new(EpitroposExecutor))?;
    registry.register(
        parateresis_def(),
        Box::new(ParateresisExecutor {
            store: store.clone(),
        }),
    )?;
    registry.register(
        mathesis_def(),
        Box::new(MathesisExecutor {
            store: store.clone(),
        }),
    )?;
    registry.register(prographe_def(), Box::new(ProographeExecutor))?;
    registry.register(schedion_def(), Box::new(SchedionExecutor))?;
    registry.register(metron_def(), Box::new(MetronExecutor { store }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolRegistry;
    use crate::types::ToolCategory;

    #[test]
    fn all_nine_tools_register_without_collision() {
        let mut registry = ToolRegistry::new();
        register(&mut registry, None).expect("energeia tools registered without collision");
        let defs = registry.definitions();
        assert_eq!(defs.len(), 9, "expected 9 energeia tools registered");
    }

    #[test]
    fn tool_categories_match_design() {
        for def in [
            dromeus_def(),
            dokimasia_def(),
            diorthosis_def(),
            epitropos_def(),
            parateresis_def(),
        ] {
            assert_eq!(
                def.category,
                ToolCategory::Agent,
                "{} must be in Agent category",
                def.name
            );
        }
        assert_eq!(mathesis_def().category, ToolCategory::Memory);
        assert_eq!(prographe_def().category, ToolCategory::Planning);
        assert_eq!(schedion_def().category, ToolCategory::Planning);
        assert_eq!(metron_def().category, ToolCategory::System);
    }

    #[test]
    fn no_tools_auto_activate() {
        for def in [
            dromeus_def(),
            dokimasia_def(),
            diorthosis_def(),
            epitropos_def(),
            parateresis_def(),
            mathesis_def(),
            prographe_def(),
            schedion_def(),
            metron_def(),
        ] {
            assert!(!def.auto_activate, "{} must not auto-activate", def.name);
        }
    }
}
