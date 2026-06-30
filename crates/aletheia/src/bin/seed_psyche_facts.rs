//! Seed identity facts into an episteme cohort keyspace.
//!
//! Generic tool: takes `--cohort <name>` and `--facts-file <path>`.
//! The facts-file is TOML with `nous_id` and an array of `[[fact]]` entries.
//!
//! Idempotent: skips facts whose ID already exists in the cohort keyspace.

#![deny(clippy::unwrap_used)]

use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;
use serde::Deserialize;

#[derive(Debug, Clone, Parser)]
struct Args {
    /// Instance root directory (default: discovered via `ALETHEIA_ROOT` or `./instance`)
    #[arg(short, long)]
    instance_root: Option<PathBuf>,
    /// Cohort name (keyspace subdirectory under knowledge.fjall/)
    #[arg(short, long)]
    cohort: String,
    /// Path to TOML facts file
    #[arg(short, long)]
    facts_file: PathBuf,
}

#[derive(Debug, Deserialize)]
struct FactsFile {
    nous_id: String,
    #[serde(default, rename = "fact")]
    facts: Vec<FactSpec>,
}

#[derive(Debug, Deserialize)]
struct FactSpec {
    id: String,
    content: String,
    #[serde(default = "default_scope")]
    scope: String,
    #[serde(default = "default_tier")]
    tier: String,
    #[serde(default = "default_sensitivity")]
    sensitivity: String,
    #[serde(default = "default_fact_type")]
    fact_type: String,
    #[serde(default = "default_confidence")]
    confidence: f64,
    #[serde(default = "default_stability")]
    stability_hours: f64,
}

fn default_scope() -> String {
    "user".to_owned()
}

fn default_tier() -> String {
    "verified".to_owned()
}

fn default_sensitivity() -> String {
    "confidential".to_owned()
}

fn default_fact_type() -> String {
    "identity".to_owned()
}

fn default_confidence() -> f64 {
    1.0
}

fn default_stability() -> f64 {
    17_520.0
}

#[derive(Debug)]
struct Stats {
    inserted: usize,
    skipped: usize,
    errors: usize,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    #[cfg(feature = "recall")]
    {
        run(args)?;
        Ok(())
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = args;
        anyhow::bail!(
            "seed-psyche-facts requires the 'recall' feature. \
             Build with: cargo build --features recall"
        );
    }
}

#[cfg(feature = "recall")]
fn run(args: Args) -> anyhow::Result<()> {
    use mneme::id::FactId;
    use mneme::knowledge::{
        Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
        MemoryScope, Visibility, far_future,
    };
    use mneme::knowledge_store::KnowledgeStore;

    let facts_toml = std::fs::read_to_string(&args.facts_file)
        .map_err(|e| anyhow::anyhow!("failed to read facts file: {e}"))?;
    let facts_file: FactsFile = toml::from_str(&facts_toml)
        .map_err(|e| anyhow::anyhow!("failed to parse facts file: {e}"))?;

    if facts_file.facts.is_empty() {
        println!("No facts to seed.");
        return Ok(());
    }

    let oikos = match args.instance_root {
        Some(root) => taxis::oikos::Oikos::from_root(&root),
        None => taxis::oikos::Oikos::discover(),
    };
    let cohort_path = oikos.knowledge_cohort_db(&args.cohort);
    std::fs::create_dir_all(&cohort_path)
        .map_err(|e| anyhow::anyhow!("failed to create cohort directory: {e}"))?;

    let knowledge_config = knowledge_config_for_oikos(&oikos);
    let store = KnowledgeStore::open_fjall(&cohort_path, knowledge_config)
        .map_err(|e| anyhow::anyhow!("failed to open knowledge store: {e}"))?;

    let now = jiff::Timestamp::now();
    let mut stats = Stats {
        inserted: 0,
        skipped: 0,
        errors: 0,
    };

    for spec in &facts_file.facts {
        let existing = store
            .read_facts_by_id(&spec.id)
            .map_err(|e| anyhow::anyhow!("failed to query fact {}: {e}", spec.id))?;
        if !existing.is_empty() {
            println!("SKIP {}: already exists", spec.id);
            stats.skipped += 1;
            continue;
        }

        let tier = parse_tier(&spec.tier)?;
        let scope = MemoryScope::from_str(&spec.scope)
            .map_err(|e| anyhow::anyhow!("invalid scope for {}: {e}", spec.id))?;
        let sensitivity = FactSensitivity::from_str(&spec.sensitivity)
            .map_err(|e| anyhow::anyhow!("invalid sensitivity for {}: {e}", spec.id))?;

        let fact = Fact {
            id: FactId::new(&spec.id)
                .map_err(|e| anyhow::anyhow!("invalid fact id {}: {e}", spec.id))?,
            nous_id: facts_file.nous_id.clone(),
            content: spec.content.clone(),
            fact_type: spec.fact_type.clone(),
            scope: Some(scope),
            project_id: None,
            sensitivity,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: now,
                valid_to: far_future(),
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence: spec.confidence,
                tier,
                source_session_id: None,
                stability_hours: spec.stability_hours,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        };

        match store.insert_fact(&fact) {
            Ok(()) => {
                println!("INSERT {}", spec.id);
                stats.inserted += 1;
            }
            Err(e) => {
                eprintln!("ERROR {}: {e}", spec.id);
                stats.errors += 1;
            }
        }
    }

    println!(
        "\nDone: {} inserted, {} skipped, {} errors",
        stats.inserted, stats.skipped, stats.errors
    );

    if stats.errors > 0 {
        anyhow::bail!("{} fact(s) failed to insert", stats.errors);
    }

    Ok(())
}

