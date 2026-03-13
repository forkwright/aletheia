//! Unit tests for CLI argument parsing.

#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::path::PathBuf;

use super::{Cli, Command, commands::maintenance};
use clap::Parser;

#[test]
fn cli_help_works() {
    let result = Cli::try_parse_from(["aletheia", "--help"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
}

#[test]
fn cli_defaults() {
    let cli = Cli::parse_from(["aletheia"]);
    assert!(cli.port.is_none());
    assert!(cli.bind.is_none());
    assert_eq!(cli.log_level, "info");
    assert!(!cli.json_logs);
    assert!(cli.command.is_none());
}

#[test]
fn health_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "health", "--url", "http://localhost:9999"]);
    assert!(matches!(cli.command, Some(Command::Health(_))));
}

#[test]
fn maintenance_status_parses() {
    let cli = Cli::parse_from(["aletheia", "maintenance", "status"]);
    assert!(matches!(
        cli.command,
        Some(Command::Maintenance {
            action: maintenance::Action::Status
        })
    ));
}

#[test]
fn maintenance_run_parses() {
    let cli = Cli::parse_from(["aletheia", "maintenance", "run", "trace-rotation"]);
    assert!(matches!(
        cli.command,
        Some(Command::Maintenance {
            action: maintenance::Action::Run { .. }
        })
    ));
}

#[test]
fn status_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "status"]);
    assert!(matches!(cli.command, Some(Command::Status { .. })));
}

#[test]
fn status_custom_url_parses() {
    let cli = Cli::parse_from(["aletheia", "status", "--url", "http://example:9999"]);
    match cli.command {
        Some(Command::Status { url }) => assert_eq!(url, "http://example:9999"),
        _ => panic!("expected Status command"),
    }
}

#[test]
fn eval_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "eval"]);
    assert!(matches!(cli.command, Some(Command::Eval(_))));
}

#[test]
fn eval_with_options_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "eval",
        "--url",
        "http://example:9999",
        "--token",
        "my-jwt-token",
        "--scenario",
        "health",
        "--json",
        "--timeout",
        "60",
    ]);
    match cli.command {
        Some(Command::Eval(args)) => {
            assert_eq!(args.url, "http://example:9999");
            assert_eq!(args.token.as_deref(), Some("my-jwt-token"));
            assert_eq!(args.scenario.as_deref(), Some("health"));
            assert!(args.json);
            assert_eq!(args.timeout, 60);
        }
        _ => panic!("expected Eval command"),
    }
}

#[test]
fn export_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "export", "syn", "--archived", "--compact"]);
    match cli.command {
        Some(Command::Export(args)) => {
            assert_eq!(args.nous_id, "syn");
            assert!(args.archived);
            assert!(args.compact);
            assert_eq!(args.max_messages, 500);
        }
        _ => panic!("expected Export command"),
    }
}

#[test]
fn export_with_output_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "export",
        "demiurge",
        "-o",
        "/tmp/backup.agent.json",
        "--max-messages",
        "100",
    ]);
    match cli.command {
        Some(Command::Export(args)) => {
            assert_eq!(args.nous_id, "demiurge");
            assert_eq!(
                args.output.unwrap(),
                PathBuf::from("/tmp/backup.agent.json")
            );
            assert_eq!(args.max_messages, 100);
        }
        _ => panic!("expected Export command"),
    }
}
