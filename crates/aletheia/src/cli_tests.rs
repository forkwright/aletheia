//! Unit tests for CLI argument parsing.

#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::path::PathBuf;

use super::{
    Cli, Command, commands::add_nous::AddNousArgs, commands::agent_io::InitArgs,
    commands::backup::BackupArgs, commands::credential, commands::maintenance,
    commands::session_export::ExportFormat, commands::tls,
};
use clap::Parser;

#[test]
fn cli_help_works() {
    let result = Cli::try_parse_from(["aletheia", "--help"]);
    assert!(result.is_err(), "result should be err");
    let err = result.unwrap_err();
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::DisplayHelp,
        "kind( should match result"
    );
}

#[test]
fn cli_defaults() {
    let cli = Cli::parse_from(["aletheia"]);
    assert!(cli.port.is_none(), "port should be none");
    assert!(cli.bind.is_none(), "bind should be none");
    assert_eq!(
        cli.log_level, "info",
        "log_level should equal expected value"
    );
    assert!(!cli.json_logs, "json_logs should be false");
    assert!(cli.command.is_none(), "command should be none");
}

#[test]
fn health_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "health", "--url", "http://localhost:9999"]);
    assert!(
        matches!(cli.command, Some(Command::Health(_))),
        "value should match expected pattern"
    );
}

#[test]
fn maintenance_status_parses() {
    let cli = Cli::parse_from(["aletheia", "maintenance", "status"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Maintenance {
                action: maintenance::Action::Status { .. }
            })
        ),
        "value should match expected pattern",
    );
}

#[test]
fn maintenance_run_parses() {
    let cli = Cli::parse_from(["aletheia", "maintenance", "run", "trace-rotation"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Maintenance {
                action: maintenance::Action::Run { .. }
            })
        ),
        "value should match expected pattern",
    );
}

#[test]
fn status_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "status"]);
    assert!(
        matches!(cli.command, Some(Command::Status { .. })),
        "value should match expected pattern"
    );
}

#[test]
fn status_custom_url_parses() {
    let cli = Cli::parse_from(["aletheia", "status", "--url", "http://example:9999"]);
    match cli.command {
        Some(Command::Status { url }) => assert_eq!(
            url, "http://example:9999",
            "url should equal expected value"
        ),
        _ => panic!("expected Status command"),
    }
}

#[test]
fn eval_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "eval"]);
    assert!(
        matches!(cli.command, Some(Command::Eval(_))),
        "value should match expected pattern"
    );
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
            assert_eq!(
                args.url, "http://example:9999",
                "url should equal expected value"
            );
            assert_eq!(
                args.token.as_deref(),
                Some("my-jwt-token"),
                "as_deref( should match Some(\"my-jwt-token\")"
            );
            assert_eq!(
                args.scenario.as_deref(),
                Some("health"),
                "as_deref( should match Some(\"health\")"
            );
            assert!(args.json, "assertion failed in eval with options parses");
            assert_eq!(args.timeout, 60, "timeout should equal expected value");
        }
        _ => panic!("expected Eval command"),
    }
}

#[test]
fn export_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "export", "syn", "--archived", "--compact"]);
    match cli.command {
        Some(Command::Export(args)) => {
            assert_eq!(args.nous_id, "syn", "nous_id should equal expected value");
            assert!(
                args.archived,
                "assertion failed in export subcommand parses"
            );
            assert!(args.compact, "assertion failed in export subcommand parses");
            assert_eq!(
                args.max_messages, 500,
                "max_messages should equal expected value"
            );
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
            assert_eq!(
                args.nous_id, "demiurge",
                "nous_id should equal expected value"
            );
            assert_eq!(
                args.output.unwrap(),
                PathBuf::from("/tmp/backup.agent.json"),
                "unwrap( should match json\"",
            );
            assert_eq!(
                args.max_messages, 100,
                "max_messages should equal expected value"
            );
        }
        _ => panic!("expected Export command"),
    }
}

