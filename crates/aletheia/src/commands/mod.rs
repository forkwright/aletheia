//! CLI subcommand handlers: one module per subcommand.

pub(crate) mod add_nous;
pub(crate) mod agent_io;
pub(crate) mod backup;
pub(crate) mod check_config;
pub(crate) mod config;
pub(crate) mod credential;
pub(crate) mod eval;
pub(crate) mod health;
pub(crate) mod maintenance;
pub(crate) mod memory;
pub(crate) mod server;
pub(crate) mod session_export;
pub(crate) mod tls;

use std::path::PathBuf;

use aletheia_taxis::oikos::Oikos;

/// Resolve the instance root and verify it exists.
///
/// Returns a clear error message directing the user to `aletheia init` or `-r`
/// instead of letting downstream code fail with opaque SQLite/config errors.
pub(crate) fn resolve_oikos(instance_root: Option<&PathBuf>) -> crate::error::Result<Oikos> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    if !oikos.root().exists() {
        snafu::whatever!(
            "instance not found at {}\n  \
             Use -r /path/to/instance or set ALETHEIA_ROOT.\n  \
             To create a new instance: aletheia init",
            oikos.root().display()
        );
    }
    Ok(oikos)
}
