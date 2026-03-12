use super::*;

const SAMPLE_SKILL: &str = r"# Website and Review Intelligence Gathering
Systematically research a company by fetching official pages and aggregating third-party reviews.

## When to Use
When you need to comprehensively understand a company's offerings and reputation.

## Steps
1. Enable web_fetch tool
2. Fetch the company's homepage to identify main offerings
3. Search for independent reviews and discussions

## Tools Used
- web_fetch: to retrieve complete content from official company pages
- web_search: to locate independent reviews and discussions
";

const SAMPLE_WITH_FRONTMATTER: &str = r"---
tools: [web_fetch, web_search]
domains: [research, writing]
---

# Website Intelligence
Research a company via web.

## When to Use
When you need company intelligence.

## Steps
1. Fetch homepage
2. Search reviews

## Tools Used
- web_fetch: fetch pages
- web_search: search web
";

#[test]
fn parse_basic_skill_md() {
    let skill = parse_skill_md(SAMPLE_SKILL, "web-research").expect("valid skill md");
    assert_eq!(skill.name, "web-research");
    assert!(skill.description.contains("Systematically research"));
    assert_eq!(skill.steps.len(), 3);
    assert_eq!(skill.steps[0], "Enable web_fetch tool");
    assert_eq!(skill.tools_used, vec!["web_fetch", "web_search"]);
    assert_eq!(skill.origin, "seeded");
}

#[test]
fn parse_skill_with_frontmatter() {
    let skill =
        parse_skill_md(SAMPLE_WITH_FRONTMATTER, "web-intel").expect("valid frontmatter skill md");
    assert_eq!(skill.tools_used, vec!["web_fetch", "web_search"]);
    assert_eq!(skill.domain_tags, vec!["research", "writing"]);
    assert_eq!(skill.steps.len(), 2);
}

#[test]
fn parse_skill_derives_domain_tags_from_slug() {
    let skill = parse_skill_md(SAMPLE_SKILL, "docker-network-diagnostics")
        .expect("valid skill md for domain tag derivation");
    assert_eq!(skill.domain_tags, vec!["docker", "network", "diagnostics"]);
}

#[test]
fn parse_skill_missing_heading_fails() {
    let bad = "No heading here\n\n## Steps\n1. Do stuff";
    let err = parse_skill_md(bad, "bad-skill").expect_err("bad skill md must fail");
    assert!(err.reason.contains("missing top-level heading"));
}

#[test]
fn parse_skill_empty_doc_fails() {
    let err = parse_skill_md("", "empty").expect_err("empty skill md must fail");
    assert!(err.reason.contains("empty document"));
}

#[test]
fn parse_skill_no_description_uses_when_to_use() {
    let md = "# Skill\n\n## When to Use\nWhen you need to do things.\n\n## Steps\n1. Do it\n";
    let skill = parse_skill_md(md, "fallback")
        .expect("skill with when-to-use fallback description should parse");
    assert!(skill.description.contains("When you need to do things"));
}

#[test]
fn parse_skill_no_description_at_all_fails() {
    let md = "# Skill\n\n## Steps\n1. Do it\n";
    let err = parse_skill_md(md, "no-desc")
        .expect_err("skill without any description must fail to parse");
    assert!(err.reason.contains("no description"));
}

#[test]
fn skill_content_serde_roundtrip() {
    let skill = SkillContent {
        name: "test-skill".to_owned(),
        description: "A test skill".to_owned(),
        steps: vec!["step 1".to_owned(), "step 2".to_owned()],
        tools_used: vec!["Read".to_owned(), "Edit".to_owned()],
        domain_tags: vec!["test".to_owned()],
        origin: "manual".to_owned(),
    };
    let json = serde_json::to_string(&skill).expect("SkillContent serializes to JSON");
    let back: SkillContent =
        serde_json::from_str(&json).expect("SkillContent deserializes from JSON");
    assert_eq!(skill, back);
}

#[test]
fn parse_yaml_array_formats() {
    assert_eq!(parse_yaml_array("[a, b, c]"), vec!["a", "b", "c"]);
    assert_eq!(parse_yaml_array("[\"a\", 'b']"), vec!["a", "b"]);
    assert_eq!(parse_yaml_array("[]"), Vec::<String>::new());
}

#[test]
fn split_frontmatter_present() {
    let (fm, body) = split_frontmatter("---\ntools: [a]\n---\n# Title\n");
    assert!(fm.is_some());
    assert!(fm.expect("frontmatter present").contains("tools:"));
    assert!(body.contains("# Title"));
}