#[test]
fn session_export_defaults_to_markdown() {
    let cli = Cli::parse_from(["aletheia", "session-export", "01JBVK0000000000000000000A"]);
    match cli.command {
        Some(Command::SessionExport(args)) => {
            assert_eq!(
                args.session_id, "01JBVK0000000000000000000A",
                "session_id should equal expected value"
            );
            assert!(
                matches!(args.format, ExportFormat::Md),
                "value should match expected pattern"
            );
            assert!(args.output.is_none(), "output should be none");
            assert_eq!(
                args.url, "http://127.0.0.1:18789",
                "url should equal expected value"
            );
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
            assert!(
                matches!(args.format, ExportFormat::Json),
                "value should match expected pattern"
            );
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
            assert_eq!(
                args.output.unwrap(),
                PathBuf::from("/tmp/session.md"),
                "unwrap( should match md\""
            );
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
            assert_eq!(
                instance_root.unwrap(),
                PathBuf::from("/tmp/test-instance"),
                "unwrap( should match result"
            );
            assert!(
                non_interactive,
                "assertion failed in init non interactive with instance path parses"
            );
            assert!(!yes, "yes should be false");
            assert!(api_key.is_none(), "api_key should be none");
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
            assert_eq!(
                instance_root.unwrap(),
                PathBuf::from("/srv/aletheia"),
                "unwrap( should match PathBuf::from(\"/srv/aletheia\")"
            );
            assert!(
                non_interactive,
                "assertion failed in init non interactive with all flags parses"
            );
            assert_eq!(
                auth_mode.as_deref(),
                Some("token"),
                "as_deref( should match Some(\"token\")"
            );
            assert_eq!(
                api_provider.as_deref(),
                Some("anthropic"),
                "as_deref( should match Some(\"anthropic\")"
            );
            assert_eq!(
                model.as_deref(),
                Some("claude-opus-4-6"),
                "as_deref( should match Some(\"claude-opus-4-6\")"
            );
            assert_eq!(
                api_key.as_deref(),
                Some("sk-ant-test"),
                "as_deref( should match Some(\"sk-ant-test\")"
            );
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
            assert!(instance_root.is_none(), "instance_root should be none");
            assert!(
                yes,
                "assertion failed in init yes flag no instance path parses"
            );
            assert!(!non_interactive, "non_interactive should be false");
        }
        _ => panic!("expected Init command"),
    }
}

#[test]
fn init_instance_root_alias_accepted() {
    let cli = Cli::parse_from(["aletheia", "init", "--instance-root", "/custom/path"]);
    match cli.command {
        Some(Command::Init(InitArgs { instance_root, .. })) => {
            assert_eq!(
                instance_root.unwrap(),
                PathBuf::from("/custom/path"),
                "unwrap( should match PathBuf::from(\"/custom/path\")"
            );
        }
        _ => panic!("expected Init command"),
    }
}

#[test]
fn backup_default_parses() {
    let cli = Cli::parse_from(["aletheia", "backup"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            list,
            prune,
            export_json,
            ..
        })) => {
            assert!(!list, "list should be false");
            assert!(!prune, "prune should be false");
            assert!(!export_json, "export_json should be false");
        }
        _ => panic!("expected Backup command"),
    }
}

#[test]
fn backup_list_flag_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "--list"]);
    match cli.command {
        Some(Command::Backup(args)) => {
            assert!(args.list, "assertion failed in backup list flag parses");
        }
        _ => panic!("expected Backup command"),
    }
}

#[test]
fn backup_list_with_json_flag_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "--list", "--json"]);
    match cli.command {
        Some(Command::Backup(args)) => {
            assert!(
                args.list,
                "assertion failed in backup list with json flag parses"
            );
            assert!(
                args.json,
                "assertion failed in backup list with json flag parses"
            );
        }
        _ => panic!("expected Backup command"),
    }
}

#[test]
fn backup_prune_with_keep_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "--prune", "--keep", "3", "--yes"]);
    match cli.command {
        Some(Command::Backup(args)) => {
            assert!(
                args.prune,
                "assertion failed in backup prune with keep parses"
            );
            assert_eq!(args.keep, 3, "keep should equal expected value");
            assert!(
                args.yes,
                "assertion failed in backup prune with keep parses"
            );
        }
        _ => panic!("expected Backup command"),
    }
}

#[test]
fn backup_export_json_flag_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "--export-json"]);
    match cli.command {
        Some(Command::Backup(args)) => assert!(
            args.export_json,
            "assertion failed in backup export json flag parses"
        ),
        _ => panic!("expected Backup command"),
    }
}

#[test]
fn credential_status_parses() {
    let cli = Cli::parse_from(["aletheia", "credential", "status"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Credential {
                action: credential::Action::Status
            })
        ),
        "value should match expected pattern",
    );
}

#[test]
fn credential_refresh_parses() {
    let cli = Cli::parse_from(["aletheia", "credential", "refresh"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Credential {
                action: credential::Action::Refresh
            })
        ),
        "value should match expected pattern",
    );
}

