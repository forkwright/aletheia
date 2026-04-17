//! `aletheia config`: encryption key management, config encryption, parameter
//! description, and structured config diff.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::Subcommand;
use serde::Serialize;
use snafu::prelude::*;

use taxis::encrypt;
use taxis::loader::parse_toml_file;
use taxis::oikos::Oikos;
use taxis::registry::{self, ParameterTier};

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Generate a new primary encryption key
    InitKey,
    /// Encrypt sensitive plaintext values in aletheia.toml
    Encrypt,
    /// List tunable parameters with metadata, bounds, and tuning guidance
    Describe {
        /// Filter by config section (e.g. "agents.defaults.behavior", "knowledge")
        #[arg(long)]
        section: Option<String>,
        /// Filter by affected subsystem (e.g. "distillation", "competence")
        #[arg(long)]
        affects: Option<String>,
        /// Show only self-tunable parameters
        #[arg(long)]
        self_tunable: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Compare two configuration files and report added, removed, and changed keys
    Diff {
        /// Old config file path
        old: Option<PathBuf>,
        /// New config file path
        new: Option<PathBuf>,
        /// Compare bundled default config for a version (e.g. v0.19.0)
        #[arg(long)]
        from_version: Option<String>,
        /// Compare bundled default config for a version (e.g. v0.20.0)
        #[arg(long)]
        to_version: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(action: &Action, instance_root: Option<&PathBuf>) -> Result<()> {
    match action {
        Action::InitKey => run_init_key(),
        Action::Encrypt => run_encrypt(instance_root),
        Action::Describe {
            section,
            affects,
            self_tunable,
            json,
        } => {
            run_describe(section.as_deref(), affects.as_deref(), *self_tunable, *json);
            Ok(())
        }
        Action::Diff {
            old,
            new,
            from_version,
            to_version,
            json,
        } => run_diff(
            old.as_deref(),
            new.as_deref(),
            from_version.as_deref(),
            to_version.as_deref(),
            *json,
        ),
    }
}

fn run_init_key() -> Result<()> {
    let key_path = encrypt::primary_key_path()
        .ok_or_else(|| crate::error::Error::msg("cannot determine key path: HOME not set"))?;

    println!("Generating primary key at {}", key_path.display());
    encrypt::generate_primary_key(&key_path).whatever_context("failed to generate primary key")?;
    println!("Primary key generated.");
    println!("  Permissions: 0600 (owner read/write only)");
    println!(
        "  Back up this file securely. Without it, encrypted config values cannot be recovered."
    );
    Ok(())
}

fn run_encrypt(instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let key_path = encrypt::primary_key_path()
        .ok_or_else(|| crate::error::Error::msg("cannot determine key path: HOME not set"))?;

    let primary_key = encrypt::load_primary_key(&key_path)
        .whatever_context("failed to load primary key")?
        .ok_or_else(|| {
            crate::error::Error::msg(format!(
                "no primary key found at {}\n  Run `aletheia config init-key` first.",
                key_path.display()
            ))
        })?;

    let toml_path = oikos.config().join("aletheia.toml");
    if !toml_path.exists() {
        whatever!("config file not found: {}", toml_path.display());
    }

    let count = encrypt::encrypt_config_file(&toml_path, &primary_key)
        .whatever_context("failed to encrypt config file")?;

    if count == 0 {
        println!("No plaintext sensitive values found to encrypt.");
    } else {
        println!(
            "Encrypted {count} sensitive value(s) in {}",
            toml_path.display()
        );
    }
    Ok(())
}

fn run_describe(section: Option<&str>, affects: Option<&str>, self_tunable: bool, json: bool) {
    let all = registry::all_specs();

    let filtered: Vec<_> = all
        .iter()
        .filter(|s| {
            if let Some(sec) = section
                && !s.section.contains(sec)
            {
                return false;
            }
            if let Some(aff) = affects
                && !s.affects.contains(aff)
            {
                return false;
            }
            if self_tunable && s.tier != ParameterTier::SelfTuning {
                return false;
            }
            true
        })
        .collect();

    if json {
        // WHY: serde_json::to_string_pretty cannot fail on Vec<&ParameterSpec>
        // since all fields are serializable static types.
        let json_output = serde_json::to_string_pretty(&filtered)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
        println!("{json_output}");
        return;
    }

    if filtered.is_empty() {
        println!("No parameters match the given filters.");
        return;
    }

    println!("{} parameter(s) found:\n", filtered.len());

    for spec in &filtered {
        println!("{}", spec.key);
        println!("  Section:        {}", spec.section);
        println!("  Tier:           {}", spec.tier);
        println!("  Default:        {}", spec.default);
        if let Some((min, max)) = spec.bounds {
            println!("  Bounds:         [{min}, {max}]");
        }
        println!(
            "  Hot-reloadable: {}",
            if spec.hot_reloadable { "yes" } else { "no" }
        );
        println!("  Description:    {}", spec.description);
        println!("  Affects:        {}", spec.affects);
        println!("  Outcome signal: {}", spec.outcome_signal);
        println!("  Evidence:       {}", spec.evidence_required);
        println!("  Direction:      {}", spec.direction_hint);
        println!();
    }
}

// ── Config Diff ────────────────────────────────────────────────────────────

/// Structured result of comparing two flattened config trees.
#[derive(Debug, Serialize)]
struct ConfigDiff {
    old: String,
    new: String,
    added: BTreeMap<String, String>,
    removed: BTreeMap<String, String>,
    changed: BTreeMap<String, ChangeEntry>,
}

#[derive(Debug, Serialize)]
struct ChangeEntry {
    old: String,
    new: String,
}

/// Resolve the path for a versioned default config snapshot.
///
/// Snapshots are stored in `instance.example/versions/` relative to the
/// workspace root. The path is derived from `CARGO_MANIFEST_DIR` at compile
/// time so the command works when run from the source tree.
fn version_snapshot_path(version: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .join("../../instance.example/versions")
        .join(format!("{version}.toml"))
}

fn run_diff(
    old: Option<&std::path::Path>,
    new: Option<&std::path::Path>,
    from_version: Option<&str>,
    to_version: Option<&str>,
    json: bool,
) -> Result<()> {
    let (old_path, new_path) = match (old, new, from_version, to_version) {
        (Some(old), Some(new), None, None) => (old.to_path_buf(), new.to_path_buf()),
        (None, None, Some(from), Some(to)) => {
            (version_snapshot_path(from), version_snapshot_path(to))
        }
        (Some(_), None, _, _) | (None, Some(_), _, _) => {
            whatever!("provide both <old.toml> and <new.toml> arguments");
        }
        (_, _, Some(_), None) | (_, _, None, Some(_)) => {
            whatever!("provide both --from-version and --to-version");
        }
        _ => {
            whatever!("provide either <old.toml> <new.toml> or --from-version and --to-version");
        }
    };

    if !old_path.exists() {
        whatever!("old config not found: {}", old_path.display());
    }
    if !new_path.exists() {
        whatever!("new config not found: {}", new_path.display());
    }

    let old_toml = parse_toml_file(&old_path).whatever_context("failed to load old config")?;
    let new_toml = parse_toml_file(&new_path).whatever_context("failed to load new config")?;

    let mut old_flat = BTreeMap::new();
    let mut new_flat = BTreeMap::new();
    flatten_toml(&old_toml, "", &mut old_flat);
    flatten_toml(&new_toml, "", &mut new_flat);

    let mut diff = ConfigDiff {
        old: old_path.display().to_string(),
        new: new_path.display().to_string(),
        added: BTreeMap::new(),
        removed: BTreeMap::new(),
        changed: BTreeMap::new(),
    };

    for (key, val) in &new_flat {
        match old_flat.get(key) {
            None => {
                diff.added.insert(key.clone(), val.clone());
            }
            Some(old_val) if old_val != val => {
                diff.changed.insert(
                    key.clone(),
                    ChangeEntry {
                        old: old_val.clone(),
                        new: val.clone(),
                    },
                );
            }
            Some(_) => {} // unchanged
        }
    }

    for (key, val) in &old_flat {
        if !new_flat.contains_key(key) {
            diff.removed.insert(key.clone(), val.clone());
        }
    }

    if json {
        let out = serde_json::to_string_pretty(&diff)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
        println!("{out}");
        return Ok(());
    }

    // Human-readable output
    println!("Config diff: {} → {}", diff.old, diff.new);
    println!();

    if diff.added.is_empty() && diff.removed.is_empty() && diff.changed.is_empty() {
        println!("No differences found.");
        return Ok(());
    }

    if !diff.added.is_empty() {
        println!("Added ({}):", diff.added.len());
        for (key, val) in &diff.added {
            println!("  + {key} = {val}");
        }
        println!();
    }

    if !diff.removed.is_empty() {
        println!("Removed ({}):", diff.removed.len());
        for (key, val) in &diff.removed {
            println!("  - {key} = {val}");
        }
        println!();
    }

    if !diff.changed.is_empty() {
        println!("Changed ({}):", diff.changed.len());
        for (key, change) in &diff.changed {
            println!("  ~ {key}: {} → {}", change.old, change.new);
        }
    }

    Ok(())
}

/// Recursively flatten a TOML value tree into a sorted dotted-key map.
///
/// Arrays are indexed with `[n]` notation. Scalar values are formatted with
/// their canonical TOML representation.
fn flatten_toml(value: &toml::Value, prefix: &str, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            if table.is_empty() {
                out.insert(prefix.to_owned(), "{}".to_owned());
                return;
            }
            for (key, val) in table {
                let new_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_toml(val, &new_prefix, out);
            }
        }
        toml::Value::Array(arr) => {
            if arr.is_empty() {
                out.insert(prefix.to_owned(), "[]".to_owned());
                return;
            }
            for (i, item) in arr.iter().enumerate() {
                let new_prefix = format!("{prefix}[{i}]");
                flatten_toml(item, &new_prefix, out);
            }
        }
        scalar => {
            let formatted = scalar.to_string().trim().to_owned();
            out.insert(prefix.to_owned(), formatted);
        }
    }
}
