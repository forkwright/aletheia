use super::*;

// -- ResearchDomain --

#[test]
fn domain_all_contains_four_variants() {
    assert_eq!(ResearchDomain::ALL.len(), 4);
}

#[test]
fn domain_as_str_roundtrips_through_display() {
    for domain in &ResearchDomain::ALL {
        assert_eq!(domain.to_string(), domain.as_str());
    }
}

#[test]
fn domain_serde_roundtrip() {
    for domain in &ResearchDomain::ALL {
        let json = serde_json::to_string(domain).unwrap();
        let back: ResearchDomain = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, domain, "roundtrip failed for {domain:?}");
    }
}

// -- domain_prompt --

#[test]
fn domain_prompt_contains_project_goal() {
    for domain in &ResearchDomain::ALL {
        let prompt = domain_prompt(*domain, "build a chat app");
        assert!(
            prompt.contains("build a chat app"),
            "prompt for {domain} missing project goal"
        );
    }
}

#[test]
fn domain_prompt_stack_mentions_technology() {
    let prompt = domain_prompt(ResearchDomain::Stack, "test");
    assert!(prompt.contains("technology"));
}

#[test]
fn domain_prompt_pitfalls_mentions_failure_modes() {
    let prompt = domain_prompt(ResearchDomain::Pitfalls, "test");
    assert!(prompt.contains("failure modes"));
}

// -- FindingStatus --

#[test]
fn finding_status_serde_roundtrip() {
    let statuses = [
        FindingStatus::Complete,
        FindingStatus::Partial,
        FindingStatus::Failed,
        FindingStatus::TimedOut,
    ];
    for status in &statuses {
        let json = serde_json::to_string(status).unwrap();
        let back: FindingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, status, "roundtrip failed for {status:?}");
    }
}

// -- ResearchConfig --

#[test]
fn default_config_has_all_domains_and_5min_timeout() {
    let config = ResearchConfig::default();
    assert_eq!(config.timeout_secs, 300);
    assert_eq!(config.domains.len(), 4);
}

#[test]
fn config_serde_roundtrip() {
    let config = ResearchConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let back: ResearchConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.timeout_secs, config.timeout_secs);
    assert_eq!(back.domains.len(), config.domains.len());
}

// -- merge_research / deduplicate --

#[test]
fn merge_empty_findings_produces_header_only() {
    let output = merge_research(Vec::new());
    assert!(output.findings.is_empty());
    assert!(output.markdown.contains("# Research Summary"));
}

#[test]
fn merge_preserves_all_domains() {
    let findings = vec![
        ResearchFinding {
            domain: ResearchDomain::Stack,
            content: "Use Rust with Tokio for async.".into(),
            status: FindingStatus::Complete,
        },
        ResearchFinding {
            domain: ResearchDomain::Features,
            content: "Support real-time streaming.".into(),
            status: FindingStatus::Complete,
        },
    ];
    let output = merge_research(findings);
    assert_eq!(output.findings.len(), 2);
    assert!(output.markdown.contains("## Stack"));
    assert!(output.markdown.contains("## Features"));
}

#[test]
fn merge_deduplicates_identical_lines_across_domains() {
    let shared_line = "Use Rust for the backend implementation.";
    let findings = vec![
        ResearchFinding {
            domain: ResearchDomain::Stack,
            content: format!("{shared_line}\nStack-specific detail."),
            status: FindingStatus::Complete,
        },
        ResearchFinding {
            domain: ResearchDomain::Architecture,
            content: format!("{shared_line}\nArchitecture-specific detail."),
            status: FindingStatus::Complete,
        },
    ];
    let output = merge_research(findings);

    let stack = &output.findings[0];
    let arch = &output.findings[1];
    assert!(
        stack.content.contains(shared_line),
        "first occurrence should be preserved"
    );
    assert!(
        !arch.content.contains(shared_line),
        "duplicate line should be removed from later finding"
    );
    assert!(arch.content.contains("Architecture-specific detail."));
}

#[test]
fn merge_preserves_headings_even_if_duplicated() {
    let findings = vec![
        ResearchFinding {
            domain: ResearchDomain::Stack,
            content: "### Overview\nStack content.".into(),
            status: FindingStatus::Complete,
        },
        ResearchFinding {
            domain: ResearchDomain::Features,
            content: "### Overview\nFeature content.".into(),
            status: FindingStatus::Complete,
        },
    ];
    let output = merge_research(findings);

    assert!(output.findings[0].content.contains("### Overview"));
    assert!(output.findings[1].content.contains("### Overview"));
}