#[test]
fn split_frontmatter_absent() {
    let (fm, body) = split_frontmatter("# Title\nBody text");
    assert!(fm.is_none());
    assert!(body.contains("# Title"));
}

#[test]
fn scan_skill_dir_with_tempdir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let skill_dir = dir.path().join("my-skill");
    std::fs::create_dir(&skill_dir).expect("create skill subdir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "# My Skill\nDoes things.\n\n## When to Use\nAlways.\n\n## Steps\n1. Go\n",
    )
    .expect("write SKILL.md");

    let skills = scan_skill_dir(dir.path()).expect("scan skill dir");
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].0, "my-skill");
}

#[test]
fn scan_skill_dir_empty() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let skills = scan_skill_dir(dir.path()).expect("scan empty skill dir");
    assert!(skills.is_empty());
}

#[test]
fn scan_skill_dir_ignores_non_skill_dirs() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let sub = dir.path().join("not-a-skill");
    std::fs::create_dir(&sub).expect("create non-skill subdir");
    std::fs::write(sub.join("README.md"), "not a skill").expect("write README.md");

    let skills = scan_skill_dir(dir.path()).expect("scan dir with non-skill subdirs");
    assert!(skills.is_empty());
}

#[test]
fn extract_steps_mixed_format() {
    let lines = vec![
        "1. First step".to_owned(),
        "2. Second step".to_owned(),
        "- Third step".to_owned(),
    ];
    let steps = extract_steps(&lines);
    assert_eq!(steps, vec!["First step", "Second step", "Third step"]);
}

#[test]
fn skill_parse_error_display() {
    let err = SkillParseError {
        path: "test-skill".to_owned(),
        reason: "missing heading".to_owned(),
    };
    assert_eq!(
        err.to_string(),
        "failed to parse test-skill: missing heading"
    );
}

// ── slugify ──────────────────────────────────────────────────────────────

#[test]
fn slugify_simple_name() {
    assert_eq!(slugify("rust-error-handling"), "rust-error-handling");
}

#[test]
fn slugify_spaces_to_dashes() {
    assert_eq!(
        slugify("Docker Network Diagnostics"),
        "docker-network-diagnostics"
    );
}

#[test]
fn slugify_special_chars() {
    assert_eq!(slugify("C++ Template (Meta)"), "c-template-meta");
}

#[test]
fn slugify_consecutive_specials_collapsed() {
    assert_eq!(slugify("test---skill___name"), "test-skill-name");
}

#[test]
fn slugify_empty_string() {
    assert_eq!(slugify(""), "");
}

#[test]
fn slugify_all_special() {
    assert_eq!(slugify("---"), "");
}

// ── format_skill_md ──────────────────────────────────────────────────────

fn export_skill() -> SkillContent {
    SkillContent {
        name: "rust-error-handling".to_owned(),
        description: "Pattern for converting error types across crate boundaries".to_owned(),
        steps: vec![
            "Identify the source error type".to_owned(),
            "Create a snafu variant with #[snafu(source)]".to_owned(),
            "Add .context() at the call site".to_owned(),
        ],
        tools_used: vec!["Read".to_owned(), "Edit".to_owned(), "Bash".to_owned()],
        domain_tags: vec!["rust".to_owned(), "errors".to_owned()],
        origin: "manual".to_owned(),
    }
}

#[test]
fn format_skill_md_has_yaml_frontmatter() {
    let md = format_skill_md(&export_skill());
    assert!(
        md.starts_with("---\n"),
        "should start with frontmatter delimiter"
    );
    // Count frontmatter delimiters
    let delimiters: Vec<_> = md.match_indices("---").collect();
    assert!(delimiters.len() >= 2, "should have opening and closing ---");
}

#[test]
fn format_skill_md_frontmatter_has_name() {
    let md = format_skill_md(&export_skill());
    assert!(md.contains("name: rust-error-handling"));
}

#[test]
fn format_skill_md_frontmatter_has_description() {
    let md = format_skill_md(&export_skill());
    assert!(md.contains("description: Pattern for converting error types"));
}

#[test]
fn format_skill_md_frontmatter_has_allowed_tools() {
    let md = format_skill_md(&export_skill());
    assert!(md.contains("allowed-tools: Read, Edit, Bash"));
}

#[test]
fn format_skill_md_has_when_to_use_section() {
    let md = format_skill_md(&export_skill());
    assert!(md.contains("## When to Use"));
}

#[test]
fn format_skill_md_has_steps_section() {
    let md = format_skill_md(&export_skill());
    assert!(md.contains("## Steps"));
    assert!(md.contains("1. Identify the source error type"));
    assert!(md.contains("2. Create a snafu variant"));
    assert!(md.contains("3. Add .context() at the call site"));
}

