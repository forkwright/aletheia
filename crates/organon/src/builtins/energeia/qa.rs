//! QA tools (dokimasia + diorthosis).
//!
//! - dokimasia (δοκιμασία — examination): run QA evaluation of a PR
//! - diorthosis (διόρθωσις — correction): generate corrective prompt specs

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use energeia::qa::corrective::generate_corrective;
use energeia::qa::run_qa;
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::shared::{opt_str, opt_u64, require_str, to_json_text};

// ── dokimasia (δοκιμασία — examination) ────────────────────────────────────

pub(super) fn dokimasia_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("dokimasia"),
        description: "Run mechanical QA checks against a caller-provided pull-request diff. \
            Semantic acceptance-criteria evaluation requires orchestrator-side prompt and LLM \
            wiring; empty diffs return no-work rather than a pass verdict."
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
                        ..Default::default()
                    },
                ),
                (
                    "pr_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "GitHub pull request number to evaluate".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional GitHub project slug (owner/repo), reserved for \
                            future QA result persistence"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "diff".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Unified PR diff to evaluate; empty diffs return no-work."
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec![
                "prompt_number".to_owned(),
                "pr_number".to_owned(),
                "diff".to_owned(),
            ],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Verify],
    }
}

pub(super) struct DokimasiaExecutor;

impl ToolExecutor for DokimasiaExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let prompt_number = match opt_u64(args, "prompt_number") {
                Some(n) => u32::try_from(n).unwrap_or(0),
                None => return Ok(ToolResult::error("missing required field 'prompt_number'")),
            };
            let Some(pr_number) = opt_u64(args, "pr_number") else {
                return Ok(ToolResult::error("missing required field 'pr_number'"));
            };
            let project = opt_str(args, "project");
            if project.is_some_and(|p| p.trim().is_empty()) {
                return Ok(ToolResult::error("field 'project' must not be empty"));
            }
            let diff = match require_str(args, "diff") {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::error(e)),
            };
            if diff.trim().is_empty() {
                let output = serde_json::json!({
                    "status": "no_work",
                    "reason": "no diff to QA",
                    "project": project,
                    "prompt_number": prompt_number,
                    "pr_number": pr_number,
                });
                return Ok(to_json_text(&output));
            }

            // WHY: Build a minimal QA prompt spec from the prompt number. Full
            // prompt spec loading (with real acceptance criteria) requires file
            // I/O outside the tool's scope. Mechanical checks run against the
            // caller-provided diff.
            let qa_prompt =
                energeia::qa::PromptSpec::new(prompt_number, format!("Prompt #{prompt_number}"));

            // WHY: No LLM provider available in the tool context — runs
            // mechanical-only evaluation. Semantic evaluation requires the
            // orchestrator which has access to hermeneus providers.
            let qa_result = run_qa(diff, &qa_prompt, pr_number, None).await;

            // WHY(#6419): a mechanically-clean QA verdict on a real diff is
            // the one place in production where post-merge lesson extraction
            // has an actual diff to work with. Gated on Pass|NeedsReview
            // (never Fail|Partial) so corrective-loop noise does not flood
            // the graph — NeedsReview is included deliberately because this
            // tool always builds a criteria-less PromptSpec (see WHY above),
            // so a clean diff verdicts NeedsReview, not Pass; gating on Pass
            // alone would make this call structurally unreachable, exactly
            // the "wired but never fires" shape #6419 is about. Persisting
            // is best-effort — a lesson-graph write failure must never fail
            // the QA check it is riding on, and the tool's output contract
            // (raw QaResult JSON, consumed verbatim by diorthosis's
            // qa_result_id chaining) must not change.
            if matches!(
                qa_result.verdict,
                energeia::types::QaVerdict::Pass | energeia::types::QaVerdict::NeedsReview
            ) {
                persist_pr_lesson(ctx, diff, &qa_prompt.description, pr_number, project).await;
            }

            Ok(to_json_text(&qa_result))
        })
    }
}

/// Best-effort post-merge lesson persist via the knowledge service, if configured.
///
/// Returns whether a lesson was actually written. Failures are logged and
/// swallowed: `dokimasia`'s contract is the QA verdict, not knowledge-graph
/// persistence.
async fn persist_pr_lesson(
    ctx: &ToolContext,
    diff: &str,
    pr_title: &str,
    pr_number: u64,
    project: Option<&str>,
) -> bool {
    let Some(knowledge) = ctx.services.as_deref().and_then(|s| s.knowledge.as_ref()) else {
        return false;
    };
    let pr_number_u32 = u32::try_from(pr_number).ok();
    let source = format!("pr-merge:{pr_number}");
    match knowledge
        .persist_pr_lesson(diff, pr_title, pr_number_u32, ctx.nous_id.as_str(), &source)
        .await
    {
        Ok(summary) => {
            tracing::info!(
                pr_number,
                project = project.unwrap_or("unknown"),
                facts_inserted = summary.facts_inserted,
                entities_inserted = summary.entities_inserted,
                relationships_inserted = summary.relationships_inserted,
                causal_edges_inserted = summary.causal_edges_inserted,
                "dokimasia: post-merge lesson persisted"
            );
            summary.facts_inserted + summary.entities_inserted > 0
        }
        Err(e) => {
            tracing::warn!(pr_number, error = %e, "dokimasia: lesson persist failed");
            false
        }
    }
}