#[test]
fn tls_generate_defaults_parses() {
    let cli = Cli::parse_from(["aletheia", "tls", "generate"]);
    match cli.command {
        Some(Command::Tls {
            action:
                tls::Action::Generate {
                    output_dir,
                    days,
                    force,
                    ..
                },
        }) => {
            assert_eq!(
                output_dir,
                PathBuf::from("instance/config/tls"),
                "output_dir should match result"
            );
            assert_eq!(days, 365, "days should equal expected value");
            assert!(!force, "force should be false");
        }
        _ => panic!("expected Tls Generate command"),
    }
}

#[test]
fn tls_generate_custom_options_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "tls",
        "generate",
        "--output-dir",
        "/tmp/certs",
        "--days",
        "90",
        "--san",
        "example.com",
        "--force",
    ]);
    match cli.command {
        Some(Command::Tls {
            action:
                tls::Action::Generate {
                    output_dir,
                    days,
                    san,
                    force,
                },
        }) => {
            assert_eq!(
                output_dir,
                PathBuf::from("/tmp/certs"),
                "output_dir should match PathBuf::from(\"/tmp/certs\")"
            );
            assert_eq!(days, 90, "days should equal expected value");
            assert!(
                san.contains(&"example.com".to_owned()),
                "san should contain \"example.com\".to_owned()"
            );
            assert!(
                force,
                "assertion failed in tls generate custom options parses"
            );
        }
        _ => panic!("expected Tls Generate command"),
    }
}

#[test]
fn import_minimal_parses() {
    let cli = Cli::parse_from(["aletheia", "import", "/tmp/agent.agent.json"]);
    match cli.command {
        Some(Command::Import(args)) => {
            assert_eq!(
                args.file,
                PathBuf::from("/tmp/agent.agent.json"),
                "file should match json\""
            );
            assert!(args.target_id.is_none(), "target_id should be none");
            assert!(!args.skip_sessions, "skip_sessions should be false");
            assert!(!args.skip_workspace, "skip_workspace should be false");
            assert!(!args.force, "force should be false");
            assert!(!args.dry_run, "dry_run should be false");
        }
        _ => panic!("expected Import command"),
    }
}

#[test]
fn import_with_all_flags_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "import",
        "/tmp/agent.agent.json",
        "--target-id",
        "new-agent",
        "--skip-sessions",
        "--skip-workspace",
        "--force",
        "--dry-run",
    ]);
    match cli.command {
        Some(Command::Import(args)) => {
            assert_eq!(
                args.target_id.as_deref(),
                Some("new-agent"),
                "as_deref( should match Some(\"new-agent\")"
            );
            assert!(
                args.skip_sessions,
                "assertion failed in import with all flags parses"
            );
            assert!(
                args.skip_workspace,
                "assertion failed in import with all flags parses"
            );
            assert!(
                args.force,
                "assertion failed in import with all flags parses"
            );
            assert!(
                args.dry_run,
                "assertion failed in import with all flags parses"
            );
        }
        _ => panic!("expected Import command"),
    }
}

#[test]
fn completions_bash_parses() {
    let cli = Cli::parse_from(["aletheia", "completions", "bash"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Completions {
                shell: clap_complete::Shell::Bash
            })
        ),
        "value should match expected pattern",
    );
}

#[test]
fn completions_zsh_parses() {
    let cli = Cli::parse_from(["aletheia", "completions", "zsh"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Completions {
                shell: clap_complete::Shell::Zsh
            })
        ),
        "value should match expected pattern",
    );
}

#[test]
fn check_config_parses() {
    let cli = Cli::parse_from(["aletheia", "check-config"]);
    assert!(
        matches!(cli.command, Some(Command::CheckConfig)),
        "value should match expected pattern"
    );
}

#[test]
fn add_nous_defaults_parses() {
    let cli = Cli::parse_from(["aletheia", "add-nous", "alice"]);
    match cli.command {
        Some(Command::AddNous(AddNousArgs {
            name,
            provider,
            model,
        })) => {
            assert_eq!(name, "alice", "name should equal expected value");
            assert_eq!(
                provider, "anthropic",
                "provider should equal expected value"
            );
            assert!(!model.is_empty(), "!model should be empty");
        }
        _ => panic!("expected AddNous command"),
    }
}

#[test]
fn add_nous_with_custom_model_parses() {
    let cli = Cli::parse_from([
        "aletheia",
        "add-nous",
        "bob",
        "--provider",
        "anthropic",
        "--model",
        "claude-opus-4-20250514",
    ]);
    match cli.command {
        Some(Command::AddNous(AddNousArgs { name, model, .. })) => {
            assert_eq!(name, "bob", "name should equal expected value");
            assert_eq!(
                model, "claude-opus-4-20250514",
                "model should equal expected value"
            );
        }
        _ => panic!("expected AddNous command"),
    }
}