#[test]
fn format_skill_md_has_tools_section() {
    let md = format_skill_md(&export_skill());
    assert!(md.contains("## Tools Used"));
    assert!(md.contains("- Read"));
    assert!(md.contains("- Edit"));
    assert!(md.contains("- Bash"));
}

#[test]
fn format_skill_md_has_tags_section() {
    let md = format_skill_md(&export_skill());
    assert!(md.contains("## Tags"));
    assert!(md.contains("rust, errors"));
}

#[test]
fn format_skill_md_no_tools_omits_allowed_tools() {
    let mut skill = export_skill();
    skill.tools_used.clear();
    let md = format_skill_md(&skill);
    assert!(!md.contains("allowed-tools:"));
    assert!(!md.contains("## Tools Used"));
}

#[test]
fn format_skill_md_no_steps_omits_steps_section() {
    let mut skill = export_skill();
    skill.steps.clear();
    let md = format_skill_md(&skill);
    assert!(!md.contains("## Steps"));
}

#[test]
fn format_skill_md_description_with_colon_is_quoted() {
    let mut skill = export_skill();
    skill.description = "Error handling: a deep dive".to_owned();
    let md = format_skill_md(&skill);
    assert!(md.contains(r#"description: "Error handling: a deep dive""#));
}

// ── export_skills_to_cc ──────────────────────────────────────────────────

#[test]
fn export_creates_correct_directory_structure() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let skills = vec![export_skill()];
    let exported = export_skills_to_cc(&skills, dir.path(), None).expect("export skills to cc");

    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].slug, "rust-error-handling");

    let skill_md = dir.path().join("rust-error-handling").join("SKILL.md");
    assert!(
        skill_md.exists(),
        "SKILL.md should exist at {}",
        skill_md.display()
    );
}

#[test]
fn export_skill_md_contains_valid_frontmatter() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let skills = vec![export_skill()];
    export_skills_to_cc(&skills, dir.path(), None).expect("export skills to cc");

    let content = std::fs::read_to_string(dir.path().join("rust-error-handling").join("SKILL.md"))
        .expect("read exported SKILL.md");
    assert!(content.starts_with("---\n"));
    assert!(content.contains("name: rust-error-handling"));
    assert!(content.contains("description:"));
    assert!(content.contains("allowed-tools:"));
}

#[test]
fn export_domain_filtering_excludes_non_matching() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let rust_skill = export_skill();
    let mut python_skill = export_skill();
    python_skill.name = "python-testing".to_owned();
    python_skill.domain_tags = vec!["python".to_owned(), "testing".to_owned()];

    let skills = vec![rust_skill, python_skill];
    let exported = export_skills_to_cc(&skills, dir.path(), Some(&["rust"]))
        .expect("export with domain filter");

    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].slug, "rust-error-handling");
}

#[test]
fn export_no_skills_produces_empty_result() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let exported = export_skills_to_cc(&[], dir.path(), None).expect("export empty skills list");
    assert!(exported.is_empty());
}

#[test]
fn export_multiple_skills_creates_separate_directories() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut docker_skill = export_skill();
    docker_skill.name = "docker-diagnostics".to_owned();
    docker_skill.domain_tags = vec!["docker".to_owned()];

    let skills = vec![export_skill(), docker_skill];
    let exported = export_skills_to_cc(&skills, dir.path(), None).expect("export multiple skills");

    assert_eq!(exported.len(), 2);
    assert!(
        dir.path()
            .join("rust-error-handling")
            .join("SKILL.md")
            .exists()
    );
    assert!(
        dir.path()
            .join("docker-diagnostics")
            .join("SKILL.md")
            .exists()
    );
}

#[test]
fn export_roundtrip_content_preserved() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let original = export_skill();
    export_skills_to_cc(std::slice::from_ref(&original), dir.path(), None)
        .expect("export skill for roundtrip");

    // Read back and parse
    let exported_md =
        std::fs::read_to_string(dir.path().join("rust-error-handling").join("SKILL.md"))
            .expect("read back exported SKILL.md");
    let parsed =
        parse_skill_md(&exported_md, "rust-error-handling").expect("re-parse exported skill md");

    assert_eq!(parsed.name, original.name);
    assert_eq!(parsed.description, original.description);
    assert_eq!(parsed.steps, original.steps);
    assert_eq!(parsed.tools_used, original.tools_used);
}

