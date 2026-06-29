//! Unit tests for CLI argument parsing.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::path::PathBuf;

use clap::Parser;

use super::{
    Cli, Command, commands::add_nous::AddNousArgs, commands::agent_io::InitArgs,
    commands::credential, commands::maintenance, commands::memory,
    commands::session_create::SessionCreateArgs, commands::session_export::ExportFormat,
    commands::tls,
};

mod backup_tests;

#[test]
fn cli_help_works() {
    let result = Cli::try_parse_from(["aletheia", "--help"]);
    assert!(result.is_err(), "help flag should produce an error result");
    let err = result.expect_err("help flag should produce an error result");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::DisplayHelp,
        "error kind should be DisplayHelp"
    );
}

#[test]
fn cli_defaults() {
    let cli = Cli::parse_from(["aletheia"]);
    assert!(cli.port.is_none(), "port should default to none");
    assert!(cli.bind.is_none(), "bind should default to none");
    assert_eq!(cli.log_level, "info", "log level should default to info");
    assert!(!cli.json_logs, "json logs should default to false");
    assert!(cli.command.is_none(), "command should default to none");
}

#[test]
fn health_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "health", "--url", "http://localhost:9999"]);
    assert!(
        matches!(cli.command, Some(Command::Health(_))),
        "health subcommand should parse"
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
        "maintenance status subcommand should parse"
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
        "maintenance run subcommand should parse"
    );
}

#[test]
fn status_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "status"]);
    assert!(
        matches!(cli.command, Some(Command::Status { .. })),
        "status subcommand should parse"
    );
}

#[test]
fn status_custom_url_parses() {
    let cli = Cli::parse_from(["aletheia", "status", "--url", "http://example:9999"]);
    match cli.command {
        Some(Command::Status { url }) => {
            assert_eq!(url, "http://example:9999", "custom url should be set");
        }
        _ => panic!("expected Status command"),
    }
}

#[test]
fn memory_reembed_parses() {
    let cli = Cli::parse_from(["aletheia", "memory", "reembed"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Memory {
                action: memory::Action::Reembed,
                ..
            })
        ),
        "memory reembed subcommand should parse"
    );
}

#[test]
fn memory_gc_parses() {
    let cli = Cli::parse_from(["aletheia", "memory", "gc"]);
    assert!(
        matches!(
            cli.command,
            Some(Command::Memory {
                action: memory::Action::Gc,
                ..
            })
        ),
        "memory gc subcommand should parse"
    );
}

#[test]
fn eval_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "eval"]);
    match cli.command {
        Some(Command::Eval(args)) => {
            assert_eq!(
                args.coverage_policy,
                dokimion::coverage::Policy::Ci,
                "eval should default to CI coverage policy"
            );
        }
        _ => panic!("expected Eval command"),
    }
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
        "--coverage-policy",
        "smoke-dev",
    ]);
    match cli.command {
        Some(Command::Eval(args)) => {
            assert_eq!(args.url, "http://example:9999", "url should be set");
            assert_eq!(
                args.token.as_deref(),
                Some("my-jwt-token"),
                "token should be set"
            );
            assert_eq!(
                args.scenario.as_deref(),
                Some("health"),
                "scenario should be set"
            );
            assert!(args.json, "json flag should be set");
            assert_eq!(args.timeout, 60, "timeout should be set");
            assert_eq!(
                args.coverage_policy,
                dokimion::coverage::Policy::SmokeDev,
                "coverage policy should be set"
            );
        }
        _ => panic!("expected Eval command"),
    }
}

#[test]
fn export_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "export", "syn", "--archived", "--compact"]);
    match cli.command {
        Some(Command::Export(args)) => {
            assert_eq!(args.nous_id, "syn", "nous_id should be set");
            assert!(args.archived, "archived flag should be set");
            assert!(args.compact, "compact flag should be set");
            assert_eq!(args.max_messages, 500, "max_messages should default to 500");
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
            assert_eq!(args.nous_id, "demiurge", "nous_id should be set");
            assert_eq!(
                args.output.expect("export output path should be set"),
                PathBuf::from("/tmp/backup.agent.json"),
                "output path should be set"
            );
            assert_eq!(args.max_messages, 100, "max_messages should be set");
        }
        _ => panic!("expected Export command"),
    }
}

#[test]
fn session_create_defaults_parses() {
    let cli = Cli::parse_from(["aletheia", "session-create", "alice"]);
    match cli.command {
        Some(Command::SessionCreate(SessionCreateArgs { nous_id, key })) => {
            assert_eq!(nous_id, "alice", "nous_id should be set");
            assert_eq!(key, "main", "key should default to main");
        }
        _ => panic!("expected SessionCreate command"),
    }
}

