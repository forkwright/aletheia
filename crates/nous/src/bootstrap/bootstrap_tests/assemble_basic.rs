//! Basic bootstrap assembly tests.

#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#![expect(clippy::expect_used, reason = "test assertions")]

use std::fs;
use std::future::Future;
use std::pin::Pin;

use organon::surface::SurfaceInputs;
use organon::types::{ToolGroupId, ToolGroupPolicy};
use tempfile::TempDir;

use super::super::*;
use super::{default_budget, setup_oikos};

#[tokio::test]
async fn assemble_with_required_only() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "I am a test agent.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result.system_prompt.contains("I am a test agent."),
        "system prompt should include SOUL.md content"
    );
    assert_eq!(
        result.sections_included,
        vec!["SOUL.md", "output-style"],
        "SOUL.md and output-style should be included when it is the only file"
    );
    assert!(
        result.sections_dropped.is_empty(),
        "no sections should be dropped when only required file is present"
    );
}

#[tokio::test]
async fn assemble_with_tool_summary_extra_includes_live_registry_tools() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "I am a test agent.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();
    let mut registry = organon::registry::ToolRegistry::new();
    registry
        .register(test_tool_def("read_file"), Box::new(NoopToolExecutor))
        .expect("register tool");
    let estimator = crate::budget::CharEstimator::default();
    let active = std::collections::HashSet::new();
    let policy = ToolGroupPolicy::groups(vec![ToolGroupId::Read]);
    let surface = registry.effective_surface(SurfaceInputs {
        policy: &policy,
        allowlist: None,
        active: &active,
        server_tools: &[],
        server_tool_config: None,
    });
    let tool_section =
        tools::tool_summary_bootstrap_section(&surface, &estimator).expect("tool summary section");

    let result = assembler
        .assemble_with_extra("test", &mut budget, vec![tool_section])
        .await
        .expect("assemble should succeed");

    assert!(
        result
            .sections_included
            .iter()
            .any(|s| s == "tools-summary"),
        "tool summary section should participate in bootstrap packing"
    );
    assert!(
        result
            .system_prompt
            .contains("- **read_file**: Read a file from disk."),
        "system prompt should include the compact live-registry tool summary"
    );
}

fn test_tool_def(name: &str) -> organon::types::ToolDef {
    organon::types::ToolDef {
        name: koina::id::ToolName::new(name).expect("valid tool name"),
        description: "Read a file from disk. Extra details.".to_owned(),
        extended_description: None,
        input_schema: organon::types::InputSchema {
            properties: indexmap::IndexMap::new(),
            required: Vec::new(),
        },
        category: organon::types::ToolCategory::Workspace,
        reversibility: organon::types::Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![organon::types::ToolGroupId::Read],
        tags: Vec::new(),
    }
}

struct NoopToolExecutor;

impl organon::registry::ToolExecutor for NoopToolExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a organon::types::ToolInput,
        _ctx: &'a organon::types::ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<organon::types::ToolResult>> + Send + 'a>>
    {
        Box::pin(async { Ok(organon::types::ToolResult::text("ok")) })
    }
}

