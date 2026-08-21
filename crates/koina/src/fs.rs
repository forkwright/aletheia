//! Restricted filesystem helpers for writing sensitive files.

use std::path::{Path, PathBuf};

/// Reject a config-supplied path before it is joined onto a root directory.
///
/// Rejects an empty path, an absolute path, and any path containing a
/// non-plain component (`..`, `.`, or a root/prefix). This is the pre-join
/// half of path containment: `Path::join` silently discards the base when
/// its argument is itself absolute (`root.join("/etc")` == `/etc`), so a
/// config value meant to name a subdirectory of `root` must be checked
/// *before* joining — by the time an escape reaches
/// [`validate_within_root`], `fs::create_dir_all` may already have run
/// against the escaped path. Requires no filesystem access, so it is safe
/// to call before `root` exists or is even known.
///
/// Pair with [`validate_within_root`] once the joined path exists, to also
/// catch a symlink placed inside `root` that resolves back out of it —
/// something a string-only check cannot see.
///
/// # Errors
///
/// Returns [`std::io::Error`] (`InvalidInput`) if `path` is empty,
/// absolute, or contains any component other than a plain directory/file
/// name (rejects `..`, `.`, and root/prefix components alike).
pub fn reject_path_override(path: &str) -> std::io::Result<()> {
    if path.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path must not be empty",
        ));
    }

    let p = Path::new(path);
    if p.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path must be relative, got absolute path: {path}"),
        ));
    }

    for component in p.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "path contains a disallowed component ({component:?}); only plain \
                     directory/file names are allowed: {path}"
                ),
            ));
        }
    }

    Ok(())
}

/// Validate that `path` resolves within `root` after canonicalization.
///
/// Follows the security standard's path validation sequence:
/// normalize -> check `allowed_roots` -> canonicalize -> re-check `allowed_roots`.
///
/// For paths that do not yet exist on disk, the parent directory is
/// canonicalized and the final component is appended. This handles the
/// common pattern of validating a file path before creating it.
///
/// # Errors
///
/// Returns [`std::io::Error`] if:
/// - The path contains `..` components (pre-canonicalization check).
/// - The canonicalized path does not start with the canonicalized root.
/// - Canonicalization itself fails (e.g. root directory does not exist).
pub fn validate_within_root(path: &Path, root: &Path) -> std::io::Result<PathBuf> {
    // WHY: reject `..` before filesystem access to catch traversal early.
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("path contains '..' component: {}", path.display()),
            ));
        }
    }

    let canonical_root = std::fs::canonicalize(root)?;

    // WHY: the target path may not exist yet (e.g. health check write test,
    // new credential file). Canonicalize the parent, then append the filename.
    let canonical_path = if path.exists() {
        std::fs::canonicalize(path)?
    } else {
        let parent = path.parent().unwrap_or(path);
        let canonical_parent = std::fs::canonicalize(parent)?;
        match path.file_name() {
            Some(name) => canonical_parent.join(name),
            None => canonical_parent,
        }
    };

    // WHY: re-check containment after canonicalization to catch symlink escapes.
    if !canonical_path.starts_with(&canonical_root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "path escapes root: {} is not within {}",
                canonical_path.display(),
                canonical_root.display()
            ),
        ));
    }

    Ok(canonical_path)
}

/// Create `path` and its parents with owner-only (0700) permissions.
///
/// WHY(#5351) a helper rather than `create_dir_all` plus `set_permissions`: the mode
/// goes to the `mkdir(2)` syscall itself, so the directory never exists with
/// umask-permissive bits. A create-then-chmod leaves a window in which another user can
/// enter the directory, and `create_dir_all` under a default 022 umask produces 0755.
///
/// Only the leaf gets the restrictive mode when intermediate directories already exist,
/// which matches `mkdir -p` semantics: this does not tighten directories it did not
/// create, and does not loosen them either.
///
/// # Errors
///
/// Returns an I/O error if the directory cannot be created.
pub fn create_dir_all_restricted(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }

    match builder.create(path) {
        Ok(()) => Ok(()),
        // WHY: `recursive(true)` already tolerates an existing directory, but a race
        // with another process creating it first still surfaces here on some platforms.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(e) => Err(e),
    }
}

