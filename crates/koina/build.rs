//! Compile-time validation for the embedded model seed.
//!
//! WHY (#5635): `MODEL_SEED` in `src/models.rs` is initialized from this file at
//! runtime via `toml::from_str`. A malformed file compiles successfully because
//! `include_str!` only checks existence, then panics on first access. Parsing it
//! here converts the production crash path into a build error.
//!
//! The structs below intentionally mirror the private `ModelSeed` types in
//! `src/models.rs`; keep them in sync.

use std::env;
use std::io;
use std::path::PathBuf;

use serde::Deserialize;

// Fields are read only by serde during validation; suppress dead-code noise
// from the build-time lint pass.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ModelSeed {
    as_of: String,
    cache: CacheSeed,
    tiers: TierSeed,
    task_roles: TaskRoleSeed,
    models: Vec<ModelEntry>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct CacheSeed {
    read_ratio: f64,
    write_ratio: f64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct TierSeed {
    opus: String,
    sonnet: String,
    haiku: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct TaskRoleSeed {
    coder: String,
    researcher: String,
    reviewer: String,
    explorer: String,
    runner: String,
    prosoche: String,
    extraction: String,
    triage_prompt: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    provider: String,
    tier: String,
    family: String,
    context_tokens: u32,
    input_cost_per_mtok: Option<f64>,
    output_cost_per_mtok: Option<f64>,
    #[serde(default)]
    menu: bool,
    #[serde(default)]
    recommended: bool,
}

fn main() -> io::Result<()> {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?,
    );
    let seed_path = manifest_dir.join("data/model-seed.toml");

    println!("cargo:rerun-if-changed={}", seed_path.display());

    let seed_text = std::fs::read_to_string(&seed_path)?;
    toml::from_str::<ModelSeed>(&seed_text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e}")))?;

    Ok(())
}
