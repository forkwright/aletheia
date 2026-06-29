use std::path::PathBuf;

use clap::{Parser, error::ErrorKind};

use super::super::{
    Cli, Command,
    commands::backup::{BackupAction, BackupArgs},
};

#[test]
fn backup_default_parses() {
    let cli = Cli::parse_from(["aletheia", "backup"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: None,
            list,
            prune,
            ..
        })) => {
            assert!(!list, "list flag should default to false");
            assert!(!prune, "prune flag should default to false");
        }
        _ => panic!("expected Backup command with no action"),
    }
}

#[test]
fn backup_list_flag_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "--list"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: None, list, ..
        })) => assert!(list, "list flag should be set"),
        _ => panic!("expected Backup command"),
    }
}

#[test]
fn backup_list_with_json_flag_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "--list", "--json"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: None,
            list,
            json,
            ..
        })) => {
            assert!(list, "list flag should be set");
            assert!(json, "json flag should be set");
        }
        _ => panic!("expected Backup command"),
    }
}

#[test]
fn backup_prune_with_keep_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "--prune", "--keep", "3", "--yes"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: None,
            prune,
            keep,
            yes,
            ..
        })) => {
            assert!(prune, "prune flag should be set");
            assert_eq!(keep, Some(3), "keep count should be set");
            assert!(yes, "yes flag should be set");
        }
        _ => panic!("expected Backup command"),
    }
}

#[test]
fn backup_legacy_flag_conflicts_fail_during_parse() {
    for (argv, expected) in [
        (
            vec!["aletheia", "backup", "--list", "--prune"],
            ErrorKind::ArgumentConflict,
        ),
        (
            vec!["aletheia", "backup", "--keep", "3"],
            ErrorKind::MissingRequiredArgument,
        ),
        (
            vec!["aletheia", "backup", "--json"],
            ErrorKind::MissingRequiredArgument,
        ),
        (
            vec!["aletheia", "backup", "--list", "list"],
            ErrorKind::ArgumentConflict,
        ),
    ] {
        let err =
            Cli::try_parse_from(argv).expect_err("legacy backup flags should fail during parse");
        assert_eq!(err.kind(), expected);
    }
}

#[test]
fn backup_list_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "list"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: Some(BackupAction::List { json }),
            ..
        })) => assert!(!json, "json should default to false"),
        _ => panic!("expected Backup List subcommand"),
    }
}

#[test]
fn backup_list_subcommand_with_json_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "list", "--json"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: Some(BackupAction::List { json }),
            ..
        })) => assert!(json, "json should be set"),
        _ => panic!("expected Backup List subcommand"),
    }
}

#[test]
fn backup_prune_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "prune", "--keep", "7", "--yes"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: Some(BackupAction::Prune { keep, yes }),
            ..
        })) => {
            assert_eq!(keep, Some(7), "keep should be set");
            assert!(yes, "yes should be set");
        }
        _ => panic!("expected Backup Prune subcommand"),
    }
}

#[test]
fn backup_verify_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "verify", "/tmp/backup"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: Some(BackupAction::Verify { path }),
            ..
        })) => {
            assert_eq!(path, PathBuf::from("/tmp/backup"), "path should be set");
        }
        _ => panic!("expected Backup Verify subcommand"),
    }
}

#[test]
fn backup_create_subcommand_parses() {
    let cli = Cli::parse_from(["aletheia", "backup", "create"]);
    match cli.command {
        Some(Command::Backup(BackupArgs {
            action: Some(BackupAction::Create),
            ..
        })) => {}
        _ => panic!("expected Backup Create subcommand"),
    }
}
