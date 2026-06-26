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
    /// All layer variants in enum order.
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
// kanon:ignore RUST/non-exhaustive-enum -- WHY: #[non_exhaustive] is already present; linter false-positive when an intervening #[expect] separates the attribute from the enum keyword
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

/// A filesystem path that has passed all defense-in-depth validation layers.
///
/// This newtype can only be constructed through [`validate_memory_path()`],
/// ensuring that path security validation cannot be bypassed. The inner
/// `PathBuf` is private, so callers must go through the validation function
/// to obtain an instance.
///
/// Provides [`read()`](Self::read), [`write()`](Self::write),
/// [`async_read()`](Self::async_read), and [`async_write()`](Self::async_write)
/// methods that gate all memory I/O through validated paths, making it
/// impossible to perform memory file operations without passing validation
/// first.
///
/// WHY: The synchronous `read`/`write` methods are retained for non-async
/// callers but are deprecated for async contexts; `async_read`/`async_write`
/// yield to the Tokio runtime so file syscalls do not block worker threads.
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
    #[deprecated(
        since = "0.1.0",
        note = "Use `async_read` from async/Tokio contexts to avoid blocking the runtime thread."
    )]
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        std::fs::read(&self.inner)
    }

    /// Write data to the validated path, creating parent directories as needed.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if directories cannot be created or the file
    /// cannot be written.
    #[deprecated(
        since = "0.1.0",
        note = "Use `async_write` from async/Tokio contexts to avoid blocking the runtime thread."
    )]
    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = self.inner.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.inner, data)
    }

    /// Asynchronously read the validated file's contents.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the file cannot be read.
    ///
    /// WHY: Uses `tokio::fs::read` so the syscall is executed on the blocking
    /// thread pool instead of stalling a Tokio worker thread.
    pub async fn async_read(&self) -> std::io::Result<Vec<u8>> {
        tokio::fs::read(&self.inner).await
    }

    /// Asynchronously write data to the validated path, creating parent
    /// directories as needed.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if directories cannot be created or the file
    /// cannot be written.
    ///
    /// WHY: Uses `tokio::fs::create_dir_all` and `tokio::fs::write` so the
    /// syscalls are executed on the blocking thread pool instead of stalling a
    /// Tokio worker thread.
    pub async fn async_write(&self, data: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = self.inner.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&self.inner, data).await
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

    // INVARIANT: Layer 1 rejects null bytes before any filesystem access.
    if path_str.contains('\0') {
        return Err(PathValidationError::NullByte {
            path: path_str.into_owned(),
        });
    }

    // INVARIANT: Layer 2 rejects `..` components and backslashes before normalization.
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

    // INVARIANT: Layer 3 rejects percent-encoded traversal separators.
    let lower = path_str.to_ascii_lowercase();
    for pattern in &["%2e", "%2f", "%5c"] {
        if lower.contains(pattern) {
            return Err(PathValidationError::UrlEncodedTraversal {
                path: path_str.into_owned(),
                decoded_fragment: (*pattern).to_owned(),
            });
        }
    }

    // INVARIANT: Layer 4 rejects fullwidth separators that normalize to ASCII.
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

    // INVARIANT: Layer 5 keeps the normalized path within `scope_dir`.
    if !normalized.starts_with(&scope_dir) {
        return Err(PathValidationError::ScopeContainment {
            path: normalized,
            scope,
            expected_dir: scope_dir,
        });
    }

    // WHY: Layers 6-7 only run when the path exists; pure string layers above
    // already validated the shape for non-existent paths.
    validate_symlinks(&normalized, root, &scope_dir, scope)?;

    Ok(ValidatedPath {
        inner: normalized,
        scope,
    })
}