#[tokio::test]
async fn assemble_missing_required_errors() {
    let (_dir, oikos) = setup_oikos("test", &[("USER.md", "some user info")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let err = assembler
        .assemble("test", &mut budget)
        .await
        .expect_err("assemble with invalid config should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("SOUL.md"),
        "error should mention SOUL.md: {msg}"
    );
}

#[tokio::test]
async fn assemble_missing_optional_skips() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert_eq!(
        result.sections_included,
        vec!["SOUL.md", "output-style"],
        "SOUL.md and output-style should be included when optional files are absent"
    );
    assert!(
        result.sections_dropped.is_empty(),
        "missing optional files should be silently skipped, not dropped"
    );
}

#[tokio::test]
async fn assemble_priority_ordering() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("MEMORY.md", "memory notes"),
            ("GOALS.md", "goals"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    // WHY: Required (SOUL) before Important (GOALS) before Flexible (MEMORY)
    let soul_pos = result
        .sections_included
        .iter()
        .position(|s| s == "SOUL.md")
        .expect("SOUL.md should be in sections_included");
    let goals_pos = result
        .sections_included
        .iter()
        .position(|s| s == "GOALS.md")
        .expect("GOALS.md should be in sections_included");
    let memory_pos = result
        .sections_included
        .iter()
        .position(|s| s == "MEMORY.md")
        .expect("MEMORY.md should be in sections_included");
    assert!(
        soul_pos < goals_pos,
        "SOUL.md (Required) should appear before GOALS.md (Important)"
    );
    assert!(
        goals_pos < memory_pos,
        "GOALS.md (Important) should appear before MEMORY.md (Flexible)"
    );
}

#[tokio::test]
async fn assemble_all_files_present() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("USER.md", "user info"),
            ("AGENTS.md", "team topology"),
            ("GOALS.md", "goals"),
            ("TOOLS.md", "tool list"),
            ("MEMORY.md", "memory"),
            ("IDENTITY.md", "name and emoji"),
            ("PROSOCHE.md", "checklist"),
            ("CONTEXT.md", "runtime config"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert_eq!(
        result.sections_included.len(),
        10,
        "all 9 workspace sections + output-style should be included when budget allows"
    );
    assert!(
        result.total_tokens > 0,
        "total token count should be greater than zero when sections are included"
    );
}

#[tokio::test]
async fn assemble_empty_file_skipped() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("AGENTS.md", ""),
            ("GOALS.md", "   \n  \n  "), // whitespace-only
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert_eq!(
        result.sections_included,
        vec!["SOUL.md", "output-style"],
        "empty and whitespace-only sections should be skipped, output-style always present"
    );
}

#[tokio::test]
async fn assemble_memory_truncated() {
    let large_memory = "## Recent\nNew stuff here.\n## Old\nOld stuff here that is much longer and should be truncated when the budget is tight. ".repeat(50);
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "identity"), ("MEMORY.md", &large_memory)],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = TokenBudget::new(100_000, 0.0, 0, 500);

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result.sections_included.contains(&"MEMORY.md".to_owned()),
        "MEMORY.md should be included even when truncated"
    );
    assert!(
        result.sections_truncated.contains(&"MEMORY.md".to_owned()),
        "MEMORY.md should be recorded as truncated when it exceeded the budget"
    );
    assert!(
        result
            .system_prompt
            .contains("[truncated for token budget]"),
        "truncated section should include a truncation marker in the system prompt"
    );
}

#[tokio::test]
async fn assemble_optional_dropped() {
    let large_soul = "x".repeat(2000); // ~500 tokens at 4 chars/token
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", &large_soul), ("MEMORY.md", "memory notes")],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = TokenBudget::new(100_000, 0.0, 0, 500);

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result.sections_included.contains(&"SOUL.md".to_owned()),
        "SOUL.md should always be included as a required section"
    );
    assert!(
        result.sections_dropped.contains(&"MEMORY.md".to_owned()),
        "MEMORY.md should be dropped when the budget is fully consumed by required sections"
    );
}

#[tokio::test]
async fn assemble_budget_consumed_correctly() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity"), ("USER.md", "user info")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert_eq!(
        budget.consumed(),
        result.total_tokens,
        "budget consumed should match the total tokens reported in the result"
    );
    assert!(
        result.total_tokens > 0,
        "total token count should be greater than zero when sections are included"
    );
}

