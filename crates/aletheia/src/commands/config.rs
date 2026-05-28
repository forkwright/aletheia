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

/// Bundled default-config snapshots, embedded at compile time so version-pair
/// diffs work from any installed binary (not just a source checkout).
///
/// Adding a new snapshot is one line: drop a `vX.Y.Z.toml` under
/// `instance.example/versions/` and append it here.
const BUNDLED_SNAPSHOTS: &[(&str, &str)] = &[(
    "v0.19.0",
    include_str!("../../../../instance.example/versions/v0.19.0.toml"),
)];

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
        /// Compare bundled default config for a version (e.g. v0.19.0).
        /// Run `aletheia config diff --list-versions` to see what's bundled.
        #[arg(long)]
        from_version: Option<String>,
        /// Compare bundled default config for a version (e.g. v0.20.0).
        /// Run `aletheia config diff --list-versions` to see what's bundled.
        #[arg(long)]
        to_version: Option<String>,
        /// List the bundled default-config snapshot versions and exit
        #[arg(long, conflicts_with_all = ["old", "new", "from_version", "to_version"])]
        list_versions: bool,
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
            list_versions,
            json,
        } => {
            if *list_versions {
                run_list_versions(*json);
                return Ok(());
            }
            run_diff(
                old.as_deref(),
                new.as_deref(),
                from_version.as_deref(),
                to_version.as_deref(),
                *json,
            )
        }
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

/// Look up a bundled default-config snapshot by version tag.
///
/// Returns the embedded TOML source. Available versions are listed in
/// `BUNDLED_SNAPSHOTS`.
fn bundled_snapshot(version: &str) -> Option<&'static str> {
    BUNDLED_SNAPSHOTS
        .iter()
        .find_map(|(v, body)| (*v == version).then_some(*body))
}

/// Comma-separated list of bundled snapshot versions, for error messages.
fn bundled_versions_list() -> String {
    BUNDLED_SNAPSHOTS
        .iter()
        .map(|(v, _)| *v)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Source of a config to diff: a filesystem path or an embedded bundled snapshot.
#[derive(Debug)]
enum DiffSource {
    Path(PathBuf),
    Bundled { version: String, body: &'static str },
}

impl DiffSource {
    fn label(&self) -> String {
        match self {
            DiffSource::Path(p) => p.display().to_string(),
            DiffSource::Bundled { version, .. } => format!("bundled:{version}"),
        }
    }

    fn parse_toml(&self) -> Result<toml::Value> {
        match self {
            DiffSource::Path(p) => {
                if !p.exists() {
                    whatever!("config not found: {}", p.display());
                }
                parse_toml_file(p).whatever_context("failed to load config")
            }
            DiffSource::Bundled { version, body } => toml::from_str(body)
                .with_whatever_context(|_| format!("failed to parse bundled snapshot {version}")),
        }
    }
}

fn resolve_version(version: &str) -> Result<DiffSource> {
    match bundled_snapshot(version) {
        Some(body) => Ok(DiffSource::Bundled {
            version: version.to_owned(),
            body,
        }),
        None => whatever!(
            "no bundled snapshot for {version}; available: {}",
            bundled_versions_list()
        ),
    }
}

fn run_list_versions(json: bool) {
    let versions: Vec<&str> = BUNDLED_SNAPSHOTS.iter().map(|(v, _)| *v).collect();
    if json {
        let out = serde_json::to_string_pretty(&versions)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
        println!("{out}");
        return;
    }
    println!("Bundled default-config snapshots ({}):", versions.len());
    for v in &versions {
        println!("  {v}");
    }
}

fn run_diff(
    old: Option<&std::path::Path>,
    new: Option<&std::path::Path>,
    from_version: Option<&str>,
    to_version: Option<&str>,
    json: bool,
) -> Result<()> {
    let (old_src, new_src) = match (old, new, from_version, to_version) {
        (Some(old), Some(new), None, None) => (
            DiffSource::Path(old.to_path_buf()),
            DiffSource::Path(new.to_path_buf()),
        ),
        (None, None, Some(from), Some(to)) => (resolve_version(from)?, resolve_version(to)?),
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

    let old_toml = old_src.parse_toml()?;
    let new_toml = new_src.parse_toml()?;

    let mut old_flat = BTreeMap::new();
    let mut new_flat = BTreeMap::new();
    flatten_toml(&old_toml, "", &mut old_flat);
    flatten_toml(&new_toml, "", &mut new_flat);

    let mut diff = ConfigDiff {
        old: old_src.label(),
        new: new_src.label(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn first_bundled() -> (&'static str, &'static str) {
        // INVARIANT: bundled_snapshots_is_nonempty_and_parses() pins
        // BUNDLED_SNAPSHOTS to be non-empty, so .first() is Some at runtime.
        // We assert here defensively for the case where that ordering is
        // perturbed under nextest / single-test runs.
        let (v, body) = BUNDLED_SNAPSHOTS
            .first()
            .copied()
            .unwrap_or_else(|| panic!("BUNDLED_SNAPSHOTS must be non-empty"));
        (v, body)
    }

    #[test]
    fn bundled_snapshots_is_nonempty_and_parses() {
        assert!(
            !BUNDLED_SNAPSHOTS.is_empty(),
            "BUNDLED_SNAPSHOTS must include at least one version"
        );
        for (version, body) in BUNDLED_SNAPSHOTS {
            toml::from_str::<toml::Value>(body)
                .unwrap_or_else(|e| panic!("bundled snapshot {version} must parse as TOML: {e}"));
        }
    }

    #[test]
    fn bundled_snapshot_lookup_round_trips() {
        for (version, body) in BUNDLED_SNAPSHOTS {
            let looked_up = bundled_snapshot(version)
                .unwrap_or_else(|| panic!("bundled_snapshot must find {version}"));
            assert_eq!(looked_up, *body);
        }
    }

    #[test]
    fn bundled_snapshot_unknown_returns_none() {
        assert!(bundled_snapshot("v999.999.999").is_none());
    }

    #[test]
    fn resolve_version_succeeds_for_bundled() {
        let (version, _) = first_bundled();
        let src = match resolve_version(version) {
            Ok(s) => s,
            Err(e) => panic!("resolve_version must succeed for bundled {version}: {e}"),
        };
        match src {
            DiffSource::Bundled { version: v, .. } => assert_eq!(v, version),
            DiffSource::Path(_) => panic!("expected Bundled source"),
        }
    }

    #[test]
    fn resolve_version_rejects_unbundled_with_available_list() {
        let Err(err) = resolve_version("v0.99.0") else {
            panic!("must reject unbundled version");
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("v0.99.0") && msg.contains("available:"),
            "error must name the missing version and list available ones: {msg}"
        );
        for (v, _) in BUNDLED_SNAPSHOTS {
            assert!(msg.contains(v), "available list must include {v}: {msg}");
        }
    }

    #[test]
    fn version_pair_diff_runs_from_bundled_with_no_filesystem() {
        let (version, _) = first_bundled();
        if let Err(e) = run_diff(None, None, Some(version), Some(version), false) {
            panic!(
                "diff against own bundled snapshot {version} must succeed without filesystem: {e}"
            );
        }
    }
}
