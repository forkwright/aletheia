//! Compile-time validation for the embedded model seed.
//!
//! WHY (#5635): `MODEL_SEED` in `src/models.rs` is initialized from this file at
//! runtime via `toml::from_str`. A malformed file compiles successfully because
//! `include_str!` only checks existence, then panics on first access. Parsing it
//! here converts the production crash path into a build error.
//!
//! WHY (#7025): the schema below is not a hand-maintained copy of the runtime
//! `ModelSeed` types — it IS them, via `include!` of
//! `src/model_seed_schema.rs`, the single file both this build script and
//! `src/models.rs` splice in. A build that accepts a seed is therefore
//! guaranteed to accept the same seed at runtime, because there is only one
//! schema to accept it against.

use std::env;
use std::io;
use std::path::PathBuf;

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/model_seed_schema.rs"
));

fn main() -> io::Result<()> {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?,
    );
    let seed_path = manifest_dir.join("data/model-seed.toml");
    let schema_path = manifest_dir.join("src/model_seed_schema.rs");

    println!("cargo:rerun-if-changed={}", seed_path.display());
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let seed_text = std::fs::read_to_string(&seed_path)?;
    toml::from_str::<ModelSeed>(&seed_text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e}")))?;

    Ok(())
}