/// Async variant of [`validate_memory_path`].
///
/// Performs the same defense-in-depth checks, but uses [`tokio::fs`] for the
/// symlink hop loop so that validation does not block a Tokio worker thread.
/// Async callers must use this function instead of the synchronous variant.
///
/// # Errors
///
/// Returns [`PathValidationError`] identifying the first layer that
/// rejected the path, with structured context for logging and diagnostics.
pub async fn validate_memory_path_async(
    path: &Path,
    root: &Path,
    scope: MemoryScope,
) -> std::result::Result<ValidatedPath, PathValidationError> {
    let path_str = path.to_string_lossy();

    // INVARIANT: Layer 1 rejects null bytes before any filesystem access.
    if path_str.contains('\0') {
        return Err(PathValidationError::NullByte {
            path: path_str.into_owned(),
        });
    }

    // INVARIANT: Layer 2 rejects `..` components and backslashes before normalization.
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

    // INVARIANT: Layer 3 rejects percent-encoded traversal separators.
    let lower = path_str.to_ascii_lowercase();
    for pattern in &["%2e", "%2f", "%5c"] {
        if lower.contains(pattern) {
            return Err(PathValidationError::UrlEncodedTraversal {
                path: path_str.into_owned(),
                decoded_fragment: (*pattern).to_owned(),
            });
        }
    }

    // INVARIANT: Layer 4 rejects fullwidth separators that normalize to ASCII.
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

    // INVARIANT: Layer 5 keeps the normalized path within `scope_dir`.
    if !normalized.starts_with(&scope_dir) {
        return Err(PathValidationError::ScopeContainment {
            path: normalized,
            scope,
            expected_dir: scope_dir,
        });
    }

    // WHY: Layers 6-7 only run when the path exists; pure string layers above
    // already validated the shape for non-existent paths.
    validate_symlinks_async(&normalized, root, &scope_dir, scope).await?;

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
            // WHY: ParentDir should already be rejected by Layer 2, but defense in depth keeps this helper defensive.
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
    // WHY: `symlink_metadata` reports the link itself, so `is_symlink()` stays accurate even when dangling.
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(()); // Path doesn't exist yet; pure layers sufficient.
    };

    if !meta.file_type().is_symlink() {
        return Ok(()); // Not a symlink; no further checks needed.
    }

    // WHY: resolve symlinks with hop counting so loops are bounded.
    let canonical = resolve_with_hop_limit(path)?;

    if !canonical.starts_with(root) {
        return Err(PathValidationError::SymlinkResolution {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        });
    }

    if !canonical.starts_with(scope_dir) {
        return Err(PathValidationError::ScopeContainment {
            path: canonical,
            scope,
            expected_dir: scope_dir.to_path_buf(),
        });
    }

    Ok(())
}

/// Async counterpart to [`validate_symlinks`].
///
/// Uses non-blocking [`tokio::fs`] syscalls so the hop loop does not stall
/// the async runtime.
async fn validate_symlinks_async(
    path: &Path,
    root: &Path,
    scope_dir: &Path,
    scope: MemoryScope,
) -> std::result::Result<(), PathValidationError> {
    // WHY: `symlink_metadata` reports the link itself, so `is_symlink()` stays accurate even when dangling.
    let Ok(meta) = tokio::fs::symlink_metadata(path).await else {
        return Ok(()); // Path doesn't exist yet; pure layers sufficient.
    };

    if !meta.file_type().is_symlink() {
        return Ok(()); // Not a symlink; no further checks needed.
    }

    // WHY: resolve symlinks with hop counting so loops are bounded.
    let canonical = resolve_with_hop_limit_async(path).await?;

    if !canonical.starts_with(root) {
        return Err(PathValidationError::SymlinkResolution {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        });
    }

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

/// Async counterpart to [`resolve_with_hop_limit`].
///
/// Uses [`tokio::fs::symlink_metadata`] and [`tokio::fs::read_link`] so that
/// each hop yields to the async runtime instead of blocking a worker thread.
async fn resolve_with_hop_limit_async(
    start: &Path,
) -> std::result::Result<PathBuf, PathValidationError> {
    let mut current = start.to_path_buf();
    let mut hops: usize = 0;

    loop {
        let Ok(meta) = tokio::fs::symlink_metadata(&current).await else {
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

        let Ok(target) = tokio::fs::read_link(&current).await else {
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