/// Open `path` for appending, creating it with owner-only (0600) permissions.
///
/// WHY(#5351) this exists next to [`write_restricted`]: that helper rewrites a whole
/// file atomically, which is wrong for an append-only log -- it would need the previous
/// contents read back and rewritten on every line. Appended files still need the same
/// protection, and the mode is passed to `open(2)` for the same race-closing reason.
///
/// The mode applies only when `open` actually creates the file. An existing file keeps
/// whatever permissions it already has; tightening those is deliberately not done here,
/// because silently changing the mode of a file this process did not create is a
/// surprise, and the caller may not own it.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be opened or created.
pub fn open_append_restricted(path: &Path) -> std::io::Result<std::fs::File> {
    let mut open_options = std::fs::OpenOptions::new();
    open_options.create(true).append(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        open_options.mode(0o600);
    }

    open_options.open(path)
}

/// Write `content` to `path` atomically with 0600 permissions.
///
/// 1. Creates parent directories if needed.
/// 2. Writes to a `.tmp` sibling with mode 0600.
/// 3. Renames atomically to the target path.
///
/// The two-step write prevents other processes from reading a partially-written
/// file and ensures the final file is never world-readable.
///
/// # Errors
///
/// Returns an I/O error if any step (dir creation, write, rename) fails.
pub fn write_restricted(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    if let Some(parent) = path.parent() {
        // WHY(#5351) restricted: this helper exists to keep private content off other
        // users, and a 0600 file inside a 0755 directory still leaks its name, size and
        // mtime -- and lets anyone with write access to that directory replace it.
        create_dir_all_restricted(parent)?;
    }

    let tmp = path.with_extension("tmp");

    {
        let mut open_options = std::fs::OpenOptions::new();
        open_options.write(true).create(true).truncate(true);

        // WHY: pass the restrictive mode to the open(2) syscall itself so the
        // temp file never exists with umask-permissive bits, closing the
        // create-then-chmod race that could leak secret bytes under a
        // permissive umask.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            open_options.mode(0o600);
        }

        let mut file = open_options.open(&tmp)?;

        file.write_all(content)?;
        file.flush()?;
    }

    std::fs::rename(&tmp, path)?;

    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn reject_path_override_accepts_plain_relative_path() {
        assert!(reject_path_override("data/training").is_ok());
    }

    #[test]
    fn reject_path_override_rejects_empty_path() {
        let err = reject_path_override("").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn reject_path_override_rejects_blank_path() {
        let err = reject_path_override("   ").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn reject_path_override_rejects_absolute_path() {
        let err = reject_path_override("/etc/passwd").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn reject_path_override_rejects_dotdot_traversal() {
        let err = reject_path_override("data/../../etc").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn reject_path_override_rejects_dotdot_prefix() {
        let err = reject_path_override("../escape").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn reject_path_override_rejects_leading_curdir_component() {
        // WHY leading, not "data/./training": `Path::components()` silently
        // normalizes a mid-path `.` away (it never yields `CurDir` there),
        // so there is nothing to reject — `data/./training` and
        // `data/training` name the same location. A *leading* `./` does
        // surface as an explicit `Component::CurDir`.
        let err = reject_path_override("./data/training").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn reject_path_override_accepts_mid_path_dot_as_equivalent_to_normalized() {
        // `components()` treats "data/./training" identically to
        // "data/training" — see the WHY above.
        assert!(reject_path_override("data/./training").is_ok());
    }

    #[test]
    fn validate_within_root_accepts_child_path() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("config").join("aletheia.toml");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(&child, b"").unwrap();

        let result = validate_within_root(&child, dir.path());
        assert!(result.is_ok(), "child path should be accepted: {result:?}");
    }

    #[test]
    fn validate_within_root_accepts_nonexistent_file_in_existing_parent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        let nonexistent = dir.path().join("data").join("new-file.txt");

        let result = validate_within_root(&nonexistent, dir.path());
        assert!(
            result.is_ok(),
            "nonexistent file in existing parent should be accepted: {result:?}"
        );
    }

    #[test]
    fn validate_within_root_rejects_dotdot_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let escape = dir
            .path()
            .join("data")
            .join("..")
            .join("..")
            .join("etc")
            .join("passwd");

        let result = validate_within_root(&escape, dir.path());
        assert!(result.is_err(), "path with '..' should be rejected");
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn validate_within_root_rejects_path_outside_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, b"secret").unwrap();

        let result = validate_within_root(&outside_file, root.path());
        assert!(result.is_err(), "path outside root should be rejected");
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn validate_within_root_rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, b"secret").unwrap();

        let link = root.path().join("escape-link");
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();

        let result = validate_within_root(&link, root.path());
        assert!(result.is_err(), "symlink escaping root should be rejected");
    }

    #[test]
    fn validate_within_root_accepts_root_itself() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_within_root(dir.path(), dir.path());
        assert!(result.is_ok(), "root itself should be accepted: {result:?}");
    }

    /// The mode bits a path actually carries on disk.
    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    // WHY these assert an exact mode rather than "no group or other bits": a requested
    // mode only ever has bits REMOVED by the umask, so 0o700 lands as 0o700 under every
    // realistic umask, while the unfixed code lands as 0o777/0o666 minus the umask.
    //
    // That does mean the discriminating power comes from the ambient umask not already
    // being restrictive -- under `umask 077` the unfixed code produces 0o700 too, and
    // these would pass without the fix. That is the right trade rather than a gap worth
    // closing with a umask(2) call here: `umask 077` is also the condition under which
    // the defect cannot be exploited, so the test goes red in exactly the environments
    // where the bug is real. Setting the umask instead would be process-global, and
    // under `cargo test` -- which runs tests as threads, unlike nextest -- it would
    // silently change what every other test in this binary creates.

    #[cfg(unix)]
    #[test]
    fn create_dir_all_restricted_excludes_group_and_other() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("outer").join("inner");

        create_dir_all_restricted(&nested).unwrap();

        assert_eq!(
            mode_of(&nested),
            0o700,
            "a directory holding private content must not be enterable by other users"
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_dir_all_restricted_is_idempotent_on_an_existing_directory() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("twice");

        create_dir_all_restricted(&dir).unwrap();
        create_dir_all_restricted(&dir).unwrap();

        assert!(dir.is_dir(), "a second call must succeed, as mkdir -p does");
    }

    #[cfg(unix)]
    #[test]
    fn open_append_restricted_creates_at_owner_only_and_appends() {
        use std::io::Write as _;

        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("shard.jsonl");

        writeln!(open_append_restricted(&log).unwrap(), "first").unwrap();
        writeln!(open_append_restricted(&log).unwrap(), "second").unwrap();

        assert_eq!(
            mode_of(&log),
            0o600,
            "an append-only log of private content must not be world-readable"
        );
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "first\nsecond\n",
            "the second open must append rather than truncate"
        );
    }

    #[cfg(unix)]
    #[test]
    fn open_append_restricted_leaves_an_existing_files_mode_alone() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempfile::tempdir().unwrap();
        let existing = root.path().join("already-there");
        std::fs::write(&existing, b"x").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o640)).unwrap();

        drop(open_append_restricted(&existing).unwrap());

        // WHY assert the non-tightening rather than treat it as an omission: `mode` on
        // OpenOptions applies only when open(2) creates the file, and silently chmod-ing
        // a file this process did not create is a surprise the caller cannot see. The
        // residue is real and is stated in the helper's own docs -- files written before
        // this change keep their original mode until something recreates them.
        assert_eq!(
            mode_of(&existing),
            0o640,
            "opening an existing file must not silently change its permissions"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_restricted_does_not_leave_its_parent_world_readable() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("created-by-the-helper");
        let target = parent.join("secret.json");

        write_restricted(&target, b"{}").unwrap();

        assert_eq!(mode_of(&target), 0o600, "the file itself");
        // WHY this is the interesting half: a 0600 file inside a 0755 directory still
        // exposes its name, size and mtime to every user on the box, and lets anyone
        // with write access to the directory replace it outright. The helper created
        // this directory, so the directory is its responsibility too.
        assert_eq!(
            mode_of(&parent),
            0o700,
            "the parent this helper created must not be readable by other users"
        );
    }
}
