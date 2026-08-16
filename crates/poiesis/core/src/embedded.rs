//! The shipped deck component packs, compiled into the binary.
//!
//! [`ComponentRegistry::discover`] reads packs from a real filesystem path —
//! and [`crate::bodies::Deck`] rendering reads each component's template
//! file lazily at render time too, so this is not just a discovery-time
//! concern. A release build is a standalone static binary with no guarantee
//! `crates/poiesis/components/` sits next to it, so the packs are embedded
//! at compile time (`include_dir!`) and materialized to a real directory on
//! first use by [`extract_to`].

use std::path::Path;

use include_dir::{Dir, include_dir};

use crate::components::ComponentRegistry;
use crate::error::RegistryError;

/// The shipped component packs (`crates/poiesis/components/`), embedded at
/// compile time.
static EMBEDDED_COMPONENTS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../components");

/// Write the embedded component packs out to `dest` and discover them into
/// a fresh [`ComponentRegistry`].
///
/// `dest` should be an empty, writable directory (a [`tempfile::TempDir`]
/// in the common case) that the caller keeps alive for as long as the
/// returned registry is used — `Deck` rendering reads each component's
/// template file again at render time, not just here at discovery time.
///
/// # Errors
///
/// Returns [`RegistryError::Io`] if extraction fails, or any
/// [`RegistryError`] variant [`ComponentRegistry::discover`] itself returns
/// for a malformed embedded pack.
pub fn extract_to(dest: &Path) -> Result<ComponentRegistry, RegistryError> {
    EMBEDDED_COMPONENTS
        .extract(dest)
        .map_err(|e| RegistryError::Io {
            path: dest.display().to_string(),
            detail: e.to_string(),
        })?;
    let mut registry = ComponentRegistry::new();
    registry.discover(dest)?;
    Ok(registry)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn embedded_components_extract_and_discover() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let registry = extract_to(tmp.path()).expect("extract embedded components");
        assert!(
            !registry.is_empty(),
            "the shipped component packs must not be empty"
        );
        // WHY this exact id: `title` is one of the shipped packs
        // (crates/poiesis/components/title/) and a stable, unlikely-to-be-
        // renamed anchor for "did discovery actually find real packs".
        let title_id = crate::ids::ComponentId::new("title").expect("valid component id");
        assert!(
            registry.get(&title_id).is_some(),
            "expected the shipped 'title' component pack to be discovered"
        );
    }
}
