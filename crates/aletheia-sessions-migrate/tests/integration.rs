#![expect(
    dead_code,
    reason = "integration harness mounts binary modules without every CLI-only path"
)]

#[path = "integration/common/mod.rs"]
mod common;
#[path = "../src/dest.rs"]
mod dest;
#[path = "../src/error.rs"]
mod error;
#[path = "../src/migrate.rs"]
mod migrate;
#[path = "../src/schema.rs"]
mod schema;
#[path = "../src/source.rs"]
mod source;
#[path = "../src/verify.rs"]
mod verify;

use std::path::Path;

const DEFAULT_VERIFY_SAMPLES: usize = 16;

fn run_migration(
    source: &Path,
    dest: &Path,
    replace_existing: bool,
) -> error::Result<migrate::MigrationReport> {
    let staged = migrate::stage_migration(source, dest, replace_existing)?;
    let verification = staged.verify(source, DEFAULT_VERIFY_SAMPLES)?;
    if !verification.ok() {
        return Err(error::VerificationFailedSnafu {
            mismatches: verification.mismatches.len(),
            summary: verification.mismatches.join("; "),
        }
        .build());
    }
    staged.publish()
}

#[path = "integration/idempotency.rs"]
mod idempotency;
#[path = "integration/legacy_extras.rs"]
mod legacy_extras;
#[path = "integration/orphan_recovery.rs"]
mod orphan_recovery;
#[path = "integration/round_trip_integrity.rs"]
mod round_trip_integrity;
#[path = "integration/schema_mismatch.rs"]
mod schema_mismatch;
#[path = "integration/staging_durability.rs"]
mod staging_durability;
#[path = "integration/strict_enums.rs"]
mod strict_enums;
#[path = "integration/tiny_synthetic.rs"]
mod tiny_synthetic;
#[path = "integration/verification_completeness.rs"]
mod verification_completeness;
