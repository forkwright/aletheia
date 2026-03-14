//! Instance drift detection — compare instance against template.

use std::fs;
use std::path::{Path, PathBuf};

use snafu::ResultExt;

use crate::error;

/// Configuration for instance drift detection.
#[derive(Debug, Clone)]
pub struct DriftDetectionConfig {
    /// Whether drift detection is active.
    pub enabled: bool,
    /// Path to the live instance directory.
    pub instance_root: PathBuf,
    /// Path to the example/template directory to compare against.
    pub example_root: PathBuf,
    /// Whether to raise alerts for files present in the template but missing from the instance.
    pub alert_on_missing: bool,
    /// Glob-like patterns to exclude from comparison (e.g., `"data/"`, `"*.db"`).
    pub ignore_patterns: Vec<String>,
}

impl Default for DriftDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            instance_root: PathBuf::from("instance"),
            example_root: PathBuf::from("instance.example"),
            alert_on_missing: true,
            ignore_patterns: vec![
                "data/".to_owned(),
                "signal/".to_owned(),
                "*.db".to_owned(),
                ".gitkeep".to_owned(),
            ],
        }
    }
}

/// Outcome of a drift detection check.
#[derive(Debug, Clone, Default)]
pub struct DriftReport {
    /// Files present in the template but absent from the instance.
    pub missing_files: Vec<PathBuf>,
    /// Files present in the instance but absent from the template.
    pub extra_files: Vec<PathBuf>,
    /// Files with permission discrepancies (path, description).
    pub permission_issues: Vec<(PathBuf, String)>,
    /// When the check was performed.
    pub checked_at: Option<jiff::Timestamp>,
}

/// Compares an instance directory against the example template.
pub struct DriftDetector {
    config: DriftDetectionConfig,
}

impl DriftDetector {
    /// Create a detector with the given instance and template paths.
    pub fn new(config: DriftDetectionConfig) -> Self {
        Self { config }
    }

    /// Run drift detection. Returns a report of discrepancies.
    pub fn check(&self) -> error::Result<DriftReport> {
        if !self.config.example_root.exists() {
            return Ok(DriftReport {
                checked_at: Some(jiff::Timestamp::now()),
                ..Default::default()
            });
        }

        let mut report = DriftReport {
            checked_at: Some(jiff::Timestamp::now()),
            ..Default::default()
        };

        self.walk_example(&self.config.example_root, &mut report)?;

        Ok(report)
    }

