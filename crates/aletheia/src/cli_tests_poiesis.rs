//! Unit tests for `aletheia poiesis` CLI argument parsing
//! (forkwright/aletheia#7172, poiesis-evolution B-010 verb canon).
//!
//! Split out of `cli_tests.rs` (RUST/file-too-long: that file already sits
//! at 800+ lines on `main`; the poiesis verb set is sizeable enough to
//! justify its own module rather than growing it further).

use clap::Parser;

use super::{Cli, Command, commands::poiesis};

#[test]
fn poiesis_list_parses() {
    let cli = Cli::parse_from(["aletheia", "poiesis", "list"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Poiesis {
                action: poiesis::Action::List { json: false }
            })
        ),
        "poiesis list subcommand should parse"
    );
}

#[test]
fn poiesis_list_components_alias_parses_to_list() {
    let cli = Cli::parse_from(["aletheia", "poiesis", "list-components"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Poiesis {
                action: poiesis::Action::List { json: false }
            })
        ),
        "poiesis list-components alias should parse to the List action"
    );
}

#[test]
fn poiesis_get_parses() {
    let cli = Cli::parse_from(["aletheia", "poiesis", "get", "title"]);
    match cli.command {
        Some(Command::Poiesis {
            action: poiesis::Action::Get { id },
        }) => assert_eq!(id, "title", "component id should be captured"),
        _ => panic!("expected Poiesis Get action"),
    }
}

#[test]
fn poiesis_get_component_alias_parses() {
    let cli = Cli::parse_from(["aletheia", "poiesis", "get-component", "bullet"]);
    assert!(
        matches!(cli.command, Some(Command::Poiesis { .. })),
        "poiesis get-component alias should parse"
    );
}

#[test]
fn poiesis_create_requires_slug() {
    let result = Cli::try_parse_from(["aletheia", "poiesis", "create"]);
    assert!(
        result.is_err(),
        "poiesis create without --slug should fail to parse"
    );

    let cli = Cli::parse_from(["aletheia", "poiesis", "create", "--slug", "quarterly"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Poiesis {
                action: poiesis::Action::Create { .. }
            })
        ),
        "poiesis create subcommand should parse with just --slug (format defaults to typst)"
    );
}

#[test]
fn poiesis_preview_parses_with_no_args() {
    let cli = Cli::parse_from(["aletheia", "poiesis", "preview"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Poiesis {
                action: poiesis::Action::Preview { .. }
            })
        ),
        "poiesis preview subcommand should parse with no arguments"
    );
}

#[test]
fn poiesis_preview_rejects_source_and_template_together() {
    let result = Cli::try_parse_from([
        "aletheia",
        "poiesis",
        "preview",
        "--template",
        "default",
        "--source",
        "report.typ",
    ]);
    assert!(
        result.is_err(),
        "--template and --source should be mutually exclusive"
    );
}

#[test]
fn poiesis_qa_requires_prose() {
    let result = Cli::try_parse_from(["aletheia", "poiesis", "qa"]);
    assert!(result.is_err(), "poiesis qa without --prose should fail");

    let cli = Cli::parse_from(["aletheia", "poiesis", "qa", "--prose", "report.md"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Poiesis {
                action: poiesis::Action::Qa { .. }
            })
        ),
        "poiesis qa subcommand should parse"
    );
}

#[test]
fn poiesis_lint_requires_prose() {
    let result = Cli::try_parse_from(["aletheia", "poiesis", "lint"]);
    assert!(result.is_err(), "poiesis lint without --prose should fail");

    let cli = Cli::parse_from(["aletheia", "poiesis", "lint", "--prose", "report.md"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Poiesis {
                action: poiesis::Action::Lint { .. }
            })
        ),
        "poiesis lint subcommand should parse"
    );
}

#[test]
fn poiesis_verify_requires_manifest() {
    let result = Cli::try_parse_from(["aletheia", "poiesis", "verify"]);
    assert!(
        result.is_err(),
        "poiesis verify without --manifest should fail"
    );

    let cli = Cli::parse_from([
        "aletheia",
        "poiesis",
        "verify",
        "--manifest",
        "manifest.json",
    ]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Poiesis {
                action: poiesis::Action::Verify { .. }
            })
        ),
        "poiesis verify subcommand should parse"
    );
}

#[test]
fn poiesis_run_requires_format_and_content() {
    let result = Cli::try_parse_from(["aletheia", "poiesis", "run"]);
    assert!(
        result.is_err(),
        "poiesis run without --format/--content should fail to parse"
    );

    let cli = Cli::parse_from([
        "aletheia",
        "poiesis",
        "run",
        "--format",
        "pdf",
        "--content",
        "blocks.json",
    ]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Poiesis {
                action: poiesis::Action::Run { .. }
            })
        ),
        "poiesis run subcommand should parse"
    );
}