#[test]
fn merge_preserves_blank_lines() {
    let findings = vec![ResearchFinding {
        domain: ResearchDomain::Stack,
        content: "Line one.\n\nLine two.".into(),
        status: FindingStatus::Complete,
    }];
    let output = merge_research(findings);
    assert!(output.findings[0].content.contains("\n\n"));
}

#[test]
fn merge_handles_failed_findings() {
    let findings = vec![
        ResearchFinding {
            domain: ResearchDomain::Stack,
            content: "Good content.".into(),
            status: FindingStatus::Complete,
        },
        ResearchFinding {
            domain: ResearchDomain::Features,
            content: String::new(),
            status: FindingStatus::Failed,
        },
    ];
    let output = merge_research(findings);
    assert!(output.markdown.contains("*Research failed"));
}

#[test]
fn merge_handles_timed_out_findings() {
    let findings = vec![ResearchFinding {
        domain: ResearchDomain::Pitfalls,
        content: String::new(),
        status: FindingStatus::TimedOut,
    }];
    let output = merge_research(findings);
    assert!(output.markdown.contains("*Research timed out"));
}

#[test]
fn merge_partial_findings_get_note() {
    let findings = vec![ResearchFinding {
        domain: ResearchDomain::Stack,
        content: "Partial data here.".into(),
        status: FindingStatus::Partial,
    }];
    let output = merge_research(findings);
    assert!(output.markdown.contains("partial results"));
}

#[test]
fn dedup_is_case_insensitive() {
    let findings = vec![
        ResearchFinding {
            domain: ResearchDomain::Stack,
            content: "Use Tokio for async runtime.".into(),
            status: FindingStatus::Complete,
        },
        ResearchFinding {
            domain: ResearchDomain::Architecture,
            content: "use tokio for async runtime.".into(),
            status: FindingStatus::Complete,
        },
    ];
    let output = merge_research(findings);
    assert!(
        output.findings[1].content.is_empty()
            || !output.findings[1]
                .content
                .to_lowercase()
                .contains("use tokio for async runtime"),
        "case-insensitive duplicate should be removed"
    );
}

#[test]
fn dedup_does_not_touch_failed_findings() {
    let shared = "shared content across domains";
    let findings = vec![
        ResearchFinding {
            domain: ResearchDomain::Stack,
            content: shared.into(),
            status: FindingStatus::Complete,
        },
        ResearchFinding {
            domain: ResearchDomain::Features,
            content: shared.into(),
            status: FindingStatus::Failed,
        },
    ];
    let output = merge_research(findings);
    assert_eq!(
        output.findings[1].content, shared,
        "failed findings are not deduplicated"
    );
}

// -- normalize_for_dedup --

#[test]
fn normalize_blank_line_returns_empty() {
    assert!(normalize_for_dedup("").is_empty());
    assert!(normalize_for_dedup("   ").is_empty());
}

#[test]
fn normalize_heading_returns_empty() {
    assert!(normalize_for_dedup("# Heading").is_empty());
    assert!(normalize_for_dedup("## Sub").is_empty());
}

#[test]
fn normalize_short_list_marker_returns_empty() {
    assert!(normalize_for_dedup("- item").is_empty());
    assert!(normalize_for_dedup("* short").is_empty());
}

#[test]
fn normalize_long_list_item_returns_lowercase() {
    let result = normalize_for_dedup("- This is a long list item with real content");
    assert_eq!(result, "- this is a long list item with real content");
}

#[test]
fn normalize_regular_line_returns_lowercase_trimmed() {
    assert_eq!(normalize_for_dedup("  Hello World  "), "hello world");
}

// -- format_markdown --

#[test]
fn format_markdown_no_trailing_whitespace() {
    let findings = vec![ResearchFinding {
        domain: ResearchDomain::Stack,
        content: "Content.".into(),
        status: FindingStatus::Complete,
    }];
    let md = format_markdown(&findings);
    assert!(!md.ends_with('\n'), "should not end with trailing newline");
    assert!(!md.ends_with(' '), "should not end with trailing space");
}

// -- ResearchLevel --

#[test]
fn level_skip_has_no_domains() {
    assert!(ResearchLevel::Skip.domains().is_empty());
}

#[test]
fn level_quick_has_pitfalls_only() {
    let domains = ResearchLevel::Quick.domains();
    assert_eq!(domains.len(), 1);
    assert_eq!(domains[0], ResearchDomain::Pitfalls);
}

#[test]
fn level_standard_has_all_domains() {
    assert_eq!(ResearchLevel::Standard.domains().len(), 4);
}