    #[expect(
        clippy::expect_used,
        reason = "path is obtained by walking example_root so strip_prefix is guaranteed to succeed"
    )]
    fn walk_example(&self, dir: &Path, report: &mut DriftReport) -> error::Result<()> {
        let entries = fs::read_dir(dir).context(error::MaintenanceIoSnafu {
            context: format!("reading example dir {}", dir.display()),
        })?;

        for entry in entries {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: "reading example entry",
            })?;
            let path = entry.path();
            let relative = path
                .strip_prefix(&self.config.example_root)
                .expect("path is under example root");

            if self.is_ignored(relative) {
                continue;
            }

            let instance_path = self.config.instance_root.join(relative);

            if path.is_dir() {
                if !instance_path.exists() {
                    report.missing_files.push(relative.to_path_buf());
                }
                self.walk_example(&path, report)?;
            } else if !instance_path.exists() {
                report.missing_files.push(relative.to_path_buf());
            }
        }

        Ok(())
    }

    fn is_ignored(&self, relative: &Path) -> bool {
        let path_str = relative.to_string_lossy();

        for pattern in &self.config.ignore_patterns {
            if pattern.ends_with('/') {
                let prefix = &pattern[..pattern.len() - 1];
                if path_str.starts_with(prefix) || path_str == *prefix {
                    return true;
                }
            } else if pattern.starts_with("*.") {
                let ext = &pattern[1..];
                if path_str.ends_with(ext) {
                    return true;
                }
            } else if let Some(name) = relative.file_name().and_then(|n| n.to_str())
                && name == pattern
            {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_config(tmp: &Path) -> DriftDetectionConfig {
        DriftDetectionConfig {
            enabled: true,
            instance_root: tmp.join("instance"),
            example_root: tmp.join("example"),
            alert_on_missing: true,
            ignore_patterns: vec![
                "data/".to_owned(),
                "signal/".to_owned(),
                "*.db".to_owned(),
                ".gitkeep".to_owned(),
            ],
        }
    }

    #[test]
    fn detects_missing_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());

        fs::create_dir_all(config.example_root.join("config")).unwrap();
        fs::write(config.example_root.join("config/aletheia.toml"), "").unwrap();

        fs::create_dir_all(config.instance_root.join("config")).unwrap();

        let detector = DriftDetector::new(config);
        let report = detector.check().expect("check succeeds");

        assert!(
            report
                .missing_files
                .contains(&PathBuf::from("config/aletheia.toml")),
            "should detect missing config file"
        );
    }

    #[test]
    fn detects_missing_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());

        fs::create_dir_all(config.example_root.join("nous")).unwrap();
        fs::write(config.example_root.join("nous/SOUL.md"), "").unwrap();

        fs::create_dir_all(&config.instance_root).unwrap();

        let detector = DriftDetector::new(config);
        let report = detector.check().expect("check succeeds");

        assert!(report.missing_files.contains(&PathBuf::from("nous")));
        assert!(
            report
                .missing_files
                .contains(&PathBuf::from("nous/SOUL.md"))
        );
    }

    #[test]
    fn ignored_patterns_are_skipped() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());

        fs::create_dir_all(config.example_root.join("data")).unwrap();
        fs::write(config.example_root.join("data/sessions.db"), "").unwrap();
        fs::create_dir_all(config.example_root.join("signal")).unwrap();
        fs::write(config.example_root.join("config.db"), "").unwrap();
        fs::create_dir_all(config.example_root.join("logs")).unwrap();
        fs::write(config.example_root.join("logs/.gitkeep"), "").unwrap();

        fs::create_dir_all(&config.instance_root).unwrap();

        let detector = DriftDetector::new(config);
        let report = detector.check().expect("check succeeds");

        for path in &report.missing_files {
            let path_str = path.to_string_lossy();
            assert!(
                !path_str.starts_with("data"),
                "data/ should be ignored, got {path_str}"
            );
            assert!(
                !path_str.starts_with("signal"),
                "signal/ should be ignored, got {path_str}"
            );
            assert!(
                !path_str.ends_with(".db"),
                "*.db should be ignored, got {path_str}"
            );
            assert!(
                !path_str.ends_with(".gitkeep"),
                ".gitkeep should be ignored, got {path_str}"
            );
        }
    }

    #[test]
    fn missing_example_dir_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = DriftDetectionConfig {
            example_root: tmp.path().join("nonexistent"),
            ..make_config(tmp.path())
        };

        let detector = DriftDetector::new(config);
        let report = detector.check().expect("should not error");
        assert!(report.missing_files.is_empty());
        assert!(report.checked_at.is_some());
    }

    #[test]
    fn matching_instance_reports_no_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());

        fs::create_dir_all(config.example_root.join("config")).unwrap();
        fs::write(config.example_root.join("config/aletheia.toml"), "").unwrap();
        fs::create_dir_all(config.instance_root.join("config")).unwrap();
        fs::write(config.instance_root.join("config/aletheia.toml"), "").unwrap();

        let detector = DriftDetector::new(config);
        let report = detector.check().expect("check succeeds");
        assert!(report.missing_files.is_empty());
    }

    #[test]
    fn default_config_values() {
        let config = DriftDetectionConfig::default();
        assert!(config.enabled);
        assert!(config.alert_on_missing);
        assert_eq!(config.instance_root, PathBuf::from("instance"));
        assert_eq!(config.example_root, PathBuf::from("instance.example"));
        assert!(
            config.ignore_patterns.contains(&"data/".to_owned()),
            "default should ignore data/"
        );
        assert!(
            config.ignore_patterns.contains(&"*.db".to_owned()),
            "default should ignore *.db"
        );
    }

    #[test]
    fn empty_example_and_instance() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());

        fs::create_dir_all(&config.example_root).unwrap();
        fs::create_dir_all(&config.instance_root).unwrap();

        let detector = DriftDetector::new(config);
        let report = detector.check().expect("check succeeds");
        assert!(report.missing_files.is_empty());
        assert!(report.extra_files.is_empty());
    }

    #[test]
    fn nested_directory_missing_detected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());

        fs::create_dir_all(config.example_root.join("level1/level2/level3")).unwrap();
        fs::write(
            config.example_root.join("level1/level2/level3/deep.yaml"),
            "",
        )
        .unwrap();

        fs::create_dir_all(&config.instance_root).unwrap();

        let detector = DriftDetector::new(config);
        let report = detector.check().expect("check succeeds");

        assert!(
            report
                .missing_files
                .contains(&PathBuf::from("level1/level2/level3/deep.yaml")),
            "should detect deeply nested missing file"
        );
    }
}
