use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use snafu::ResultExt;

use crate::error::{
    DiscoverySnafu, IdMismatchSnafu, InvalidIdSnafu, NotFoundSnafu, ParseTomlSnafu, ReadThemeSnafu,
    ThemeError,
};
use crate::id::ThemeId;
use crate::resolved::ResolvedTheme;
use crate::tokens::Theme;

/// The themes the registry has loaded, keyed by [`ThemeId`].
///
/// Discovery is filesystem-bounded: a `themes/` directory contains one
/// `<id>.toml` per theme; subdirectories are ignored. The on-disk filename
/// stem is the authoritative `id`, and the `[meta].id` field in the TOML must
/// agree — disagreement is a hard error so a copy-paste of one theme into
/// another file name cannot drift.
///
/// The registry is the runtime-side gate for theme references: a
/// `theme: ThemeId` field on a deliverable spec passes parsing only after
/// [`Registry::get`] returns an entry for it. This is the parse-don't-validate
/// boundary B-002 names: spec authoring sees only theme *names*; the registry
/// is the only place those names are tied to concrete tokens.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    themes: BTreeMap<ThemeId, Theme>,
}

impl Registry {
    /// Construct an empty registry. Time: O(1). Space: O(1).
    #[must_use]
    pub fn new() -> Self {
        Self {
            themes: BTreeMap::new(),
        }
    }

    /// Discover and load every `<id>.toml` in `dir`. Files whose filename
    /// stem fails [`ThemeId::parse`] are skipped (not errors) because a
    /// `themes/` directory may legitimately carry README or schema files.
    ///
    /// Time: O(n) in the number of entries, plus the cost of TOML parsing
    /// each accepted file. Space: O(themes).
    ///
    /// # Errors
    ///
    /// Returns the first error encountered while reading the directory or
    /// parsing a candidate file. Parse failures are surfaced rather than
    /// silenced because a brand TOML that fails to parse is the only signal
    /// the author has that something is wrong.
    pub fn load_dir(dir: &Path) -> Result<Self, ThemeError> {
        let entries = fs::read_dir(dir).context(DiscoverySnafu {
            path: dir.display().to_string(),
        })?;
        let mut registry = Self::new();
        let mut candidates: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.context(DiscoverySnafu {
                path: dir.display().to_string(),
            })?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "toml") {
                candidates.push(path);
            }
        }
        // WHY: directory iteration order is filesystem-dependent. Sort so that
        // the load order is deterministic across hosts and CI runs — every
        // sink relies on the iteration order of `Registry` and `ResolvedTheme`
        // being stable.
        candidates.sort();
        for path in candidates {
            // WHY: a path with a non-UTF8 stem cannot be a valid ThemeId
            // (which is ASCII-bounded), so skipping it is the same outcome
            // as parsing-and-rejecting. The skip branch keeps the loop
            // monotone without surfacing a stem-decode error variant.
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(expected_id) = ThemeId::parse(stem) else {
                continue;
            };
            let raw = fs::read_to_string(&path).context(ReadThemeSnafu {
                path: path.display().to_string(),
            })?;
            let theme: Theme = toml::from_str(&raw).context(ParseTomlSnafu {
                path: path.display().to_string(),
            })?;
            if theme.meta.id != expected_id {
                return IdMismatchSnafu {
                    path: path.clone(),
                    declared: theme.meta.id.as_str().to_owned(),
                    expected: expected_id.as_str().to_owned(),
                }
                .fail();
            }
            registry.themes.insert(expected_id, theme);
        }
        Ok(registry)
    }

    /// Insert a theme programmatically (test helper + future scaffolding path).
    /// Time: O(log n) in the registry size. Space: O(1) amortized.
    pub fn insert(&mut self, theme: Theme) {
        let id = theme.meta.id.clone();
        self.themes.insert(id, theme);
    }

    /// Number of themes carried. Time: O(1). Space: O(1).
    #[must_use]
    pub fn len(&self) -> usize {
        self.themes.len()
    }

    /// Whether the registry is empty. Time: O(1). Space: O(1).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }

    /// All theme ids in sorted order. Used by `theme list` and tests.
    #[must_use]
    pub fn ids(&self) -> Vec<&ThemeId> {
        self.themes.keys().collect()
    }

    /// Borrow the TOML-shape theme by id.
    ///
    /// Time: O(log n) in registry size. Space: O(1).
    ///
    /// # Errors
    ///
    /// Returns [`ThemeError::NotFound`] with the list of known ids attached
    /// when `id` is not in the registry.
    pub fn get(&self, id: &ThemeId) -> Result<&Theme, ThemeError> {
        self.themes.get(id).ok_or_else(|| {
            let available = self
                .themes
                .keys()
                .map(ThemeId::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            NotFoundSnafu {
                theme_id: id.as_str().to_owned(),
                available,
            }
            .build()
        })
    }

    /// Resolve a theme by id into its renderer-facing form.
    ///
    /// WHY: thin delegation — this is the single boundary every sink calls.
    /// Inlining `get(id)?.clone()` + `ResolvedTheme::from_theme(_)` at every
    /// call site would couple the sinks to the registry's internal storage
    /// shape. Keeping the resolve in one place lets the storage evolve (e.g.
    /// to a lazy `OnceCell<ResolvedTheme>` per entry) without touching the
    /// sinks.
    ///
    /// # Errors
    ///
    /// Surfaces every reason [`Registry::get`] or [`ResolvedTheme::from_theme`]
    /// can fail.
    pub fn resolve(&self, id: &ThemeId) -> Result<ResolvedTheme, ThemeError> {
        ResolvedTheme::from_theme(self.get(id)?.clone())
    }
}

