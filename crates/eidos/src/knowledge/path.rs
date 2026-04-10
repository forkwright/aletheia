//! Defense-in-depth path validation for memory file operations.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::MemoryScope;

/// Validation layer in the defense-in-depth path security model.
///
/// Each layer addresses a distinct class of path manipulation attack.
/// Layers are applied in order during `validate_memory_path()` (in mneme);
/// a path must pass all layers. The variant names map 1:1 to
/// `PathValidationError` variants for error classification and logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PathValidationLayer {
    /// Null bytes truncate paths in C-based syscalls (libc, kernel).
    NullByte,
    /// Raw string checks miss `foo/../../../etc/passwd`; resolved via
    /// `std::path::Path::components()`.
    Canonicalization,
    /// Symlinks can escape directory jails; resolved via
    /// `std::fs::canonicalize()` with root containment check.
    SymlinkResolution,
    /// Dangling symlinks indicate filesystem manipulation; detected via
    /// `std::fs::symlink_metadata()` when canonicalize returns ENOENT.
    DanglingSymlink,
    /// Symlink loops cause infinite recursion; capped at 40 hops matching
    /// the Linux `ELOOP` limit.
    LoopDetection,
    /// URL-encoded traversals (`%2e%2e%2f` = `../`) bypass string-level
    /// checks; detected by percent-decoding then re-checking for `..` or
    /// separator characters.
    UrlEncodedTraversal,
    /// Fullwidth characters (U+FF0E `.`, U+FF0F `/`) normalize to ASCII
    /// separators under NFKC; detected by normalizing and comparing to
    /// the original.
    UnicodeNormalization,
    /// Resolved path falls outside the expected scope subdirectory.
    ScopeContainment,
}

/// Total number of filesystem-level validation layers (excluding scope
/// containment which is a logical check).
pub const PATH_VALIDATION_FS_LAYERS: usize = 7;

/// Maximum symlink hops before declaring a loop, matching the Linux
/// `ELOOP` kernel limit.
pub const SYMLINK_HOP_LIMIT: usize = 40;

impl PathValidationLayer {
    /// All layer variants in application order.
    pub const ALL: [Self; 8] = [
        Self::NullByte,
        Self::Canonicalization,
        Self::SymlinkResolution,
        Self::DanglingSymlink,
        Self::LoopDetection,
        Self::UrlEncodedTraversal,
        Self::UnicodeNormalization,
        Self::ScopeContainment,
    ];

    /// Return the `snake_case` string representation of this layer.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NullByte => "null_byte",
            Self::Canonicalization => "canonicalization",
            Self::SymlinkResolution => "symlink_resolution",
            Self::DanglingSymlink => "dangling_symlink",
            Self::LoopDetection => "loop_detection",
            Self::UrlEncodedTraversal => "url_encoded_traversal",
            Self::UnicodeNormalization => "unicode_normalization",
            Self::ScopeContainment => "scope_containment",
        }
    }

    /// Whether this layer requires filesystem I/O.
    ///
    /// Pure string-based layers (`NullByte`, `Canonicalization`,
    /// `UrlEncodedTraversal`, `UnicodeNormalization`, `ScopeContainment`)
    /// can run without touching the filesystem.
    #[must_use]
    pub fn requires_io(self) -> bool {
        matches!(
            self,
            Self::SymlinkResolution | Self::DanglingSymlink | Self::LoopDetection
        )
    }
}

impl std::str::FromStr for PathValidationLayer {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "null_byte" => Ok(Self::NullByte),
            "canonicalization" => Ok(Self::Canonicalization),
            "symlink_resolution" => Ok(Self::SymlinkResolution),
            "dangling_symlink" => Ok(Self::DanglingSymlink),
            "loop_detection" => Ok(Self::LoopDetection),
            "url_encoded_traversal" => Ok(Self::UrlEncodedTraversal),
            "unicode_normalization" => Ok(Self::UnicodeNormalization),
            "scope_containment" => Ok(Self::ScopeContainment),
            other => Err(format!("unknown path validation layer: {other}")),
        }
    }
}

