#![expect(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::budget::TokenBudget;
use std::fs;
use tempfile::TempDir;

/// Create an oikos directory structure with the given files.
/// Files are placed in `nous/{nous_id}/` unless the filename starts with `theke:`.
fn setup_oikos(nous_id: &str, files: &[(&str, &str)]) -> (TempDir, Oikos) {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    // Create tier directories
    fs::create_dir_all(root.join(format!("nous/{nous_id}"))).unwrap();
    fs::create_dir_all(root.join("shared")).unwrap();
    fs::create_dir_all(root.join("theke")).unwrap();

    for (name, content) in files {
        if let Some(stripped) = name.strip_prefix("theke:") {
            fs::write(root.join("theke").join(stripped), content).unwrap();
        } else {
            fs::write(root.join(format!("nous/{nous_id}")).join(name), content).unwrap();
        }
    }

    let oikos = Oikos::from_root(root);
    (dir, oikos)
}

fn default_budget() -> TokenBudget {
    TokenBudget::new(200_000, 0.6, 16_384, 40_000)
}

// --- Assembly tests ---

#[tokio::test]
async fn assemble_with_required_only() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "I am a test agent.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler.assemble("test", &mut budget).await.unwrap();
    assert!(result.system_prompt.contains("I am a test agent."));
    assert_eq!(result.sections_included, vec!["SOUL.md"]);
    assert!(result.sections_dropped.is_empty());
}

#[tokio::test]
async fn assemble_missing_required_errors() {
    let (_dir, oikos) = setup_oikos("test", &[("USER.md", "some user info")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let err = assembler.assemble("test", &mut budget).await.unwrap_err();
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

    let result = assembler.assemble("test", &mut budget).await.unwrap();
    // Only SOUL.md included, others silently skipped
    assert_eq!(result.sections_included, vec!["SOUL.md"]);
    assert!(result.sections_dropped.is_empty());
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

    let result = assembler.assemble("test", &mut budget).await.unwrap();
    // Required (SOUL) before Important (GOALS) before Flexible (MEMORY)
    let soul_pos = result
        .sections_included
        .iter()
        .position(|s| s == "SOUL.md")
        .unwrap();
    let goals_pos = result
        .sections_included
        .iter()
        .position(|s| s == "GOALS.md")
        .unwrap();
    let memory_pos = result
        .sections_included
        .iter()
        .position(|s| s == "MEMORY.md")
        .unwrap();
    assert!(soul_pos < goals_pos);
    assert!(goals_pos < memory_pos);
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

    let result = assembler.assemble("test", &mut budget).await.unwrap();
    assert_eq!(result.sections_included.len(), 9);
    assert!(result.total_tokens > 0);
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

    let result = assembler.assemble("test", &mut budget).await.unwrap();
    assert_eq!(result.sections_included, vec!["SOUL.md"]);
}

#[tokio::test]
async fn assemble_memory_truncated() {
    // Create a large MEMORY.md that exceeds a small budget
    let large_memory = "## Recent\nNew stuff here.\n## Old\nOld stuff here that is much longer and should be truncated when the budget is tight. ".repeat(50);
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "identity"), ("MEMORY.md", &large_memory)],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    // Small budget: enough for SOUL.md but not full MNEME.md
    let mut budget = TokenBudget::new(100_000, 0.0, 0, 500);

    let result = assembler.assemble("test", &mut budget).await.unwrap();
    assert!(result.sections_included.contains(&"MEMORY.md".to_owned()));
    assert!(result.sections_truncated.contains(&"MEMORY.md".to_owned()));
    assert!(
        result
            .system_prompt
            .contains("[truncated for token budget]")
    );
}

#[tokio::test]
async fn assemble_optional_dropped() {
    // SOUL.md fills the entire budget, MEMORY.md gets dropped
    let large_soul = "x".repeat(2000); // ~500 tokens at 4 chars/token
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", &large_soul), ("MEMORY.md", "memory notes")],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = TokenBudget::new(100_000, 0.0, 0, 500);

    let result = assembler.assemble("test", &mut budget).await.unwrap();
    assert!(result.sections_included.contains(&"SOUL.md".to_owned()));
    assert!(result.sections_dropped.contains(&"MEMORY.md".to_owned()));
}

#[tokio::test]
async fn assemble_budget_consumed_correctly() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity"), ("USER.md", "user info")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler.assemble("test", &mut budget).await.unwrap();
    assert_eq!(budget.consumed(), result.total_tokens);
    assert!(result.total_tokens > 0);
}