/// Return the embedded `summus` theme resolved into renderer-facing tokens.
///
/// WHY: deployed binaries should not depend on a filesystem-bound registry
/// just to obtain the flagship theme. Embedding the seed theme keeps the
/// consumer path deterministic and available in release artifacts.
#[must_use]
pub fn summus() -> ResolvedTheme {
    #[expect(
        clippy::expect_used,
        reason = "embedded summus.toml is valid by construction"
    )]
    fn embedded_summus() -> ResolvedTheme {
        let theme: Theme = toml::from_str(include_str!("../themes/summus.toml"))
            .expect("embedded summus.toml is valid by construction");
        ResolvedTheme::from_theme(theme).expect("embedded summus.toml is valid by construction")
    }

    static SUMMUS: LazyLock<ResolvedTheme> = LazyLock::new(embedded_summus);

    SUMMUS.clone()
}

/// Parse a candidate string into a [`ThemeId`], lifting the parse error into
/// the crate's top-level [`ThemeError`].
///
/// WHY: thin delegation — callers at the registry boundary need a single
/// error type; `ThemeId::parse` returns the narrower `InvalidThemeId`. This
/// helper performs the one-line lift so the call sites don't all need to
/// import [`InvalidIdSnafu`].
///
/// # Errors
///
/// Returns [`ThemeError::InvalidId`] when the candidate fails
/// [`ThemeId::parse`]. The candidate is preserved verbatim on the error so a
/// caller can echo it back in a diagnostic.
pub fn parse_theme_id(candidate: &str) -> Result<ThemeId, ThemeError> {
    ThemeId::parse(candidate).context(InvalidIdSnafu {
        candidate: candidate.to_owned(),
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;

    use super::*;
    use crate::tokens::HexColor;

    const MINI_SUMMUS_TOML: &str = r##"
[meta]
id = "summus"
title = "Summus"

[color.role]
navy = "#232E54"
teal = "#318891"

[color.tone]
positive = "teal"
neutral = "navy"

[color.surface]
page = "navy"

[type.family]
sans = ["Geist", "system-ui"]

[type.scale]
title = 64
"##;

    fn write_theme(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(format!("{name}.toml"));
        let mut f = fs::File::create(&path).expect("create test theme");
        f.write_all(body.as_bytes()).expect("write test theme");
        path
    }

    #[test]
    fn load_dir_discovers_summus() {
        let tmp = tempdir().expect("tempdir");
        write_theme(tmp.path(), "summus", MINI_SUMMUS_TOML);
        let registry = Registry::load_dir(tmp.path()).expect("load_dir");
        assert_eq!(registry.len(), 1);
        let id = parse_theme_id("summus").expect("parse summus");
        let theme = registry.get(&id).expect("get summus");
        assert_eq!(theme.meta.id.as_str(), "summus");
    }

    #[test]
    fn load_dir_skips_non_toml() {
        let tmp = tempdir().expect("tempdir");
        write_theme(tmp.path(), "summus", MINI_SUMMUS_TOML);
        fs::write(tmp.path().join("README.md"), "# brands\n").expect("write README");
        let registry = Registry::load_dir(tmp.path()).expect("load_dir");
        assert_eq!(registry.len(), 1, "README must not register as a theme");
    }

    #[test]
    fn load_dir_skips_invalid_stem() {
        let tmp = tempdir().expect("tempdir");
        write_theme(tmp.path(), "summus", MINI_SUMMUS_TOML);
        fs::write(
            tmp.path().join("Bad.Name.toml"),
            "[meta]\nid = \"summus\"\n",
        )
        .expect("write bad stem");
        let registry = Registry::load_dir(tmp.path()).expect("load_dir");
        assert_eq!(registry.len(), 1, "non-id stem must skip without erroring");
    }

    #[test]
    fn load_dir_rejects_id_mismatch() {
        let tmp = tempdir().expect("tempdir");
        write_theme(tmp.path(), "ardent", MINI_SUMMUS_TOML);
        let err =
            Registry::load_dir(tmp.path()).expect_err("mismatched stem + meta.id must reject");
        assert!(matches!(err, ThemeError::IdMismatch { .. }));
    }

    #[test]
    fn get_missing_attaches_available_list() {
        let tmp = tempdir().expect("tempdir");
        write_theme(tmp.path(), "summus", MINI_SUMMUS_TOML);
        let registry = Registry::load_dir(tmp.path()).expect("load_dir");
        let unknown = parse_theme_id("ardent").expect("parse ardent");
        let err = registry.get(&unknown).expect_err("missing id must reject");
        let msg = format!("{err}");
        assert!(
            msg.contains("summus"),
            "diagnostic must list available themes: {msg}"
        );
    }

    #[test]
    fn resolve_returns_resolved_theme() {
        let tmp = tempdir().expect("tempdir");
        write_theme(tmp.path(), "summus", MINI_SUMMUS_TOML);
        let registry = Registry::load_dir(tmp.path()).expect("load_dir");
        let id = parse_theme_id("summus").expect("parse summus");
        let resolved = registry.resolve(&id).expect("resolve summus");
        assert_eq!(
            resolved.tone.get("positive").map(HexColor::as_str),
            Some("#318891"),
            "tone positive→teal must resolve through the registry"
        );
    }

    #[test]
    fn parse_theme_id_surfaces_invalid_id_error() {
        let err = parse_theme_id("Bad").expect_err("Bad must reject");
        assert!(matches!(err, ThemeError::InvalidId { .. }));
    }
}
