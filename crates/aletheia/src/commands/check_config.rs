//! `aletheia check-config`: validate configuration without starting any services.

use std::path::PathBuf;
use std::sync::Arc;

use aletheia_taxis::loader::load_config;
use aletheia_taxis::oikos::Oikos;

use crate::error::Result;
use crate::runtime::RuntimeBuilder;

/// Run `aletheia check-config`: load config, validate all sections, report and exit.
///
/// Exits 0 on success, 1 if any check fails.
pub(crate) fn run(instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let config = match load_config(&oikos) {
        Ok(c) => c,
        Err(e) => {
            println!("Instance root: {}", oikos.root().display());
            println!("  [FAIL] config load: {e}");
            snafu::whatever!("config validation aborted: could not load config");
        }
    };

    let builder = RuntimeBuilder::validation_only(Arc::new(oikos), config);
    builder.validate()
}