#[test]
fn level_deep_dive_has_all_domains() {
    assert_eq!(ResearchLevel::DeepDive.domains().len(), 4);
}

#[test]
fn level_synthesis_needed_for_standard_and_deep_dive() {
    assert!(!ResearchLevel::Skip.needs_synthesis());
    assert!(!ResearchLevel::Quick.needs_synthesis());
    assert!(ResearchLevel::Standard.needs_synthesis());
    assert!(ResearchLevel::DeepDive.needs_synthesis());
}

#[test]
fn level_display_includes_number_and_name() {
    assert_eq!(ResearchLevel::Skip.to_string(), "L0 (Skip)");
    assert_eq!(ResearchLevel::Quick.to_string(), "L1 (Quick)");
    assert_eq!(ResearchLevel::Standard.to_string(), "L2 (Standard)");
    assert_eq!(ResearchLevel::DeepDive.to_string(), "L3 (Deep Dive)");
}

#[test]
fn level_ordering() {
    assert!(ResearchLevel::Skip < ResearchLevel::Quick);
    assert!(ResearchLevel::Quick < ResearchLevel::Standard);
    assert!(ResearchLevel::Standard < ResearchLevel::DeepDive);
}

#[test]
fn level_to_config_uses_given_timeout() {
    let config = ResearchLevel::Standard.to_config(120);
    assert_eq!(config.timeout_secs, 120);
    assert_eq!(config.domains.len(), 4);
}

#[test]
fn level_serde_roundtrip() {
    let levels = [
        ResearchLevel::Skip,
        ResearchLevel::Quick,
        ResearchLevel::Standard,
        ResearchLevel::DeepDive,
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: ResearchLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, level, "roundtrip failed for {level:?}");
    }
}

// -- select_research_level --

#[test]
fn select_level_user_override_takes_priority() {
    let signals = ComplexitySignals {
        has_novel_technology: true,
        has_security_concerns: true,
        user_override: Some(ResearchLevel::Skip),
        ..Default::default()
    };
    assert_eq!(select_research_level(&signals), ResearchLevel::Skip);
}

#[test]
fn select_level_simple_task_returns_skip() {
    let signals = ComplexitySignals {
        requirement_count: 1,
        ..Default::default()
    };
    assert_eq!(select_research_level(&signals), ResearchLevel::Skip);
}

#[test]
fn select_level_moderate_complexity_returns_quick() {
    let signals = ComplexitySignals {
        requirement_count: 4,
        ..Default::default()
    };
    assert_eq!(select_research_level(&signals), ResearchLevel::Quick);
}

#[test]
fn select_level_novel_tech_returns_standard() {
    let signals = ComplexitySignals {
        requirement_count: 3,
        has_novel_technology: true,
        ..Default::default()
    };
    assert_eq!(select_research_level(&signals), ResearchLevel::Standard);
}

#[test]
fn select_level_high_complexity_returns_deep_dive() {
    let signals = ComplexitySignals {
        requirement_count: 12,
        has_novel_technology: true,
        has_security_concerns: true,
        has_architectural_decisions: true,
        ..Default::default()
    };
    assert_eq!(select_research_level(&signals), ResearchLevel::DeepDive);
}

#[test]
fn select_level_existing_patterns_reduce_score() {
    let signals = ComplexitySignals {
        requirement_count: 4,
        has_existing_patterns: true,
        ..Default::default()
    };
    assert_eq!(
        select_research_level(&signals),
        ResearchLevel::Skip,
        "existing patterns should offset requirement count"
    );
}

#[test]
fn select_level_security_plus_migration_returns_standard() {
    let signals = ComplexitySignals {
        requirement_count: 3,
        has_security_concerns: true,
        has_data_migration: true,
        ..Default::default()
    };
    assert_eq!(select_research_level(&signals), ResearchLevel::Standard);
}

#[test]
fn select_level_external_integrations_add_complexity() {
    let signals = ComplexitySignals {
        requirement_count: 4,
        has_external_integrations: true,
        ..Default::default()
    };
    assert_eq!(select_research_level(&signals), ResearchLevel::Standard);
}

// -- ResearchOutput --

#[test]
fn research_output_serde_roundtrip() {
    let output = merge_research(vec![ResearchFinding {
        domain: ResearchDomain::Stack,
        content: "Test content.".into(),
        status: FindingStatus::Complete,
    }]);
    let json = serde_json::to_string(&output).unwrap();
    let back: ResearchOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(back.findings.len(), 1);
    assert_eq!(back.markdown, output.markdown);
}
