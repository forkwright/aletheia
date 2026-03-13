//! Unit tests for CLI argument parsing.

#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::path::PathBuf;

use super::{
    Cli, Command, commands::agent_io::InitArgs, commands::maintenance,
    commands::session_export::ExportFormat,
};
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

#[test]
fn session_export_defaults_to_markdown() {
    let cli = Cli::parse_from(["aletheia", "session-export", "01JBVK0000000000000000000A"]);
    match cli.command {
        Some(Command::SessionExport(args)) => {
            assert_eq!(args.session_id, "01JBVK0000000000000000000A");
            assert!(matches!(args.format, ExportFormat::Md));
            assert!(args.output.is_none());
            assert_eq!(args.url, "http://127.0.0.1:18789");
        }
        _ => panic!("expected SessionExport command"),
    }
}

#[test]
fn session_export_json_format_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "session-export",
        "01JBVK0000000000000000000A",
        "--format",
        "json",
    ]);
    match cli.command {
        Some(Command::SessionExport(args)) => {
            assert!(matches!(args.format, ExportFormat::Json));
        }
        _ => panic!("expected SessionExport command"),
    }
}

#[test]
fn session_export_with_output_file_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "session-export",
        "01JBVK0000000000000000000A",
        "--output",
        "/tmp/session.md",
    ]);
    match cli.command {
        Some(Command::SessionExport(args)) => {
            assert_eq!(args.output.unwrap(), PathBuf::from("/tmp/session.md"));
        }
        _ => panic!("expected SessionExport command"),
    }
}

#[test]
fn init_non_interactive_with_instance_path_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "init",
        "--non-interactive",
        "--instance-path",
        "/tmp/test-instance",
    ]);
    match cli.command {
        Some(Command::Init(InitArgs {
            instance_root,
            non_interactive,
            yes,
            api_key,
            ..
        })) => {
            assert_eq!(instance_root.unwrap(), PathBuf::from("/tmp/test-instance"));
            assert!(non_interactive);
            assert!(!yes);
            assert!(api_key.is_none());
        }
        _ => panic!("expected Init command"),
    }
}

#[test]
fn init_non_interactive_with_all_flags_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "init",
        "--non-interactive",
        "--instance-path",
        "/srv/aletheia",
        "--auth-mode",
        "token",
        "--api-provider",
        "anthropic",
        "--model",
        "claude-opus-4-6",
        "--api-key",
        "sk-ant-test",
    ]);
    match cli.command {
        Some(Command::Init(InitArgs {
            instance_root,
            non_interactive,
            auth_mode,
            api_provider,
            model,
            api_key,
            ..
        })) => {
            assert_eq!(instance_root.unwrap(), PathBuf::from("/srv/aletheia"));
            assert!(non_interactive);
            assert_eq!(auth_mode.as_deref(), Some("token"));
            assert_eq!(api_provider.as_deref(), Some("anthropic"));
            assert_eq!(model.as_deref(), Some("claude-opus-4-6"));
            assert_eq!(api_key.as_deref(), Some("sk-ant-test"));
        }
        _ => panic!("expected Init command"),
    }
}

#[test]
fn init_yes_flag_no_instance_path_parses() {
    let cli = Cli::parse_from(["aletheia", "init", "-y"]);
    match cli.command {
        Some(Command::Init(InitArgs {
            instance_root,
            yes,
            non_interactive,
            ..
        })) => {
            assert!(instance_root.is_none());
            assert!(yes);
            assert!(!non_interactive);
        }
        _ => panic!("expected Init command"),
    }
}

#[test]
fn init_instance_root_alias_accepted() {
    let cli = Cli::parse_from(["aletheia", "init", "--instance-root", "/custom/path"]);
    match cli.command {
        Some(Command::Init(InitArgs { instance_root, .. })) => {
            assert_eq!(instance_root.unwrap(), PathBuf::from("/custom/path"));
        }
        _ => panic!("expected Init command"),
    }
}