#[test]
fn export_special_chars_in_name_slugified() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut skill = export_skill();
    skill.name = "C++ Template (Meta)".to_owned();
    let exported = export_skills_to_cc(&[skill], dir.path(), None)
        .expect("export skill with special chars in name");

    assert_eq!(exported[0].slug, "c-template-meta");
    assert!(dir.path().join("c-template-meta").join("SKILL.md").exists());
}

#[test]
fn export_overwrites_existing_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let skill_dir = dir.path().join("rust-error-handling");
    std::fs::create_dir_all(&skill_dir).expect("create pre-existing skill dir");
    std::fs::write(skill_dir.join("SKILL.md"), "old content").expect("write pre-existing SKILL.md");

    let skills = vec![export_skill()];
    export_skills_to_cc(&skills, dir.path(), None).expect("overwrite existing skill");

    let content =
        std::fs::read_to_string(skill_dir.join("SKILL.md")).expect("read overwritten SKILL.md");
    assert!(
        content.contains("## When to Use"),
        "should have new content"
    );
    assert!(!content.contains("old content"));
}

#[test]
fn export_domain_filter_with_no_matches_returns_empty() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let skills = vec![export_skill()]; // domain_tags: ["rust", "errors"]
    let exported = export_skills_to_cc(&skills, dir.path(), Some(&["python"]))
        .expect("export with non-matching domain filter");
    assert!(exported.is_empty());
}

// ── skill_decay_score ───────────────────────────────────────────────────

#[test]
fn skill_decay_fresh_skill_scores_high() {
    let score = skill_decay_score(0.0, 5, 0.9);
    assert!(
        score > 0.8,
        "fresh skill with decent usage should score > 0.8, got {score}"
    );
}

#[test]
fn skill_decay_unused_skill_decays_below_review() {
    // 40 days unused, zero usage, moderate confidence
    let score = skill_decay_score(40.0, 0, 0.7);
    assert!(
        score < decay::NEEDS_REVIEW_THRESHOLD,
        "40-day unused skill should be below review threshold, got {score}"
    );
}

#[test]
fn skill_decay_very_stale_skill_below_retire() {
    // 90 days unused, zero usage, low confidence
    let score = skill_decay_score(90.0, 0, 0.5);
    assert!(
        score < decay::RETIRE_THRESHOLD,
        "90-day unused, low-usage skill should be below retire threshold, got {score}"
    );
}

#[test]
fn skill_decay_high_usage_decays_slower() {
    let low_usage_score = skill_decay_score(40.0, 2, 0.8);
    let high_usage_score = skill_decay_score(40.0, 15, 0.8);
    assert!(
        high_usage_score > low_usage_score,
        "high-usage skill should decay slower: high={high_usage_score} > low={low_usage_score}"
    );
}

#[test]
fn skill_decay_high_usage_above_review_at_40_days() {
    // High-usage skills with 3× slower decay should survive 40 days
    let score = skill_decay_score(40.0, 15, 0.9);
    assert!(
        score >= decay::NEEDS_REVIEW_THRESHOLD,
        "high-usage skill at 40 days should still be above review threshold, got {score}"
    );
}

#[test]
fn skill_decay_score_range_zero_to_one() {
    for days in [0.0, 1.0, 10.0, 28.0, 60.0, 120.0, 365.0] {
        for usage in [0, 1, 5, 10, 20, 50] {
            for conf in [0.0, 0.5, 1.0] {
                let score = skill_decay_score(days, usage, conf);
                assert!(
                    (0.0..=1.0).contains(&score),
                    "score out of range: {score} for days={days}, usage={usage}, conf={conf}"
                );
            }
        }
    }
}

#[test]
fn skill_decay_zero_confidence_is_zero() {
    let score = skill_decay_score(0.0, 10, 0.0);
    assert!(
        score < f64::EPSILON,
        "zero confidence should produce zero score, got {score}"
    );
}

#[test]
fn skill_health_metrics_default() {
    let m = SkillHealthMetrics::default();
    assert_eq!(m.total_active, 0);
    assert_eq!(m.total_retired, 0);
    assert_eq!(m.total_needs_review, 0);
}

#[test]
fn skill_health_metrics_serde_roundtrip() {
    let m = SkillHealthMetrics {
        total_active: 10,
        total_retired: 2,
        total_needs_review: 1,
        avg_usage_count: 5.5,
        median_days_since_use: 3.0,
        top_skills: vec![("rust-errors".to_owned(), 15)],
        bottom_skills: vec![("old-skill".to_owned(), 0)],
        dedup_discard_count: 3,
        dedup_total_count: 10,
    };
    let json = serde_json::to_string(&m).expect("serialize");
    let back: SkillHealthMetrics = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.total_active, 10);
    assert_eq!(back.top_skills.len(), 1);
}
