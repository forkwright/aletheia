//! Pack section bootstrap assembly tests.

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;
use super::{default_budget, setup_oikos};
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
        slot: BootstrapSlot::Context,
    }];

    let result = assembler
        .assemble_with_extra("test", &mut budget, extra)
        .await
        .expect("assemble_with_extra should succeed");
    assert!(
        result.system_prompt.contains("Domain logic from pack."),
        "system prompt should include content from extra pack sections"
    );
    assert!(
        result
            .sections_included
            .contains(&"[pack] LOGIC.md".to_owned()),
        "extra pack section should be listed in sections_included"
    );
    assert_eq!(
        result.sections_included.len(),
        3,
        "SOUL.md, output-style, and the extra pack section should be included"
    );
}

#[tokio::test]
async fn assemble_with_extra_includes_system_prompt_additions() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "I am a test agent.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let extra = vec![BootstrapSection {
        name: "[pack] system-prompt".to_owned(),
        priority: SectionPriority::Important,
        content: "Always cite sources.".to_owned(),
        tokens: 4,
        truncatable: false,
        slot: BootstrapSlot::Context,
    }];

    let result = assembler
        .assemble_with_extra("test", &mut budget, extra)
        .await
        .expect("assemble_with_extra should succeed");
    assert!(
        result.system_prompt.contains("Always cite sources."),
        "system prompt should include system-prompt addition content"
    );
    assert!(
        result
            .sections_included
            .contains(&"[pack] system-prompt".to_owned()),
        "system-prompt addition should be listed in sections_included"
    );
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
            slot: BootstrapSlot::Context,
        },
        BootstrapSection {
            name: "important-pack".to_owned(),
            priority: SectionPriority::Important,
            content: "important".to_owned(),
            tokens: 2,
            truncatable: false,
            slot: BootstrapSlot::Context,
        },
    ];

    let result = assembler
        .assemble_with_extra("test", &mut budget, extra)
        .await
        .expect("assemble_with_extra should succeed");

    // WHY: SOUL.md (Required) < important-pack (Important) < optional-pack (Optional)
    let soul_pos = result
        .sections_included
        .iter()
        .position(|s| s == "SOUL.md")
        .expect("SOUL.md should be in sections_included");
    let important_pos = result
        .sections_included
        .iter()
        .position(|s| s == "important-pack")
        .expect("important-pack should be in sections_included");
    let optional_pos = result
        .sections_included
        .iter()
        .position(|s| s == "optional-pack")
        .expect("optional-pack should be in sections_included");
    assert!(
        soul_pos < important_pos,
        "SOUL.md (Required) should appear before important-pack (Important)"
    );
    assert!(
        important_pos < optional_pos,
        "important-pack (Important) should appear before optional-pack (Optional)"
    );
}

// --- Task hint classification tests ---

#[test]
fn classify_coding_hint() {
    assert_eq!(
        classify_task_hint("Please implement a new function to fix this bug"),
        TaskHint::Coding,
        "message with coding keywords should classify as Coding"
    );
}

#[test]
fn classify_research_hint() {
    assert_eq!(
        classify_task_hint("Research and analyze the performance of our search pipeline"),
        TaskHint::Research,
        "message with research keywords should classify as Research"
    );
}

#[test]
fn classify_planning_hint() {
    assert_eq!(
        classify_task_hint("Let's design a roadmap and plan the next milestone"),
        TaskHint::Planning,
        "message with planning keywords should classify as Planning"
    );
}

#[test]
fn classify_conversation_hint() {
    assert_eq!(
        classify_task_hint("hello"),
        TaskHint::Conversation,
        "short greeting should classify as Conversation"
    );
    assert_eq!(
        classify_task_hint("thanks"),
        TaskHint::Conversation,
        "short thanks should classify as Conversation"
    );
}

#[test]
fn classify_general_hint() {
    assert_eq!(
        classify_task_hint("What color is the sky?"),
        TaskHint::General,
        "ambiguous message with no task keywords should classify as General"
    );
}

#[test]
fn classify_empty_input() {
    assert_eq!(
        classify_task_hint(""),
        TaskHint::General,
        "empty input should classify as General"
    );
}

#[test]
fn task_hint_default_is_general() {
    assert_eq!(
        TaskHint::default(),
        TaskHint::General,
        "TaskHint default should be General for backward compatibility"
    );
}

// --- Output-style extraction tests ---

#[test]
fn extract_output_style_from_communication_section() {
    let user_md = "\
# User Profile

## Who
- Name: Alice

## Communication
- Direct and concise preferred
- Structure over prose

## Domains
- code: Syn
";
    let style = extract_output_style(user_md);
    assert!(style.is_some(), "should extract Communication section");
    let style = style.expect("just asserted Some");
    assert!(
        style.contains("Direct and concise"),
        "extracted style should contain Communication content: {style}"
    );
    assert!(
        !style.contains("## Domains"),
        "extracted style should not bleed into next section: {style}"
    );
    assert!(
        !style.contains("Alice"),
        "extracted style should not contain content from other sections: {style}"
    );
}

#[test]
fn extract_output_style_from_output_section() {
    let user_md = "\
# User

## Output
- Answer-first
- No filler
";
    let style = extract_output_style(user_md);
    assert!(style.is_some(), "should extract Output section");
    let style = style.expect("just asserted Some");
    assert!(
        style.contains("Answer-first"),
        "extracted style should contain Output content: {style}"
    );
}

#[test]
fn extract_output_style_none_when_absent() {
    let user_md = "\
# User Profile

## Who
- Name: Bob

## Domains
- research
";
    assert!(
        extract_output_style(user_md).is_none(),
        "should return None when no Communication or Output section exists"
    );
}

#[test]
fn extract_output_style_case_insensitive() {
    let user_md = "\
# User

## COMMUNICATION
- Be terse
";
    let style = extract_output_style(user_md);
    assert!(style.is_some(), "heading match should be case-insensitive");
    let style = style.expect("just asserted Some");
    assert!(
        style.contains("Be terse"),
        "should extract content from case-variant heading: {style}"
    );
}

#[test]
fn extract_output_style_with_expanding_unicode_before_heading() {
    let expanding_prefix = "\u{0130}".repeat(18);
    let user_md = format!(
        "\
# User {expanding_prefix}

## Communication
\u{00e9}quipe style should remain visible

## Domains
- code
"
    );

    let style = extract_output_style(&user_md);
    assert!(
        style.is_some(),
        "should extract Communication section after expanding Unicode prefix"
    );
    let style = style.expect("just asserted Some");
    assert!(
        style.contains("\u{00e9}quipe style should remain visible"),
        "extracted style should contain Communication content: {style}"
    );
    assert!(
        !style.contains("## Domains"),
        "extracted style should not bleed into next section: {style}"
    );
}

#[test]
fn extract_output_style_empty_section_returns_none() {
    let user_md = "\
# User

## Communication

## Domains
- code
";
    assert!(
        extract_output_style(user_md).is_none(),
        "empty Communication section should return None"
    );
}