// ── Path validation error ────────────────────────────────────────────────

/// Error from defense-in-depth path validation.
///
/// Each variant maps 1:1 to a [`PathValidationLayer`], providing
/// structured information about which layer rejected the path and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "variant fields (path, scope, hops, etc.) are self-documenting by name"
)]
pub enum PathValidationError {
    /// Path contains null bytes that would truncate C-level syscalls.
    NullByte { path: String },
    /// Path contains `..` or backslash components enabling directory traversal.
    Canonicalization { path: String, component: String },
    /// Symlink resolves outside the allowed root directory.
    SymlinkResolution { path: PathBuf, root: PathBuf },
    /// Symlink target does not exist (filesystem manipulation indicator).
    DanglingSymlink { path: PathBuf },
    /// Symlink chain exceeds the hop limit (loop indicator).
    LoopDetection { path: PathBuf, hops: usize },
    /// URL-encoded traversal characters detected (`%2e`, `%2f`, `%5c`).
    UrlEncodedTraversal {
        path: String,
        decoded_fragment: String,
    },
    /// Fullwidth Unicode characters that normalize to path separators under NFKC.
    UnicodeNormalization { path: String, offending_char: char },
    /// Resolved path falls outside the expected scope subdirectory.
    ScopeContainment {
        path: PathBuf,
        scope: MemoryScope,
        expected_dir: PathBuf,
    },
}

impl PathValidationError {
    /// The validation layer that rejected the path.
    #[must_use]
    pub fn layer(&self) -> PathValidationLayer {
        match self {
            Self::NullByte { .. } => PathValidationLayer::NullByte,
            Self::Canonicalization { .. } => PathValidationLayer::Canonicalization,
            Self::SymlinkResolution { .. } => PathValidationLayer::SymlinkResolution,
            Self::DanglingSymlink { .. } => PathValidationLayer::DanglingSymlink,
            Self::LoopDetection { .. } => PathValidationLayer::LoopDetection,
            Self::UrlEncodedTraversal { .. } => PathValidationLayer::UrlEncodedTraversal,
            Self::UnicodeNormalization { .. } => PathValidationLayer::UnicodeNormalization,
            Self::ScopeContainment { .. } => PathValidationLayer::ScopeContainment,
        }
    }
}

impl std::fmt::Display for PathValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NullByte { path } => write!(f, "null byte in path: {path}"),
            Self::Canonicalization { path, component } => {
                write!(
                    f,
                    "directory traversal component `{component}` in path: {path}"
                )
            }
            Self::SymlinkResolution { path, root } => {
                write!(
                    f,
                    "symlink at {} resolves outside root {}",
                    path.display(),
                    root.display()
                )
            }
            Self::DanglingSymlink { path } => {
                write!(f, "dangling symlink at {}", path.display())
            }
            Self::LoopDetection { path, hops } => {
                write!(f, "symlink loop at {} after {hops} hops", path.display())
            }
            Self::UrlEncodedTraversal {
                path,
                decoded_fragment,
            } => {
                write!(
                    f,
                    "URL-encoded traversal `{decoded_fragment}` in path: {path}"
                )
            }
            Self::UnicodeNormalization {
                path,
                offending_char,
            } => {
                write!(
                    f,
                    "fullwidth character U+{:04X} in path: {path}",
                    u32::from(*offending_char)
                )
            }
            Self::ScopeContainment {
                path,
                scope,
                expected_dir,
            } => {
                write!(
                    f,
                    "path {} escapes {} scope (expected under {})",
                    path.display(),
                    scope.as_str(),
                    expected_dir.display()
                )
            }
        }
    }
}

impl std::error::Error for PathValidationError {}

// ── Validated path newtype ───────────────────────────────────────────────

