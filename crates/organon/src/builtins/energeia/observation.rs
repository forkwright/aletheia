//! Observation and learning tools (parateresis + mathesis).
//!
//! - parateresis (παρατήρησις — observation): collect observations from merged PRs
//! - mathesis (μάθησις — learning): query/record lessons learned

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexMap;

use aletheia_energeia::store::EnergeiaStore;
use aletheia_energeia::store::records::{NewLesson, NewObservation};
use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use super::shared::{opt_str, opt_u64, require_str, to_json_text};

// ── parateresis (παρατήρησις — observation) ────────────────────────────────

pub(super) fn parateresis_def() -> ToolDef {
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

pub(super) struct ParateresisExecutor {
    pub(super) store: Option<Arc<EnergeiaStore>>,
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

pub(super) fn mathesis_def() -> ToolDef {
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

pub(super) struct MathesisExecutor {
    pub(super) store: Option<Arc<EnergeiaStore>>,
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