// ── diorthosis (διόρθωσις — correction) ────────────────────────────────────

pub(super) fn diorthosis_def() -> ToolDef {
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
                        ..Default::default()
                    },
                ),
                (
                    "original_prompt_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Prompt spec number that produced the failing PR".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
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
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Verify, ToolTag::Edit],
    }
}

pub(super) struct DiorthosisExecutor;

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
                Err(e) => return Ok(ToolResult::error(e)),
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
            // dokimasia) so callers can chain dokimasia -> diorthosis without a
            // persistent QA result store. A future store extension will support opaque
            // IDs for server-side lookup.
            let qa_result: energeia::types::QaResult = match serde_json::from_str(qa_result_id) {
                Ok(r) => r,
                Err(_) => {
                    return Ok(ToolResult::error(
                        "diorthosis: qa_result_id must be a JSON-encoded QaResult \
                            (copy the JSON output from a dokimasia call)",
                    ));
                }
            };

            let original = energeia::qa::PromptSpec::new(
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::result::Result;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId};

    use crate::error::KnowledgeAdapterError;
    use crate::testing::install_crypto_provider;
    use crate::types::{
        DatalogResult, FactSummary, KnowledgeSearchService, LessonPersistSummary, MemoryResult,
        ServerToolConfig, ToolHttpClients, ToolServices,
    };

    use super::*;

    /// Test double that only implements `persist_pr_lesson` for real; every
    /// other method panics if exercised, so a regression that routes
    /// dokimasia through a different knowledge-service call fails loudly
    /// instead of silently passing.
    #[derive(Default)]
    struct FakeKnowledgeService {
        persist_calls: AtomicU32,
    }

    impl KnowledgeSearchService for FakeKnowledgeService {
        fn search(
            &self,
            _query: &str,
            _nous_id: &str,
            _limit: usize,
        ) -> Pin<
            Box<dyn Future<Output = Result<Vec<MemoryResult>, KnowledgeAdapterError>> + Send + '_>,
        > {
            unimplemented!("dokimasia never calls search")
        }

        fn correct_fact(
            &self,
            _fact_id: &str,
            _new_content: &str,
            _nous_id: &str,
        ) -> Pin<Box<dyn Future<Output = Result<String, KnowledgeAdapterError>> + Send + '_>>
        {
            unimplemented!("dokimasia never calls correct_fact")
        }

        fn retract_fact(
            &self,
            _fact_id: &str,
            _reason: Option<&str>,
        ) -> Pin<Box<dyn Future<Output = Result<(), KnowledgeAdapterError>> + Send + '_>> {
            unimplemented!("dokimasia never calls retract_fact")
        }

        fn audit_facts(
            &self,
            _nous_id: Option<&str>,
            _since: Option<&str>,
            _limit: usize,
        ) -> Pin<
            Box<dyn Future<Output = Result<Vec<FactSummary>, KnowledgeAdapterError>> + Send + '_>,
        > {
            unimplemented!("dokimasia never calls audit_facts")
        }

        fn forget_fact(
            &self,
            _fact_id: &str,
            _reason: &str,
        ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>>
        {
            unimplemented!("dokimasia never calls forget_fact")
        }

        fn unforget_fact(
            &self,
            _fact_id: &str,
        ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>>
        {
            unimplemented!("dokimasia never calls unforget_fact")
        }

        fn datalog_query(
            &self,
            _query: &str,
            _params: Option<serde_json::Value>,
            _timeout_secs: Option<f64>,
            _row_limit: Option<usize>,
        ) -> Pin<Box<dyn Future<Output = Result<DatalogResult, KnowledgeAdapterError>> + Send + '_>>
        {
            unimplemented!("dokimasia never calls datalog_query")
        }

        fn find_skill_by_name(
            &self,
            _nous_id: &str,
            _skill_name: &str,
        ) -> Pin<Box<dyn Future<Output = Result<Option<String>, KnowledgeAdapterError>> + Send + '_>>
        {
            unimplemented!("dokimasia never calls find_skill_by_name")
        }

        fn persist_pr_lesson(
            &self,
            diff: &str,
            _pr_title: &str,
            _pr_number: Option<u32>,
            _nous_id: &str,
            _source: &str,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<LessonPersistSummary, KnowledgeAdapterError>>
                    + Send
                    + '_,
            >,
        > {
            self.persist_calls.fetch_add(1, Ordering::SeqCst);
            let non_empty = !diff.is_empty();
            Box::pin(async move {
                Ok(LessonPersistSummary {
                    facts_inserted: usize::from(non_empty),
                    entities_inserted: usize::from(non_empty),
                    relationships_inserted: 0,
                    causal_edges_inserted: 0,
                })
            })
        }
    }

    fn ctx_with_knowledge(knowledge: Arc<dyn KnowledgeSearchService>) -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                working_checkpoint_store: None,
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: Some(knowledge),
                http_clients: ToolHttpClients::for_tests(),
                secret_vault: hermeneus::secret::SecretVault::new(),
                lazy_tool_catalog: vec![],
                server_tool_config: ServerToolConfig::default(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn ctx_without_services() -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn dokimasia_input(diff: &str) -> ToolInput {
        ToolInput {
            name: ToolName::from_static("dokimasia"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({
                "prompt_number": 1,
                "pr_number": 42,
                "project": "acme/test",
                "diff": diff,
            }),
        }
    }

    #[tokio::test]
    async fn dokimasia_persists_a_lesson_when_the_diff_is_mechanically_clean() {
        let service = Arc::new(FakeKnowledgeService::default());
        // WHY the method form and not `Arc::clone(&service)`: the annotation is what
        // performs the unsizing coercion, and a coercion site needs a value to coerce.
        // `Arc::clone` is an associated fn whose T is fixed by the return type, so under
        // this annotation it demands `&Arc<dyn KnowledgeSearchService>` and rejects
        // `&Arc<FakeKnowledgeService>` outright. `.clone()` resolves on the concrete Arc
        // and the annotation then unsizes the result, which is what was meant. `service`
        // stays concrete for the persist_calls assertion below.
        let knowledge: Arc<dyn KnowledgeSearchService> = service.clone();
        let ctx = ctx_with_knowledge(knowledge);

        // No acceptance criteria are ever supplied by this tool (see the WHY
        // above `run_qa` in `DokimasiaExecutor::execute`), so a clean diff
        // verdicts NeedsReview, not Pass — persistence must fire on both.
        let input = dokimasia_input("diff --git a/src/lib.rs b/src/lib.rs\n+fn added() {}\n");
        let result = DokimasiaExecutor
            .execute(&input, &ctx)
            .await
            .expect("execute");
        assert!(
            !result.is_error,
            "expected a non-error QA result: {}",
            result.content.text_summary()
        );

        assert_eq!(
            service.persist_calls.load(Ordering::SeqCst),
            1,
            "dokimasia must persist a lesson for a mechanically-clean diff — \
             regressing this to zero is the unwired-pipeline defect from #6419"
        );
    }

    #[tokio::test]
    async fn dokimasia_does_not_persist_a_lesson_when_mechanical_issues_are_found() {
        let service = Arc::new(FakeKnowledgeService::default());
        // WHY the method form and not `Arc::clone(&service)`: the annotation is what
        // performs the unsizing coercion, and a coercion site needs a value to coerce.
        // `Arc::clone` is an associated fn whose T is fixed by the return type, so under
        // this annotation it demands `&Arc<dyn KnowledgeSearchService>` and rejects
        // `&Arc<FakeKnowledgeService>` outright. `.clone()` resolves on the concrete Arc
        // and the annotation then unsizes the result, which is what was meant. `service`
        // stays concrete for the persist_calls assertion below.
        let knowledge: Arc<dyn KnowledgeSearchService> = service.clone();
        let ctx = ctx_with_knowledge(knowledge);

        // Triggers the `#[allow()]` anti-pattern mechanical check, forcing a
        // Fail verdict (see energeia::qa::verdict::determine_verdict).
        let input = dokimasia_input("+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+#[allow(dead_code)]\n");
        let result = DokimasiaExecutor
            .execute(&input, &ctx)
            .await
            .expect("execute");
        assert!(
            !result.is_error,
            "mechanical fail is a QA result, not a tool error"
        );

        assert_eq!(
            service.persist_calls.load(Ordering::SeqCst),
            0,
            "corrective-loop noise must not be persisted as institutional pattern"
        );
    }

    #[tokio::test]
    async fn dokimasia_degrades_silently_when_no_knowledge_service_is_configured() {
        let ctx = ctx_without_services();
        let input = dokimasia_input("diff --git a/src/lib.rs b/src/lib.rs\n+fn added() {}\n");
        let result = DokimasiaExecutor
            .execute(&input, &ctx)
            .await
            .expect("execute");
        assert!(
            !result.is_error,
            "missing knowledge service must not fail the QA check: {}",
            result.content.text_summary()
        );
    }
}
