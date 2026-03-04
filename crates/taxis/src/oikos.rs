//! Oikos path resolution.
//!
//! The oikos is the instance directory structure. All paths in Aletheia resolve
//! relative to the instance root. Environment variable `ALETHEIA_ROOT` overrides
//! the default.

use std::path::{Path, PathBuf};

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

    // --- Root ---

    /// The instance root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    // --- Tier 0: Theke (human + nous collaborative) ---

    /// The theke directory (human + nous collaborative space).
    #[must_use]
    pub fn theke(&self) -> PathBuf {
        self.root.join("theke")
    }

    // --- Tier 1: Shared (nous-only) ---

    /// The shared directory (nous-only shared resources).
    #[must_use]
    pub fn shared(&self) -> PathBuf {
        self.root.join("shared")
    }

    /// Shared tools directory.
    #[must_use]
    pub fn shared_tools(&self) -> PathBuf {
        self.root.join("shared").join("tools")
    }

    /// Shared skills directory.
    #[must_use]
    pub fn shared_skills(&self) -> PathBuf {
        self.root.join("shared").join("skills")
    }

    /// Shared hooks directory.
    #[must_use]
    pub fn shared_hooks(&self) -> PathBuf {
        self.root.join("shared").join("hooks")
    }

    /// Shared coordination directory.
    #[must_use]
    pub fn coordination(&self) -> PathBuf {
        self.root.join("shared").join("coordination")
    }

    // --- Tier 2: Nous (per-agent) ---

    /// The nous directory containing all agent workspaces.
    #[must_use]
    pub fn nous_root(&self) -> PathBuf {
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

    // --- Config ---

    /// The config directory.
    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    /// The main config file (prefers YAML, falls back to JSON).
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        let yaml = self.root.join("config").join("aletheia.yaml");
        if yaml.exists() {
            return yaml;
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
    pub fn session_key(&self) -> PathBuf {
        self.root.join("config").join("session.key")
    }

    // --- Data ---

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
    pub fn planning_db(&self) -> PathBuf {
        self.root.join("data").join("planning.db")
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

    // --- Logs ---

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

    // --- Signal ---

    /// The Signal data directory.
    #[must_use]
    pub fn signal(&self) -> PathBuf {
        self.root.join("signal")
    }
}

#[cfg(test)]
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
        // Test that from_root works regardless of env
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
    fn config_file_prefers_yaml() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config/aletheia.yaml"), "{}").unwrap();

        let oikos = Oikos::from_root(dir.path());
        let cf = oikos.config_file();
        assert!(cf.to_string_lossy().ends_with("aletheia.yaml"));
    }
}