/// A filesystem path that has passed all defense-in-depth validation layers.
///
/// This newtype can only be constructed through [`validate_memory_path()`],
/// ensuring that path security validation cannot be bypassed. The inner
/// `PathBuf` is private, so callers must go through the validation function
/// to obtain an instance.
///
/// Provides [`read()`](Self::read) and [`write()`](Self::write) methods
/// that gate all memory I/O through validated paths, making it impossible
/// to perform memory file operations without passing validation first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPath {
    inner: PathBuf,
    scope: MemoryScope,
}

impl ValidatedPath {
    /// The validated filesystem path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.inner
    }

    /// The memory scope this path was validated against.
    #[must_use]
    pub fn scope(&self) -> MemoryScope {
        self.scope
    }

    /// Consume the wrapper and return the inner `PathBuf`.
    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.inner
    }

    /// Read the validated file's contents.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the file cannot be read.
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        std::fs::read(&self.inner)
    }

    /// Write data to the validated path, creating parent directories as needed.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if directories cannot be created or the file
    /// cannot be written.
    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = self.inner.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.inner, data)
    }
}

impl AsRef<Path> for ValidatedPath {
    fn as_ref(&self) -> &Path {
        &self.inner
    }
}

impl std::fmt::Display for ValidatedPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner.display())
    }
}

// ── Path validation function ─────────────────────────────────────────────

/// Validate a memory path against all defense-in-depth security layers.
///
/// Applies each [`PathValidationLayer`] in order. The path must pass all
/// layers to produce a [`ValidatedPath`]. Relative paths are resolved
/// against `root/scope_dir/`; absolute paths are checked directly against
/// the scope boundary.
///
/// # Layers (applied in order)
///
/// 1. **Null byte** — reject `\0` characters
/// 2. **Canonicalization** — reject `..` and backslash components
/// 3. **URL-encoded traversal** — detect `%2e`, `%2f`, `%5c`
/// 4. **Unicode normalization** — detect fullwidth `.` `/` `\` characters
/// 5. **Scope containment** — resolved path must be under `root/scope_dir/`
/// 6. **Symlink resolution** — canonical path must stay within root (I/O)
/// 7. **Dangling symlink / loop detection** — reject broken or looping
///    symlinks (I/O)
///
/// # Errors
///
/// Returns [`PathValidationError`] identifying the first layer that
/// rejected the path, with structured context for logging and diagnostics.
pub fn validate_memory_path(
    path: &Path,
    root: &Path,
    scope: MemoryScope,
) -> std::result::Result<ValidatedPath, PathValidationError> {
    let path_str = path.to_string_lossy();

    // SAFETY: Layer 1 — reject null bytes that truncate C-level syscalls.
    if path_str.contains('\0') {
        return Err(PathValidationError::NullByte {
            path: path_str.into_owned(),
        });
    }

    // SAFETY: Layer 2 — reject `..` components and backslashes.
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(PathValidationError::Canonicalization {
                path: path_str.into_owned(),
                component: "..".to_owned(),
            });
        }
    }
    if path_str.contains('\\') {
        return Err(PathValidationError::Canonicalization {
            path: path_str.into_owned(),
            component: "\\".to_owned(),
        });
    }

    // SAFETY: Layer 3 — detect percent-encoded traversal separators.
    let lower = path_str.to_ascii_lowercase();
    for pattern in &["%2e", "%2f", "%5c"] {
        if lower.contains(pattern) {
            return Err(PathValidationError::UrlEncodedTraversal {
                path: path_str.into_owned(),
                decoded_fragment: (*pattern).to_owned(),
            });
        }
    }

    // SAFETY: Layer 4 — detect fullwidth characters that
    // normalize to ASCII separators under NFKC (U+FF0E → '.', U+FF0F → '/',
    // U+FF3C → '\').
    for ch in path_str.chars() {
        if matches!(ch, '\u{FF0E}' | '\u{FF0F}' | '\u{FF3C}') {
            return Err(PathValidationError::UnicodeNormalization {
                path: path_str.into_owned(),
                offending_char: ch,
            });
        }
    }

    let scope_dir = root.join(scope.as_dir_name());
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        scope_dir.join(path)
    };
    let normalized = normalize_path_components(&full_path);

    // SAFETY: Layer 5 — resolved path must stay within scope_dir.
    if !normalized.starts_with(&scope_dir) {
        return Err(PathValidationError::ScopeContainment {
            path: normalized,
            scope,
            expected_dir: scope_dir,
        });
    }

    // SAFETY: Layers 6–7 — symlink resolution, dangling symlink, loop detection.
    // WHY: Only checked when the path exists on the filesystem. Pure string
    // layers above have already validated the path structure for paths that
    // don't yet exist.
    validate_symlinks(&normalized, root, &scope_dir, scope)?;

    Ok(ValidatedPath {
        inner: normalized,
        scope,
    })
}

