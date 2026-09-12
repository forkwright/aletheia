//! Workspace file materialization for daemon-required files.
//!
//! The prosoche heartbeat dispatches "Run your prosoche heartbeat check per
//! PROSOCHE.md." to each nous, but `PROSOCHE.md` was historically written
//! only by the `aletheia init` / `add-nous` scaffolds. A nous whose workspace
//! was created any other way (a config edit plus a directory, a migration, a
//! repointed workspace root) never received the file, so every heartbeat tick
//! failed with "file not found: PROSOCHE.md" (#7191). The daemon repairs this
//! at startup by materializing the file from the embedded template.

use std::io::Write as _;
use std::path::Path;

use snafu::ResultExt;

use crate::error::{self, Result};

/// Bounded-heartbeat checklist template, embedded from the same
/// `instance.example/nous/_template/` source the scaffold documentation
/// names. Embedded (not read from disk) because a deployed instance root has
/// no `instance.example/` tree.
const PROSOCHE_TEMPLATE: &str =
    include_str!("../../../instance.example/nous/_template/PROSOCHE.md");

/// Whether [`materialize_prosoche_md`] wrote the file or found it present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProsocheMaterialization {
    /// `PROSOCHE.md` was absent and was written from the embedded template.
    Materialized,
    /// `PROSOCHE.md` already exists and was left untouched.
    AlreadyPresent,
}

/// Write `PROSOCHE.md` from the embedded template when it is absent from the
/// nous workspace.
///
/// Idempotent and never overwrites: the write uses `create_new`, so an
/// operator-edited checklist always wins over the template. Intended to run
/// once per nous at daemon start; a failure (e.g. read-only workspace) is
/// returned for the caller to log and does not block startup.
///
/// # Errors
///
/// Returns [`error::Error::MaintenanceIo`] when the file cannot be created or
/// written (missing workspace directory, permission denied, ...).
pub fn materialize_prosoche_md(workspace_dir: &Path) -> Result<ProsocheMaterialization> {
    let path = workspace_dir.join("PROSOCHE.md");
    let open = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path);
    let mut file = match open {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Ok(ProsocheMaterialization::AlreadyPresent);
        }
        Err(err) => {
            return Err(err).context(error::MaintenanceIoSnafu {
                context: format!("create {}", path.display()),
            });
        }
    };

    file.write_all(PROSOCHE_TEMPLATE.as_bytes())
        .context(error::MaintenanceIoSnafu {
            context: format!("write {}", path.display()),
        })?;

    Ok(ProsocheMaterialization::Materialized)
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod workspace_tests;
