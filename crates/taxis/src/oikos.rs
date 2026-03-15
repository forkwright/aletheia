//! Oikos path resolution.
//!
//! The oikos is the instance directory structure. All paths in Aletheia resolve
//! relative to the instance root. Environment variable `ALETHEIA_ROOT` overrides
//! the default.

use std::path::{Path, PathBuf};

use snafu::{ResultExt, ensure};

/// The oikos — resolved instance paths.
///
/// All paths are absolute. Construct via [`Oikos::discover`] or [`Oikos::from_root`].
#[derive(Debug, Clone)]
pub struct Oikos {
    root: PathBuf,
}

impl Oikos {
    /// Create an oikos from an explicit root path.
    #[must_use]
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Discover the oikos root.
    ///
    /// Resolution order:
    /// 1. `ALETHEIA_ROOT` environment variable
    /// 2. `./instance` relative to current directory
    #[must_use]
    pub fn discover() -> Self {
        let root = std::env::var("ALETHEIA_ROOT")
            .map_or_else(|_| PathBuf::from("instance"), PathBuf::from);
        Self { root }
    }

    /// The instance root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
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

    /// Shared tools directory.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn shared_tools(&self) -> PathBuf {
        self.root.join("shared").join("tools")
    }

    /// Shared skills directory.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn shared_skills(&self) -> PathBuf {
        self.root.join("shared").join("skills")
    }

    /// Shared hooks directory.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn shared_hooks(&self) -> PathBuf {
        self.root.join("shared").join("hooks")
    }

    /// Shared coordination directory.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn coordination(&self) -> PathBuf {
        self.root.join("shared").join("coordination")
    }

    /// The nous directory containing all agent workspaces.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn nous_root(&self) -> PathBuf {
        self.root.join("nous")
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

    /// The config directory.
    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    /// The main config file (prefers TOML, falls back to JSON).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn config_file(&self) -> PathBuf {
        let toml = self.root.join("config").join("aletheia.toml");
        if toml.exists() {
            return toml;
        }
        self.root.join("config").join("aletheia.json")
    }

    /// The credentials directory.
    #[must_use]
    pub fn credentials(&self) -> PathBuf {
        self.root.join("config").join("credentials")
    }

    /// The session encryption key file.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn session_key(&self) -> PathBuf {
        self.root.join("config").join("session.key")
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

    /// The planning database file.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn planning_db(&self) -> PathBuf {
        self.root.join("data").join("planning.db")
    }

    /// The knowledge store directory (fjall persistent storage).
    #[must_use]
    pub fn knowledge_db(&self) -> PathBuf {
        self.root.join("data").join("knowledge.fjall")
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

    /// The Signal data directory.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "instance layout accessor; no non-test caller yet")
    )]
    pub(crate) fn signal(&self) -> PathBuf {
        self.root.join("signal")
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
    #[expect(
        clippy::result_large_err,
        reason = "shared Error enum contains figment::Error; boxing would require a crate-wide change"
    )]
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
    /// Returns [`crate::error::Error::WorkspacePathInvalid`] if the path does not
    /// exist or is not a directory.
    #[expect(
        clippy::result_large_err,
        reason = "shared Error enum contains figment::Error; boxing would require a crate-wide change"
    )]
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

    #[expect(
        clippy::result_large_err,
        reason = "shared Error enum contains figment::Error; boxing would require a crate-wide change"
    )]
    fn check_writable(path: &Path) -> crate::error::Result<()> {
        use crate::error::NotWritableSnafu;

        let test_file = path.join(".aletheia-write-test");
        std::fs::write(&test_file, b"ok").context(NotWritableSnafu {
            path: path.to_path_buf(),
        })?;
        let _ = std::fs::remove_file(&test_file);
        Ok(())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn oikos_path_structure() {
        let oikos = Oikos::from_root("/srv/aletheia/instance");

        assert_eq!(oikos.root(), Path::new("/srv/aletheia/instance"));
        assert_eq!(oikos.theke(), PathBuf::from("/srv/aletheia/instance/theke"));
        assert_eq!(
            oikos.shared(),
            PathBuf::from("/srv/aletheia/instance/shared")
        );
        assert_eq!(
            oikos.nous_dir("syn"),
            PathBuf::from("/srv/aletheia/instance/nous/syn")
        );
        assert_eq!(
            oikos.nous_file("syn", "SOUL.md"),
            PathBuf::from("/srv/aletheia/instance/nous/syn/SOUL.md")
        );
        assert_eq!(
            oikos.config(),
            PathBuf::from("/srv/aletheia/instance/config")
        );
        assert_eq!(
            oikos.sessions_db(),
            PathBuf::from("/srv/aletheia/instance/data/sessions.db")
        );
    }

    #[test]
    fn oikos_env_override() {
        let oikos = Oikos::from_root("/custom/path");
        assert_eq!(oikos.root(), Path::new("/custom/path"));
    }

    #[test]
    fn shared_subdirs() {
        let oikos = Oikos::from_root("/test");
        assert_eq!(oikos.shared_tools(), PathBuf::from("/test/shared/tools"));
        assert_eq!(oikos.shared_skills(), PathBuf::from("/test/shared/skills"));
        assert_eq!(oikos.shared_hooks(), PathBuf::from("/test/shared/hooks"));
        assert_eq!(
            oikos.coordination(),
            PathBuf::from("/test/shared/coordination")
        );
    }

    #[test]
    fn data_paths() {
        let oikos = Oikos::from_root("/srv/instance");
        assert_eq!(oikos.data(), PathBuf::from("/srv/instance/data"));
        assert_eq!(
            oikos.sessions_db(),
            PathBuf::from("/srv/instance/data/sessions.db")
        );
        assert_eq!(
            oikos.planning_db(),
            PathBuf::from("/srv/instance/data/planning.db")
        );
        assert_eq!(
            oikos.knowledge_db(),
            PathBuf::from("/srv/instance/data/knowledge.fjall")
        );
        assert_eq!(oikos.logs(), PathBuf::from("/srv/instance/logs"));
        assert_eq!(oikos.signal(), PathBuf::from("/srv/instance/signal"));
    }

    #[test]
    fn config_paths() {
        let oikos = Oikos::from_root("/srv/instance");
        assert_eq!(oikos.config(), PathBuf::from("/srv/instance/config"));
        assert_eq!(
            oikos.credentials(),
            PathBuf::from("/srv/instance/config/credentials")
        );
        assert_eq!(
            oikos.session_key(),
            PathBuf::from("/srv/instance/config/session.key")
        );
    }

    #[test]
    fn nous_root_and_files() {
        let oikos = Oikos::from_root("/srv/instance");
        assert_eq!(oikos.nous_root(), PathBuf::from("/srv/instance/nous"));
        assert_eq!(
            oikos.nous_file("demiurge", "SOUL.md"),
            PathBuf::from("/srv/instance/nous/demiurge/SOUL.md")
        );
    }

    #[test]
    fn config_file_defaults_to_json() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let oikos = Oikos::from_root(dir.path());
        let cf = oikos.config_file();
        assert!(cf.to_string_lossy().ends_with("aletheia.json"));
    }

    #[test]
    fn config_file_prefers_toml() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config/aletheia.toml"), "").unwrap();

        let oikos = Oikos::from_root(dir.path());
        let cf = oikos.config_file();
        assert!(cf.to_string_lossy().ends_with("aletheia.toml"));
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
        assert!(oikos.validate().is_ok());
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
        // NOTE: missing nous/ is a warning, not an error — the test verifies this
        assert!(oikos.validate().is_ok());
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

        // NOTE: skip if running as root — root bypasses file permission checks
        let probe = data.join(".root-probe");
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
        assert!(oikos.validate_workspace_path("nous").is_ok());
    }

    #[test]
    fn validate_workspace_path_accepts_existing_absolute() {
        let dir = make_valid_instance();
        let oikos = Oikos::from_root(dir.path());
        let abs = dir.path().join("nous").to_string_lossy().into_owned();
        assert!(oikos.validate_workspace_path(&abs).is_ok());
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
}