#[tokio::test]
async fn assemble_cascade_nous_tier() {
    // File only in nous tier
    let (_dir, oikos) = setup_oikos("syn", &[("SOUL.md", "I am Syn.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler.assemble("syn", &mut budget).await.unwrap();
    assert!(result.system_prompt.contains("I am Syn."));
}

#[tokio::test]
async fn assemble_cascade_theke_fallback() {
    // USER.md only in theke (common pattern)
    let (_dir, oikos) = setup_oikos(
        "syn",
        &[("SOUL.md", "identity"), ("theke:USER.md", "Alice T.")],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler.assemble("syn", &mut budget).await.unwrap();
    assert!(result.system_prompt.contains("Alice T."));
    assert!(result.sections_included.contains(&"USER.md".to_owned()));
}

#[tokio::test]
async fn assemble_nous_overrides_theke() {
    // SOUL.md in both tiers: nous wins
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("nous/syn")).unwrap();
    fs::create_dir_all(root.join("shared")).unwrap();
    fs::create_dir_all(root.join("theke")).unwrap();
    fs::write(root.join("nous/syn/SOUL.md"), "nous-specific soul").unwrap();
    fs::write(root.join("theke/SOUL.md"), "theke soul").unwrap();

    let oikos = Oikos::from_root(root);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler.assemble("syn", &mut budget).await.unwrap();
    assert!(result.system_prompt.contains("nous-specific soul"));
    assert!(!result.system_prompt.contains("theke soul"));
}

// --- Truncation tests ---

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
    };

    // Budget enough for one section: newest (Section C) is kept, oldest dropped.
    let truncated = assembler.truncate_section(&section, 10);
    assert!(truncated.content.contains("Section C"));
    assert!(!truncated.content.contains("Section A"));
    assert!(truncated.content.contains("[truncated for token budget]"));
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
    };

    // Budget enough for ~1 line: newest ("Line five") is kept, oldest dropped.
    let truncated = assembler.truncate_by_lines(&section, 5);
    assert!(truncated.content.contains("Line five"));
    assert!(!truncated.content.contains("Line one"));
    assert!(truncated.content.contains("[truncated for token budget]"));
}

// --- Pack conversion tests ---

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
            agents: vec!["chiron".to_owned()],
            pack_name: "test-pack".to_owned(),
        },
    ];

    let refs: Vec<&PackSection> = sections.iter().collect();
    let result = pack_sections_to_bootstrap(&refs, &CharEstimator::default());

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "[test-pack] LOGIC.md");
    assert_eq!(result[0].priority, SectionPriority::Required);
    assert!(!result[0].truncatable);
    assert_eq!(result[0].content, "Business logic content");
    assert!(result[0].tokens > 0);

    assert_eq!(result[1].name, "[test-pack] GLOSSARY.md");
    assert_eq!(result[1].priority, SectionPriority::Flexible);
    assert!(result[1].truncatable);
}

#[test]
fn pack_sections_to_bootstrap_empty_input() {
    let result = pack_sections_to_bootstrap(&[], &CharEstimator::default());
    assert!(result.is_empty());
}

#[tokio::test]
async fn assemble_with_extra_includes_pack_sections() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "I am a test agent.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let extra = vec![BootstrapSection {
        name: "[pack] LOGIC.md".to_owned(),
        priority: SectionPriority::Important,
        content: "Domain logic from pack.".to_owned(),
        tokens: 6,
        truncatable: false,
    }];

    let result = assembler
        .assemble_with_extra("test", &mut budget, extra)
        .await
        .unwrap();
    assert!(result.system_prompt.contains("Domain logic from pack."));
    assert!(
        result
            .sections_included
            .contains(&"[pack] LOGIC.md".to_owned())
    );
    assert_eq!(result.sections_included.len(), 2);
}

#[tokio::test]
async fn assemble_with_extra_respects_priority_ordering() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let extra = vec![
        BootstrapSection {
            name: "optional-pack".to_owned(),
            priority: SectionPriority::Optional,
            content: "optional".to_owned(),
            tokens: 2,
            truncatable: true,
        },
        BootstrapSection {
            name: "important-pack".to_owned(),
            priority: SectionPriority::Important,
            content: "important".to_owned(),
            tokens: 2,
            truncatable: false,
        },
    ];

    let result = assembler
        .assemble_with_extra("test", &mut budget, extra)
        .await
        .unwrap();

    // SOUL.md (Required) < important-pack (Important) < optional-pack (Optional)
    let soul_pos = result
        .sections_included
        .iter()
        .position(|s| s == "SOUL.md")
        .unwrap();
    let important_pos = result
        .sections_included
        .iter()
        .position(|s| s == "important-pack")
        .unwrap();
    let optional_pos = result
        .sections_included
        .iter()
        .position(|s| s == "optional-pack")
        .unwrap();
    assert!(soul_pos < important_pos);
    assert!(important_pos < optional_pos);
}