#[test]
fn session_create_with_key_parses() {
    let cli = Cli::parse_from(["aletheia", "session-create", "bob", "--key", "custom-key"]);
    match cli.command {
        Some(Command::SessionCreate(SessionCreateArgs { nous_id, key })) => {
            assert_eq!(nous_id, "bob", "nous_id should be set");
            assert_eq!(key, "custom-key", "key should be set");
        }
        _ => panic!("expected SessionCreate command"),
    }
}

#[test]
fn session_export_defaults_to_markdown() {
    let cli = Cli::parse_from(["aletheia", "session-export", "01JBVK0000000000000000000A"]);
    match cli.command {
        Some(Command::SessionExport(args)) => {
            assert_eq!(
                args.session_id, "01JBVK0000000000000000000A",
                "session_id should be set"
            );
            assert!(
                matches!(args.format, ExportFormat::Md),
                "format should default to markdown"
            );
            assert!(args.output.is_none(), "output should default to none");
            assert_eq!(
                args.url, "http://127.0.0.1:18789",
                "url should default to localhost"
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
                "format should be json"
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
                args.output
                    .expect("session-export output path should be set"),
                PathBuf::from("/tmp/session.md"),
                "output path should be set"
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
                instance_root.expect("instance_root should be set"),
                PathBuf::from("/tmp/test-instance"),
                "instance_root should be set"
            );
            assert!(non_interactive, "non_interactive flag should be set");
            assert!(!yes, "yes flag should default to false");
            assert!(api_key.is_none(), "api_key should default to none");
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
                instance_root.expect("instance_root should be set"),
                PathBuf::from("/srv/aletheia"),
                "instance_root should be set"
            );
            assert!(non_interactive, "non_interactive flag should be set");
            assert_eq!(
                auth_mode.as_deref(),
                Some("token"),
                "auth_mode should be set"
            );
            assert_eq!(
                api_provider.as_deref(),
                Some("anthropic"),
                "api_provider should be set"
            );
            assert_eq!(
                model.as_deref(),
                Some("claude-opus-4-6"),
                "model should be set"
            );
            assert_eq!(
                api_key.as_deref(),
                Some("sk-ant-test"),
                "api_key should be set"
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
            assert!(yes, "yes flag should be set");
            assert!(!non_interactive, "non_interactive should default to false");
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
                instance_root.expect("instance_root alias should be set"),
                PathBuf::from("/custom/path"),
                "instance_root alias should be accepted"
            );
        }
        _ => panic!("expected Init command"),
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
        "credential status subcommand should parse"
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
        "credential refresh subcommand should parse"
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
                "output_dir should default to instance/config/tls"
            );
            assert_eq!(days, 365, "days should default to 365");
            assert!(!force, "force flag should default to false");
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
                "output_dir should be set"
            );
            assert_eq!(days, 90, "days should be set");
            assert!(
                san.contains(&"example.com".to_owned()),
                "san should contain example.com"
            );
            assert!(force, "force flag should be set");
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
                "file path should be set"
            );
            assert!(args.target_id.is_none(), "target_id should default to none");
            assert!(!args.skip_sessions, "skip_sessions should default to false");
            assert!(
                !args.skip_workspace,
                "skip_workspace should default to false"
            );
            assert!(!args.force, "force should default to false");
            assert!(!args.dry_run, "dry_run should default to false");
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
                "target_id should be set"
            );
            assert!(args.skip_sessions, "skip_sessions flag should be set");
            assert!(args.skip_workspace, "skip_workspace flag should be set");
            assert!(args.force, "force flag should be set");
            assert!(args.dry_run, "dry_run flag should be set");
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
        "completions bash subcommand should parse"
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
        "completions zsh subcommand should parse"
    );
}

#[test]
fn check_config_parses() {
    let cli = Cli::parse_from(["aletheia", "check-config"]);
    assert!(
        matches!(cli.command, Some(Command::CheckConfig)),
        "check-config subcommand should parse"
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
            assert_eq!(name, "alice", "name should be set");
            assert_eq!(
                provider, "anthropic",
                "provider should default to anthropic"
            );
            assert!(!model.is_empty(), "model should have a default value");
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
            assert_eq!(name, "bob", "name should be set");
            assert_eq!(model, "claude-opus-4-20250514", "model should be set");
        }
        _ => panic!("expected AddNous command"),
    }
}