#[tokio::test]
async fn assemble_cascade_nous_tier() {
    let (_dir, oikos) = setup_oikos("syn", &[("SOUL.md", "I am Syn.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("syn", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result.system_prompt.contains("I am Syn."),
        "system prompt should include SOUL.md content from the nous tier"
    );
}

#[tokio::test]
async fn assemble_cascade_theke_fallback() {
    let (_dir, oikos) = setup_oikos(
        "syn",
        &[("SOUL.md", "identity"), ("theke:USER.md", "Alice T.")],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("syn", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result.system_prompt.contains("Alice T."),
        "system prompt should include USER.md content found in the theke tier"
    );
    assert!(
        result.sections_included.contains(&"USER.md".to_owned()),
        "USER.md should be listed as included when resolved from the theke tier"
    );
}

#[tokio::test]
async fn private_workspace_skips_theke_fallback() {
    let (_dir, oikos) = setup_oikos(
        "syn",
        &[("SOUL.md", "identity"), ("theke:USER.md", "Alice T.")],
    );
    let assembler = BootstrapAssembler::new(&oikos).with_private_workspace(true);
    let mut budget = default_budget();

    let result = assembler
        .assemble("syn", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        !result.system_prompt.contains("Alice T."),
        "private workspace should not read USER.md from the theke tier"
    );
    assert!(
        !result.sections_included.contains(&"USER.md".to_owned()),
        "private workspace should include only nous-local workspace files"
    );
}

#[tokio::test]
async fn assemble_nous_overrides_theke() {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("nous/syn")).expect("create nous dir");
    fs::create_dir_all(root.join("shared")).expect("create shared dir");
    fs::create_dir_all(root.join("theke")).expect("create theke dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("nous/syn/SOUL.md"), "nous-specific soul").expect("write nous soul");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("theke/SOUL.md"), "theke soul").expect("write theke soul");

    let oikos = Oikos::from_root(root);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("syn", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result.system_prompt.contains("nous-specific soul"),
        "nous-tier SOUL.md should override theke-tier SOUL.md in the system prompt"
    );
    assert!(
        !result.system_prompt.contains("theke soul"),
        "theke-tier SOUL.md should not appear when nous tier provides the same file"
    );
}

#[test]
fn truncate_section_aware() {
    let oikos = Oikos::from_root("/tmp/unused");
    let assembler = BootstrapAssembler::new(&oikos);

    let section = BootstrapSection {
        name: "MEMORY.md".to_owned(),
        priority: SectionPriority::Flexible,
        content: "## Section A\nContent A.\n## Section B\nContent B.\n## Section C\nContent C."
            .to_owned(),
        tokens: 100,
        truncatable: true,
        slot: BootstrapSlot::Memory,
    };

    let truncated = assembler.truncate_section(&section, 10);
    assert!(
        truncated.content.contains("Section C"),
        "newest section should be preserved when truncating to fit budget"
    );
    assert!(
        !truncated.content.contains("Section A"),
        "oldest section should be removed when truncating to fit budget"
    );
    assert!(
        truncated.content.contains("[truncated for token budget]"),
        "truncated content should include a truncation marker"
    );
}

#[test]
fn truncate_falls_back_to_lines() {
    let oikos = Oikos::from_root("/tmp/unused");
    let assembler = BootstrapAssembler::new(&oikos);

    let section = BootstrapSection {
        name: "MEMORY.md".to_owned(),
        priority: SectionPriority::Flexible,
        content: "Line one\nLine two\nLine three\nLine four\nLine five".to_owned(),
        tokens: 100,
        truncatable: true,
        slot: BootstrapSlot::Memory,
    };

    let truncated = assembler.truncate_by_lines(&section, 5);
    assert!(
        truncated.content.contains("Line five"),
        "last line should be preserved when truncating by lines"
    );
    assert!(
        !truncated.content.contains("Line one"),
        "first line should be removed when truncating by lines"
    );
    assert!(
        truncated.content.contains("[truncated for token budget]"),
        "line-truncated content should include a truncation marker"
    );
}

#[test]
fn pack_sections_to_bootstrap_converts_priorities() {
    let sections = [
        PackSection {
            name: "LOGIC.md".to_owned(),
            content: "Business logic content".to_owned(),
            priority: PackPriority::Required,
            truncatable: false,
            agents: vec![],
            pack_name: "test-pack".to_owned(),
        },
        PackSection {
            name: "GLOSSARY.md".to_owned(),
            content: "Term definitions".to_owned(),
            priority: PackPriority::Flexible,
            truncatable: true,
            agents: vec!["analyst".to_owned()],
            pack_name: "test-pack".to_owned(),
        },
    ];

    let refs: Vec<&PackSection> = sections.iter().collect();
    let result = pack_sections_to_bootstrap(&refs, &CharEstimator::default());

    assert_eq!(result.len(), 2, "both pack sections should be converted");
    assert_eq!(
        result[0].name, "[test-pack] LOGIC.md",
        "converted section name should include the pack name prefix"
    );
    assert_eq!(
        result[0].priority,
        SectionPriority::Required,
        "Required pack priority should map to Required bootstrap priority"
    );
    assert!(
        !result[0].truncatable,
        "non-truncatable pack section should remain non-truncatable after conversion"
    );
    assert_eq!(
        result[0].content, "Business logic content",
        "section content should be preserved unchanged after conversion"
    );
    assert!(
        result[0].tokens > 0,
        "token count should be estimated as greater than zero for non-empty content"
    );

    assert_eq!(
        result[1].name, "[test-pack] GLOSSARY.md",
        "converted section name should include the pack name prefix"
    );
    assert_eq!(
        result[1].priority,
        SectionPriority::Flexible,
        "Flexible pack priority should map to Flexible bootstrap priority"
    );
    assert!(
        result[1].truncatable,
        "truncatable pack section should remain truncatable after conversion"
    );
}

#[test]
fn pack_sections_to_bootstrap_empty_input() {
    let result = pack_sections_to_bootstrap(&[], &CharEstimator::default());
    assert!(
        result.is_empty(),
        "empty input should produce an empty list of bootstrap sections"
    );
}

#[tokio::test]
async fn required_section_over_budget_debt_tracked() {
    // WHY(#4623): A Required section (SOUL.md) that exceeds the system budget
    // must still be force-consumed so that downstream stages see the real
    // remaining budget. Use a budget so small it cannot fit SOUL.md.
    let large_soul = "x".repeat(2000); // ~500 tokens at 4 chars/token
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", &large_soul)]);
    let assembler = BootstrapAssembler::new(&oikos);
    // bootstrap_cap of 10 tokens is far too small for large_soul
    let mut budget = TokenBudget::new(200_000, 0.6, 16_384, 10);
    let expected_prefix = "x".repeat(20);

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed even when Required section overruns budget");

    assert!(
        result.system_prompt.contains(&expected_prefix),
        "Required SOUL.md must appear in system prompt despite budget exhaustion"
    );
    assert!(
        budget.consumed() > budget.system_budget(),
        "consumed tokens must exceed system_budget when Required section overruns"
    );
    assert!(
        budget.adjusted_history_budget() < budget.history_budget(),
        "history budget must be reduced to reflect Required-section over-budget debt"
    );
}

#[tokio::test]
async fn file_ref_expansion_debt_carried_into_budget() {
    // WHY(#4623): File-ref expansion can grow the system prompt beyond its
    // pre-expansion token estimate. The extra tokens must be force-consumed
    // so downstream stages allocate history against the true remaining budget.
    //
    // Setup: SOUL.md is a tiny file that embeds a large ref. The pre-expansion
    // token estimate (~3 tokens for the placeholder) passes the budget check,
    // but the post-expansion prompt is ~500 tokens. The bootstrap cap is set
    // to 100 tokens so the expansion pushes consumed past system_budget, making
    // the debt visible in adjusted_history_budget.
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("nous/test")).expect("create nous/test dir");
    // large_content is ~500 tokens at 4 chars/token
    let large_content = "y".repeat(2000);
    let expected_prefix = "y".repeat(20);
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("nous/test/ref.md"), &large_content).expect("write ref.md");
    // SOUL.md is tiny (pre-expansion ~3 tokens) but expands to ~500 tokens
    let soul_content = "ID:{{file:nous/test/ref.md}}";
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("nous/test/SOUL.md"), soul_content).expect("write SOUL.md");
    fs::create_dir_all(root.join("shared")).expect("create shared dir");
    fs::create_dir_all(root.join("theke")).expect("create theke dir");

    let oikos = Oikos::from_root(root);
    let assembler = BootstrapAssembler::new(&oikos);
    // bootstrap_cap of 100 lets tiny SOUL.md pass the pre-expansion check, but
    // the post-expansion prompt (~500 tokens) exceeds it, creating debt.
    let mut budget = TokenBudget::new(200_000, 0.6, 16_384, 100);

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed with file-ref expansion");

    assert!(
        result.system_prompt.contains(&expected_prefix),
        "expanded file-ref content must appear in the assembled system prompt"
    );
    // The expanded prompt is ~500 tokens; pre-expansion consumed was much less.
    // After our fix, force_consume carries the expansion delta, so consumed must
    // reflect the actual expanded prompt size.
    let estimator = crate::budget::CharEstimator::default();
    let actual_expanded_tokens = estimator.estimate(&result.system_prompt);
    assert!(
        budget.consumed() >= actual_expanded_tokens,
        "consumed ({}) must be at least the expanded prompt token estimate ({actual_expanded_tokens})",
        budget.consumed()
    );
    // Expansion pushed consumed past bootstrap_cap=100, so history must be reduced.
    assert!(
        budget.consumed() > budget.system_budget(),
        "consumed tokens must exceed system_budget after large file-ref expansion"
    );
    assert!(
        budget.adjusted_history_budget() < budget.history_budget(),
        "history budget must shrink to carry file-ref expansion debt into downstream stages"
    );
}
