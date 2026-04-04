//! Energeia capability tool stubs.
//!
//! Registers the 9 energeia agent tools with stub executors that return
//! "energeia: not yet implemented". Real implementations land in AL-2060.
//!
//! Tools are gated behind the `energeia` Cargo feature flag so that crates
//! not shipping the energeia stack incur no compile cost.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

/// Stub executor that returns a "not yet implemented" error for all energeia tools.
struct EnergeiaStub {
    tool_name: &'static str,
}

impl ToolExecutor for EnergeiaStub {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        let name = self.tool_name;
        Box::pin(async move {
            tracing::warn!(
                tool = name,
                "energeia tool invoked before implementation (AL-2060)"
            );
            Ok(ToolResult::error(format!(
                "energeia: {name} is not yet implemented (AL-2060)"
            )))
        })
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
                        description: "ID of the QA result from a previous dokimasia run".to_owned(),
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

// ── registration ───────────────────────────────────────────────────────────

/// Register all 9 energeia tool stubs with the given registry.
///
/// All stubs return "energeia: not yet implemented" until AL-2060 lands
/// real implementations.
///
/// # Errors
///
/// Returns an error if any tool name collides with an already-registered tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(
        dromeus_def(),
        Box::new(EnergeiaStub {
            tool_name: "dromeus",
        }),
    )?;
    registry.register(
        dokimasia_def(),
        Box::new(EnergeiaStub {
            tool_name: "dokimasia",
        }),
    )?;
    registry.register(
        diorthosis_def(),
        Box::new(EnergeiaStub {
            tool_name: "diorthosis",
        }),
    )?;
    registry.register(
        epitropos_def(),
        Box::new(EnergeiaStub {
            tool_name: "epitropos",
        }),
    )?;
    registry.register(
        parateresis_def(),
        Box::new(EnergeiaStub {
            tool_name: "parateresis",
        }),
    )?;
    registry.register(
        mathesis_def(),
        Box::new(EnergeiaStub {
            tool_name: "mathesis",
        }),
    )?;
    registry.register(
        prographe_def(),
        Box::new(EnergeiaStub {
            tool_name: "prographe",
        }),
    )?;
    registry.register(
        schedion_def(),
        Box::new(EnergeiaStub {
            tool_name: "schedion",
        }),
    )?;
    registry.register(
        metron_def(),
        Box::new(EnergeiaStub {
            tool_name: "metron",
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolRegistry;

    #[test]
    fn all_nine_tools_register_without_collision() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("energeia tools registered without collision");
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

    #[tokio::test]
    async fn stubs_return_not_implemented() {
        use std::collections::HashSet;
        use std::sync::{Arc, RwLock};

        use aletheia_koina::id::{NousId, SessionId};

        use crate::types::ToolContext;

        let ctx = ToolContext {
            nous_id: NousId::new("test").expect("valid nous id"),
            session_id: SessionId::new(),
            workspace: std::path::PathBuf::from("/tmp"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        };

        let stub = EnergeiaStub {
            tool_name: "dromeus",
        };
        let input = crate::types::ToolInput {
            name: ToolName::from_static("dromeus"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({}),
        };

        let result = stub
            .execute(&input, &ctx)
            .await
            .expect("stub execute returns Ok");
        assert!(result.is_error, "stub must return an error result");
        let text = match &result.content {
            crate::types::ToolResultContent::Text(t) => t.clone(),
            _ => panic!("expected text content"),
        };
        assert!(
            text.contains("not yet implemented"),
            "error message must mention 'not yet implemented', got: {text}"
        );
    }
}