#[cfg(feature = "recall")]
fn knowledge_config_for_oikos(
    oikos: &taxis::oikos::Oikos,
) -> mneme::knowledge_store::KnowledgeConfig {
    taxis::loader::load_config(oikos).ok().map_or_else(
        mneme::knowledge_store::KnowledgeConfig::default,
        |config| {
            let embedding = config.embedding.to_embedding_config();
            mneme::knowledge_store::KnowledgeConfig {
                dim: config.embedding.dimension,
                embedding_model: embedding.effective_model_name(),
                ..Default::default()
            }
        },
    )
}

#[cfg(feature = "recall")]
fn parse_tier(s: &str) -> anyhow::Result<mneme::knowledge::EpistemicTier> {
    use mneme::knowledge::EpistemicTier;
    match s {
        "verified" => Ok(EpistemicTier::Verified),
        "reflected" => Ok(EpistemicTier::Reflected),
        "inferred" => Ok(EpistemicTier::Inferred),
        "assumed" => Ok(EpistemicTier::Assumed),
        "training" => Ok(EpistemicTier::Training),
        _ => Err(anyhow::anyhow!("unknown epistemic tier: {s}")),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test setup writes files to temp directories; synchronous I/O is required in test contexts"
)]
mod tests {
    use super::*;

    #[test]
    fn seeds_six_identity_facts_into_temp_keyspace() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let facts_file = tmp.path().join("facts.toml");
        std::fs::write(
            &facts_file,
            r#"
nous_id = "test-nous"

[[fact]]
id = "identity-1"
content = "I am the first identity fact."

[[fact]]
id = "identity-2"
content = "I am the second identity fact."

[[fact]]
id = "identity-3"
content = "I am the third identity fact."

[[fact]]
id = "identity-4"
content = "I am the fourth identity fact."

[[fact]]
id = "identity-5"
content = "I am the fifth identity fact."

[[fact]]
id = "identity-6"
content = "I am the sixth identity fact."
"#,
        )
        .expect("write facts file");

        let args = Args {
            instance_root: Some(tmp.path().to_path_buf()),
            cohort: "test-cohort".to_owned(),
            facts_file,
        };

        #[cfg(feature = "recall")]
        {
            run(args).expect("seed should succeed");

            let oikos = taxis::oikos::Oikos::from_root(tmp.path());
            let cohort_path = oikos.knowledge_cohort_db("test-cohort");
            let store = mneme::knowledge_store::KnowledgeStore::open_fjall(
                &cohort_path,
                mneme::knowledge_store::KnowledgeConfig::default(),
            )
            .expect("open store");

            let facts = store
                .query_facts("test-nous", "2099-01-01", 100)
                .expect("query");
            assert_eq!(facts.len(), 6, "all 6 facts should be present");

            for fact in &facts {
                assert_eq!(
                    fact.provenance.tier,
                    mneme::knowledge::EpistemicTier::Verified
                );
                assert_eq!(fact.fact_type, "identity");
                // NOTE: scope and sensitivity are fields on Fact but are not yet
                // persisted in the Datalog schema (tracked in #3413). They
                // default to None/Public on read-back.
            }
        }
    }

    #[test]
    fn idempotent_skip_on_second_run() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let facts_file = tmp.path().join("facts.toml");
        std::fs::write(
            &facts_file,
            r#"
nous_id = "test-nous"

[[fact]]
id = "identity-1"
content = "I am the first identity fact."
"#,
        )
        .expect("write facts file");

        let args = Args {
            instance_root: Some(tmp.path().to_path_buf()),
            cohort: "test-cohort".to_owned(),
            facts_file: facts_file.clone(),
        };

        #[cfg(feature = "recall")]
        {
            run(args.clone()).expect("first seed should succeed");
            run(args).expect("second seed should succeed");

            let oikos = taxis::oikos::Oikos::from_root(tmp.path());
            let cohort_path = oikos.knowledge_cohort_db("test-cohort");
            let store = mneme::knowledge_store::KnowledgeStore::open_fjall(
                &cohort_path,
                mneme::knowledge_store::KnowledgeConfig::default(),
            )
            .expect("open store");

            let facts = store
                .query_facts("test-nous", "2099-01-01", 100)
                .expect("query");
            assert_eq!(facts.len(), 1, "fact should exist exactly once");
        }
    }
}