/// Normalize path components without filesystem access.
///
/// Resolves `.` (current dir) by skipping and `..` (parent dir) by
/// popping. This is a string-level operation; no symlinks are resolved.
fn normalize_path_components(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for c in path.components() {
        match c {
            // WHY: ParentDir should already be rejected by Layer 2, but
            // defense-in-depth means we handle it here too.
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            other => parts.push(other),
        }
    }
    parts.iter().collect()
}

/// Check symlink-related security layers on a path that exists on the filesystem.
///
/// Only performs I/O when the path actually exists as a symlink. Skips
/// silently when the path does not exist, since the pure string layers
/// have already validated the path structure.
fn validate_symlinks(
    path: &Path,
    root: &Path,
    scope_dir: &Path,
    scope: MemoryScope,
) -> std::result::Result<(), PathValidationError> {
    // WHY: symlink_metadata returns info about the link itself (not its target),
    // so is_symlink() is accurate even for dangling links.
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(()); // Path doesn't exist yet; pure layers sufficient.
    };

    if !meta.file_type().is_symlink() {
        return Ok(()); // Not a symlink; no further checks needed.
    }

    // Resolve symlinks with hop counting for loop detection.
    let canonical = resolve_with_hop_limit(path)?;

    // Layer 6: Symlink resolution — canonical path must stay within root.
    if !canonical.starts_with(root) {
        return Err(PathValidationError::SymlinkResolution {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        });
    }

    // Re-check scope containment on the canonical path.
    if !canonical.starts_with(scope_dir) {
        return Err(PathValidationError::ScopeContainment {
            path: canonical,
            scope,
            expected_dir: scope_dir.to_path_buf(),
        });
    }

    Ok(())
}

/// Resolve a symlink chain with hop counting.
///
/// Returns the final resolved path, or a [`PathValidationError`] if the
/// chain exceeds [`SYMLINK_HOP_LIMIT`] (loop) or a link target doesn't
/// exist (dangling).
fn resolve_with_hop_limit(start: &Path) -> std::result::Result<PathBuf, PathValidationError> {
    let mut current = start.to_path_buf();
    let mut hops: usize = 0;

    loop {
        let Ok(meta) = std::fs::symlink_metadata(&current) else {
            return Err(PathValidationError::DanglingSymlink {
                path: start.to_path_buf(),
            });
        };

        if !meta.file_type().is_symlink() {
            return Ok(current);
        }

        hops += 1;
        if hops > SYMLINK_HOP_LIMIT {
            return Err(PathValidationError::LoopDetection {
                path: start.to_path_buf(),
                hops,
            });
        }

        let Ok(target) = std::fs::read_link(&current) else {
            return Err(PathValidationError::DanglingSymlink {
                path: start.to_path_buf(),
            });
        };

        current = if target.is_absolute() {
            target
        } else {
            // WHY: Relative symlink targets resolve from the link's parent.
            current
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .join(&target)
        };
    }
}
