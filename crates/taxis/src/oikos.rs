//! Oikos path resolution.
//!
//! The oikos is the instance directory structure. All paths in Aletheia resolve
//! relative to the instance root. Environment variable `ALETHEIA_ROOT` overrides
//! the default.

use std::path::{Path, PathBuf};

use snafu::{ResultExt, ensure};

use koina::id::NousId;
use koina::system::{Environment, RealSystem};

/// The oikos: resolved instance paths.
///
/// All paths are absolute. Construct via [`Oikos::discover`] or [`Oikos::from_root`].
#[derive(Debug, Clone)]
pub struct Oikos {
    root: PathBuf,
}

impl Oikos {
    /// Create an oikos from an explicit root path.
    ///
    /// If the path exists on disk, it is canonicalized to an absolute path so
    /// that all derived paths (data, logs, etc.) are absolute and work
    /// regardless of the caller's current working directory. If the path does
    /// not yet exist (e.g. during `init`), the path is stored as-is.
    #[must_use]
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // WHY: canonicalize converts relative paths (./instance, instance) to
        // absolute paths so that existence checks in print_storage and
        // validate_workspace_path are cwd-independent. Fall back to the raw
        // path when the directory does not yet exist (e.g. during init).
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Self { root }
    }

    /// Discover the oikos root using the real process environment.
    ///
    /// Resolution order:
    /// 1. `ALETHEIA_ROOT` environment variable
    /// 2. `./instance` relative to current directory
    ///
    /// Call [`Oikos::discover_with`] to supply a custom [`Environment`]
    /// implementation (e.g. `koina::system::TestSystem` in tests).
    #[must_use]
    pub fn discover() -> Self {
        Self::discover_with(&RealSystem)
    }

    /// Discover the oikos root using the provided [`Environment`].
    ///
    /// Resolution order:
    /// 1. `ALETHEIA_ROOT` environment variable (from `env`)
    /// 2. `./instance` relative to current directory
    ///
    /// This variant is the primary implementation; [`Oikos::discover`] is a
    /// convenience wrapper that passes [`RealSystem`].
    ///
    /// WHY: delegates to `from_root` so the discovered path is canonicalized
    /// when it exists. This ensures existence checks (e.g. `print_storage` in
    /// `aletheia status`) are cwd-independent and work with non-default paths
    /// and symlinks.
    #[must_use]
    pub fn discover_with(env: &impl Environment) -> Self {
        let root = env
            .var("ALETHEIA_ROOT")
            .map_or_else(|| PathBuf::from("instance"), PathBuf::from);
        Self::from_root(root)
    }

    /// The instance root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Shared workspace directory for desktop file-browsing surfaces.
    ///
    /// The desktop app uses this as the default workspace root when the
    /// instance provides one. Callers must still validate the path exists
    /// before dereferencing it.
    #[must_use]
    pub fn workspace_root(&self) -> PathBuf {
        self.root.join("nous").join("workspace")
    }

    /// The theke directory (human + nous collaborative space).
    #[must_use]
    pub fn theke(&self) -> PathBuf {
        self.root.join("theke")
    }

    /// The shared directory (nous-only shared resources).
    #[must_use]
    pub fn shared(&self) -> PathBuf {
        self.root.join("shared")
    }

    /// A specific agent's workspace directory.
    #[must_use]
    pub fn nous_dir(&self, id: &str) -> PathBuf {
        self.root.join("nous").join(id)
    }

    /// A specific file within an agent's workspace.
    #[must_use]
    pub fn nous_file(&self, id: &str, filename: &str) -> PathBuf {
        self.nous_dir(id).join(filename)
    }

    /// A canonical file path within a validated agent workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be resolved or the resolved path is
    /// outside the instance root.
    pub fn contained_nous_file(
        &self,
        id: &NousId,
        filename: &str,
    ) -> crate::error::Result<PathBuf> {
        self.canonical_path_under_root(self.nous_file(id.as_str(), filename))
    }

    /// The config directory.
    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    /// The credentials directory.
    #[must_use]
    pub fn credentials(&self) -> PathBuf {
        self.root.join("config").join("credentials")
    }

    /// The data directory (runtime state).
    #[must_use]
    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    /// The sessions database file.
    #[must_use]
    pub fn sessions_db(&self) -> PathBuf {
        self.root.join("data").join("sessions.db")
    }

    /// The knowledge store directory (fjall persistent storage).
    #[must_use]
    pub fn knowledge_db(&self) -> PathBuf {
        self.root.join("data").join("knowledge.fjall")
    }

    /// The working checkpoint store directory (fjall persistent storage).
    #[must_use]
    pub fn working_checkpoint_db(&self) -> PathBuf {
        self.root.join("data").join("working-checkpoints.fjall")
    }

    /// The knowledge store directory for a single episteme cohort.
    #[must_use]
    pub fn knowledge_cohort_db(&self, cohort: &str) -> PathBuf {
        self.knowledge_db().join(cohort)
    }

    /// The backups directory.
    #[must_use]
    pub fn backups(&self) -> PathBuf {
        self.root.join("data").join("backups")
    }

    /// The archive directory (retained session exports).
    #[must_use]
    pub fn archive(&self) -> PathBuf {
        self.root.join("data").join("archive")
    }

    /// The logs directory.
    #[must_use]
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Trace files directory.
    #[must_use]
    pub fn traces(&self) -> PathBuf {
        self.root.join("logs").join("traces")
    }

    /// Trace archive directory.
    #[must_use]
    pub fn trace_archive(&self) -> PathBuf {
        self.root.join("logs").join("traces").join("archive")
    }

    /// Validate the instance layout at startup.
    ///
    /// Checks that:
    /// - The root directory exists.
    /// - `config/` and `data/` subdirectories exist.
    /// - `data/` is writable.
    /// - Emits a warning if `nous/` is absent (first-run scenario).
    ///
    /// Call this once, immediately after constructing the `Oikos`, before
    /// starting any actors or loading the session store.
    ///
    /// # Errors
    ///
    /// Returns the first validation failure encountered.
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::{InstanceRootNotFoundSnafu, RequiredDirMissingSnafu};

        ensure!(
            self.root.exists(),
            InstanceRootNotFoundSnafu {
                path: self.root.clone()
            }
        );

        for dir in &["config", "data"] {
            let path = self.root.join(dir);
            ensure!(path.exists(), RequiredDirMissingSnafu { path });
        }

        let nous_dir = self.root.join("nous");
        if !nous_dir.exists() {
            tracing::warn!(
                path = %nous_dir.display(),
                "nous/ directory not found, no agents will load on this path"
            );
        }

        Self::check_writable(&self.root.join("data"))?;

        Ok(())
    }

    /// Validate that a workspace path from agent config resolves to an existing directory.
    ///
    /// Relative paths are resolved against the instance root. Absolute paths are
    /// used as-is.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not
    /// exist or is not a directory.
    // kanon:ignore RUST/validate-returns-unit — returns Result<()> where Err carries the specific failure reason; Ok(()) signals validation passed
    pub fn validate_workspace_path(&self, workspace: &str) -> crate::error::Result<()> {
        use crate::error::WorkspacePathInvalidSnafu;

        let path = if Path::new(workspace).is_absolute() {
            PathBuf::from(workspace)
        } else {
            self.root.join(workspace)
        };

        ensure!(path.is_dir(), WorkspacePathInvalidSnafu { path });

        Ok(())
    }

    fn check_writable(path: &Path) -> crate::error::Result<()> {
        use crate::error::NotWritableSnafu;

        let test_file = path.join(".aletheia-write-test");
        #[expect(
            clippy::disallowed_methods,
            reason = "taxis config operations are CLI-invoked and require synchronous filesystem access"
        )]
        std::fs::write(&test_file, b"ok").context(NotWritableSnafu {
            path: path.to_path_buf(),
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // kanon:ignore RUST/no-silent-result-swallow — best-effort cleanup of transient test file; failure is non-fatal
            let _ = std::fs::set_permissions(&test_file, std::fs::Permissions::from_mode(0o600));
        }
        // kanon:ignore RUST/no-silent-result-swallow — best-effort cleanup of transient test file; failure is non-fatal
        let _ = std::fs::remove_file(&test_file);
        Ok(())
    }

    fn canonical_path_under_root(&self, path: PathBuf) -> crate::error::Result<PathBuf> {
        use crate::error::{PathOutsideRootSnafu, ResolvePathSnafu};

        let root = std::fs::canonicalize(&self.root).context(ResolvePathSnafu {
            path: self.root.clone(),
        })?;
        let canonical = std::fs::canonicalize(&path).context(ResolvePathSnafu { path })?;
        ensure!(
            canonical.starts_with(&root),
            PathOutsideRootSnafu {
                path: canonical.clone(),
                root
            }
        );
        Ok(canonical)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[cfg(test)]
    impl Oikos {
        /// Shared tools directory.
        #[must_use]
        fn shared_tools(&self) -> PathBuf {
            self.root.join("shared").join("tools")
        }

        /// Shared skills directory.
        #[must_use]
        fn shared_skills(&self) -> PathBuf {
            self.root.join("shared").join("skills")
        }

        /// Shared hooks directory.
        #[must_use]
        fn shared_hooks(&self) -> PathBuf {
            self.root.join("shared").join("hooks")
        }

        /// Shared coordination directory.
        #[must_use]
        fn coordination(&self) -> PathBuf {
            self.root.join("shared").join("coordination")
        }

        /// The nous directory containing all agent workspaces.
        #[must_use]
        fn nous_root(&self) -> PathBuf {
            self.root.join("nous")
        }

        /// The main config file (prefers TOML, falls back to JSON).
        #[must_use]
        fn config_file(&self) -> PathBuf {
            let toml = self.root.join("config").join("aletheia.toml");
            if toml.exists() {
                return toml;
            }
            self.root.join("config").join("aletheia.json")
        }

        /// The session encryption key file.
        #[must_use]
        fn session_key(&self) -> PathBuf {
            self.root.join("config").join("session.key")
        }

        /// The planning database file.
        #[must_use]
        fn planning_db(&self) -> PathBuf {
            self.root.join("data").join("planning.db")
        }

        /// The Signal data directory.
        #[must_use]
        fn signal(&self) -> PathBuf {
            self.root.join("signal")
        }
    }

    fn write_test_file(path: &Path, contents: &[u8]) {
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(contents).unwrap();
    }

    #[test]
    fn oikos_path_structure() {
        let oikos = Oikos::from_root("/srv/aletheia/instance");

        assert_eq!(
            oikos.root(),
            Path::new("/srv/aletheia/instance"),
            "root path should match"
        );
        assert_eq!(
            oikos.theke(),
            PathBuf::from("/srv/aletheia/instance/theke"),
            "theke path should be under root"
        );
        assert_eq!(
            oikos.shared(),
            PathBuf::from("/srv/aletheia/instance/shared"),
            "shared path should be under root"
        );
        assert_eq!(
            oikos.nous_dir("syn"),
            PathBuf::from("/srv/aletheia/instance/nous/syn"),
            "nous dir should include agent id"
        );
        assert_eq!(
            oikos.nous_file("syn", "SOUL.md"),
            PathBuf::from("/srv/aletheia/instance/nous/syn/SOUL.md"),
            "nous file should include agent id and filename"
        );
        assert_eq!(
            oikos.config(),
            PathBuf::from("/srv/aletheia/instance/config"),
            "config path should be under root"
        );
        assert_eq!(
            oikos.sessions_db(),
            PathBuf::from("/srv/aletheia/instance/data/sessions.db"),
            "sessions db should be under data/"
        );
    }

    #[test]
    fn oikos_env_override() {
        let oikos = Oikos::from_root("/custom/path");
        assert_eq!(
            oikos.root(),
            Path::new("/custom/path"),
            "custom root should be preserved"
        );
    }

    #[test]
    fn shared_subdirs() {
        let oikos = Oikos::from_root("/test");
        assert_eq!(
            oikos.shared_tools(),
            PathBuf::from("/test/shared/tools"),
            "shared tools path"
        );
        assert_eq!(
            oikos.shared_skills(),
            PathBuf::from("/test/shared/skills"),
            "shared skills path"
        );
        assert_eq!(
            oikos.shared_hooks(),
            PathBuf::from("/test/shared/hooks"),
            "shared hooks path"
        );
        assert_eq!(
            oikos.coordination(),
            PathBuf::from("/test/shared/coordination"),
            "coordination path"
        );
    }

    #[test]
    fn data_paths() {
        let oikos = Oikos::from_root("/srv/instance");
        assert_eq!(
            oikos.data(),
            PathBuf::from("/srv/instance/data"),
            "data path"
        );
        assert_eq!(
            oikos.sessions_db(),
            PathBuf::from("/srv/instance/data/sessions.db"),
            "sessions db path"
        );
        assert_eq!(
            oikos.planning_db(),
            PathBuf::from("/srv/instance/data/planning.db"),
            "planning db path"
        );
        assert_eq!(
            oikos.knowledge_db(),
            PathBuf::from("/srv/instance/data/knowledge.fjall"),
            "knowledge db path"
        );
        assert_eq!(
            oikos.logs(),
            PathBuf::from("/srv/instance/logs"),
            "logs path"
        );
        assert_eq!(
            oikos.signal(),
            PathBuf::from("/srv/instance/signal"),
            "signal path"
        );
    }

    #[test]
    fn config_paths() {
        let oikos = Oikos::from_root("/srv/instance");
        assert_eq!(
            oikos.config(),
            PathBuf::from("/srv/instance/config"),
            "config path"
        );
        assert_eq!(
            oikos.credentials(),
            PathBuf::from("/srv/instance/config/credentials"),
            "credentials path"
        );
        assert_eq!(
            oikos.session_key(),
            PathBuf::from("/srv/instance/config/session.key"),
            "session key path"
        );
    }

    #[test]
    fn nous_root_and_files() {
        let oikos = Oikos::from_root("/srv/instance");
        assert_eq!(
            oikos.nous_root(),
            PathBuf::from("/srv/instance/nous"),
            "nous root path"
        );
        assert_eq!(
            oikos.nous_file("demiurge", "SOUL.md"),
            PathBuf::from("/srv/instance/nous/demiurge/SOUL.md"),
            "nous file path for demiurge"
        );
    }

    #[test]
    fn contained_nous_file_accepts_file_inside_root() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file = dir.path().join("nous/alice/SOUL.md");
        std::fs::create_dir_all(file.parent().expect("file parent")).unwrap();
        write_test_file(&file, b"# Alice\n");

        let oikos = Oikos::from_root(dir.path());
        let nous_id = NousId::new("alice").expect("valid nous id");
        let resolved = oikos
            .contained_nous_file(&nous_id, "SOUL.md")
            .expect("contained file resolves");

        assert_eq!(resolved, std::fs::canonicalize(file).unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn contained_nous_file_rejects_symlink_escape() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let outside = tempfile::tempdir().expect("create outside temp dir");
        let outside_file = outside.path().join("SOUL.md");
        write_test_file(&outside_file, b"# Escape\n");
        let link = dir.path().join("nous/alice/SOUL.md");
        std::fs::create_dir_all(link.parent().expect("link parent")).unwrap();
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();

        let oikos = Oikos::from_root(dir.path());
        let nous_id = NousId::new("alice").expect("valid nous id");
        let err = oikos.contained_nous_file(&nous_id, "SOUL.md").unwrap_err();

        assert!(
            matches!(err, crate::error::Error::PathOutsideRoot { .. }),
            "expected PathOutsideRoot, got {err:?}"
        );
    }

    #[test]
    fn config_file_defaults_to_json() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let oikos = Oikos::from_root(dir.path());
        let cf = oikos.config_file();
        assert!(
            cf.to_string_lossy().ends_with("aletheia.json"),
            "should default to json when no toml exists"
        );
    }

    #[test]
    fn config_file_prefers_toml() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "taxis config operations are CLI-invoked and require synchronous filesystem access"
        )]
        std::fs::write(dir.path().join("config/aletheia.toml"), "").unwrap();

        let oikos = Oikos::from_root(dir.path());
        let cf = oikos.config_file();
        assert!(
            cf.to_string_lossy().ends_with("aletheia.toml"),
            "should prefer toml when it exists"
        );
    }

    fn make_valid_instance() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::create_dir_all(dir.path().join("nous")).unwrap();
        dir
    }

    #[test]
    fn validate_passes_with_valid_layout() {
        let dir = make_valid_instance();
        let oikos = Oikos::from_root(dir.path());
        assert!(
            oikos.validate().is_ok(),
            "valid instance layout should pass validation"
        );
    }

    #[test]
    fn validate_fails_when_root_missing() {
        let oikos = Oikos::from_root("/tmp/aletheia-nonexistent-root-xyz-12345");
        let err = oikos.validate().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("instance root not found"),
            "expected 'instance root not found' in: {msg}"
        );
        assert!(
            msg.contains("aletheia init"),
            "expected 'aletheia init' hint in: {msg}"
        );
    }

    #[test]
    fn validate_fails_when_config_dir_missing() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        let oikos = Oikos::from_root(dir.path());
        let err = oikos.validate().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("required directory missing"),
            "expected 'required directory missing' in: {msg}"
        );
        assert!(
            msg.contains("config"),
            "expected path to mention 'config': {msg}"
        );
        assert!(
            msg.contains("aletheia init"),
            "expected 'aletheia init' hint in: {msg}"
        );
    }

    #[test]
    fn validate_fails_when_data_dir_missing() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        let oikos = Oikos::from_root(dir.path());
        let err = oikos.validate().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("required directory missing"),
            "expected 'required directory missing' in: {msg}"
        );
        assert!(
            msg.contains("data"),
            "expected path to mention 'data': {msg}"
        );
    }

    #[test]
    fn validate_warns_but_passes_without_nous_dir() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        let oikos = Oikos::from_root(dir.path());
        // NOTE: missing nous/ is a warning, not an error: the test verifies this
        assert!(
            oikos.validate().is_ok(),
            "missing nous/ should warn but pass"
        );
    }

    #[test]
    #[cfg(unix)]
    fn validate_fails_when_data_not_writable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).unwrap();

        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o555)).unwrap();

        // NOTE: skip if running as root: root bypasses file permission checks
        let probe = data.join(".root-probe");
        #[expect(
            clippy::disallowed_methods,
            reason = "taxis config operations are CLI-invoked and require synchronous filesystem access"
        )]
        let is_root = std::fs::write(&probe, b"x").is_ok();
        let _ = std::fs::remove_file(&probe);
        if is_root {
            std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        let oikos = Oikos::from_root(dir.path());
        let result = oikos.validate();

        // WHY: restore permissions so tempdir cleanup works
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();

        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not writable"),
            "expected 'not writable' in: {msg}"
        );
        assert!(
            msg.contains("aletheia init"),
            "expected 'aletheia init' hint in: {msg}"
        );
    }

    #[test]
    fn validate_workspace_path_accepts_existing_relative() {
        let dir = make_valid_instance();
        // NOTE: nous/ already created by make_valid_instance
        let oikos = Oikos::from_root(dir.path());
        assert!(
            oikos.validate_workspace_path("nous").is_ok(),
            "relative nous/ path should be accepted"
        );
    }

    #[test]
    fn validate_workspace_path_accepts_existing_absolute() {
        let dir = make_valid_instance();
        let oikos = Oikos::from_root(dir.path());
        let abs = dir.path().join("nous").to_string_lossy().into_owned();
        assert!(
            oikos.validate_workspace_path(&abs).is_ok(),
            "absolute nous/ path should be accepted"
        );
    }

    #[test]
    fn validate_workspace_path_rejects_missing_path() {
        let dir = make_valid_instance();
        let oikos = Oikos::from_root(dir.path());
        let err = oikos
            .validate_workspace_path("nous/nonexistent-agent-xyz")
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("agent workspace path does not exist"),
            "expected workspace error in: {msg}"
        );
        assert!(
            msg.contains("aletheia init") || msg.contains("update the workspace path"),
            "expected help hint in: {msg}"
        );
    }

    #[test]
    fn init_layout_passes_validation() {
        let dir = tempfile::tempdir().expect("create temp dir");
        for sub in &["config", "data", "nous", "logs", "shared"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        let oikos = Oikos::from_root(dir.path());
        assert!(
            oikos.validate().is_ok(),
            "a freshly-initialised instance layout must pass validation"
        );
    }

    // ── discover_with (Environment trait) ────────────────────────────────

    #[test]
    fn discover_with_uses_env_var_when_set() {
        use koina::system::TestSystem;

        let env = TestSystem::new().with_env("ALETHEIA_ROOT", "/custom/root");
        let oikos = Oikos::discover_with(&env);
        assert_eq!(
            oikos.root(),
            Path::new("/custom/root"),
            "ALETHEIA_ROOT env var should override default"
        );
    }

    #[test]
    fn discover_with_falls_back_to_instance_when_unset() {
        use koina::system::TestSystem;

        let env = TestSystem::new(); // no ALETHEIA_ROOT set
        let oikos = Oikos::discover_with(&env);
        assert_eq!(
            oikos.root(),
            Path::new("instance"),
            "should fall back to 'instance' when env var unset"
        );
    }
}
