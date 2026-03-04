//! Trace file rotation and compression.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

use snafu::ResultExt;

use crate::error;

/// Configuration for trace file rotation.
#[derive(Debug, Clone)]
pub struct TraceRotationConfig {
    pub enabled: bool,
    pub trace_dir: PathBuf,
    pub archive_dir: PathBuf,
    pub max_age_days: u32,
    pub max_total_size_mb: u64,
    pub compress: bool,
    pub max_archives: usize,
}

impl Default for TraceRotationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trace_dir: PathBuf::from("logs/traces"),
            archive_dir: PathBuf::from("logs/traces/archive"),
            max_age_days: 14,
            max_total_size_mb: 500,
            compress: true,
            max_archives: 30,
        }
    }
}

/// Outcome of a trace rotation run.
#[derive(Debug, Clone, Default)]
pub struct RotationReport {
    pub files_rotated: u32,
    pub files_pruned: u32,
    pub bytes_freed: u64,
}

/// Rotates old trace files to an archive directory with optional gzip compression.
pub struct TraceRotator {
    config: TraceRotationConfig,
}

impl TraceRotator {
    pub fn new(config: TraceRotationConfig) -> Self {
        Self { config }
    }

    /// Run trace rotation. Moves old files to archive, compresses if configured,
    /// prunes archives exceeding the limit.
    pub fn rotate(&self) -> error::Result<RotationReport> {
        if !self.config.trace_dir.exists() {
            return Ok(RotationReport::default());
        }

        fs::create_dir_all(&self.config.archive_dir).context(error::MaintenanceIoSnafu {
            context: format!("creating archive dir {}", self.config.archive_dir.display()),
        })?;

        let mut report = RotationReport::default();
        let now = SystemTime::now();
        let max_age = std::time::Duration::from_secs(u64::from(self.config.max_age_days) * 86400);

        let mut entries = self.list_trace_files()?;

        // Sort by modification time ascending (oldest first).
        entries.sort_by_key(|e| e.modified);

        // Calculate total size.
        let total_size_bytes: u64 = entries.iter().map(|e| e.size).sum();
        let max_bytes = self.config.max_total_size_mb * 1024 * 1024;

        // Determine which files to rotate: old files, or oldest when over size limit.
        let mut to_rotate = Vec::new();
        let mut cumulative_freed: u64 = 0;

        for entry in &entries {
            let age = now.duration_since(entry.modified).unwrap_or_default();
            let over_size = total_size_bytes.saturating_sub(cumulative_freed) > max_bytes;

            if age > max_age || over_size {
                to_rotate.push(entry.clone());
                cumulative_freed += entry.size;
            }
        }

        // Rotate eligible files.
        for entry in &to_rotate {
            let dest = self
                .config
                .archive_dir
                .join(entry.path.file_name().expect("trace file has a file name"));

            fs::rename(&entry.path, &dest).context(error::MaintenanceIoSnafu {
                context: format!("moving {} to archive", entry.path.display()),
            })?;

            if self.config.compress {
                self.compress_file(&dest)?;
            }

            report.files_rotated += 1;
            report.bytes_freed += entry.size;
        }

        // Prune old archives beyond max_archives.
        report.files_pruned = self.prune_archives()?;

        Ok(report)
    }

    fn list_trace_files(&self) -> error::Result<Vec<TraceFileEntry>> {
        let mut entries = Vec::new();
        let dir = fs::read_dir(&self.config.trace_dir).context(error::MaintenanceIoSnafu {
            context: format!("reading trace dir {}", self.config.trace_dir.display()),
        })?;

        for entry in dir {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: "reading directory entry",
            })?;
            let path = entry.path();

            // Skip directories (including archive/).
            if path.is_dir() {
                continue;
            }

            let metadata = entry.metadata().context(error::MaintenanceIoSnafu {
                context: format!("reading metadata for {}", path.display()),
            })?;

            let modified = metadata.modified().context(error::MaintenanceIoSnafu {
                context: format!("reading mtime for {}", path.display()),
            })?;

            entries.push(TraceFileEntry {
                path,
                size: metadata.len(),
                modified,
            });
        }

        Ok(entries)
    }

    #[expect(
        clippy::unused_self,
        reason = "method for consistency, may use config later"
    )]
    fn compress_file(&self, path: &std::path::Path) -> error::Result<()> {
        let gz_path = path.with_extension(format!(
            "{}.gz",
            path.extension().and_then(|e| e.to_str()).unwrap_or("dat")
        ));

        let input = fs::read(path).context(error::MaintenanceIoSnafu {
            context: format!("reading file for compression: {}", path.display()),
        })?;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&input)
            .context(error::MaintenanceIoSnafu {
                context: format!("compressing {}", path.display()),
            })?;
        let compressed = encoder.finish().context(error::MaintenanceIoSnafu {
            context: format!("finishing compression of {}", path.display()),
        })?;

        fs::write(&gz_path, compressed).context(error::MaintenanceIoSnafu {
            context: format!("writing compressed file {}", gz_path.display()),
        })?;

        fs::remove_file(path).context(error::MaintenanceIoSnafu {
            context: format!("removing original after compression: {}", path.display()),
        })?;

        Ok(())
    }

    fn prune_archives(&self) -> error::Result<u32> {
        let dir = match fs::read_dir(&self.config.archive_dir) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => {
                return Err(e).context(error::MaintenanceIoSnafu {
                    context: "reading archive dir for pruning",
                });
            }
        };

        let mut archives: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
        for entry in dir {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: "reading archive entry",
            })?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let metadata = entry.metadata().context(error::MaintenanceIoSnafu {
                context: format!("reading archive metadata: {}", path.display()),
            })?;
            let modified = metadata.modified().context(error::MaintenanceIoSnafu {
                context: format!("reading archive mtime: {}", path.display()),
            })?;
            archives.push((path, modified, metadata.len()));
        }

        // Sort oldest first.
        archives.sort_by_key(|(_, modified, _)| *modified);

        let mut pruned = 0u32;
        while archives.len() > self.config.max_archives {
            let (path, _, _) = archives.remove(0);
            fs::remove_file(&path).context(error::MaintenanceIoSnafu {
                context: format!("pruning archive {}", path.display()),
            })?;
            pruned += 1;
        }

        Ok(pruned)
    }
}

#[derive(Debug, Clone)]
struct TraceFileEntry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn make_config(dir: &std::path::Path) -> TraceRotationConfig {
        TraceRotationConfig {
            enabled: true,
            trace_dir: dir.join("traces"),
            archive_dir: dir.join("traces/archive"),
            max_age_days: 7,
            max_total_size_mb: 10,
            compress: false,
            max_archives: 3,
        }
    }

    #[test]
    fn old_files_are_rotated_via_size_limit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = make_config(tmp.path());
        config.max_age_days = 0; // treat all files as old
        config.max_total_size_mb = 9999; // don't trigger size
        fs::create_dir_all(&config.trace_dir).unwrap();

        let file = config.trace_dir.join("old-trace.log");
        fs::write(&file, "trace data").unwrap();

        let rotator = TraceRotator::new(config.clone());
        let report = rotator.rotate().expect("rotation succeeds");

        assert_eq!(report.files_rotated, 1);
        assert!(!file.exists(), "old file should be moved");
        assert!(
            config.archive_dir.join("old-trace.log").exists(),
            "old file should be in archive"
        );
    }

    #[test]
    fn size_limit_triggers_rotation() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = make_config(tmp.path());
        config.max_total_size_mb = 0; // force everything to rotate
        config.max_age_days = 9999; // don't rotate by age
        fs::create_dir_all(&config.trace_dir).unwrap();

        fs::write(config.trace_dir.join("a.log"), "data").unwrap();
        fs::write(config.trace_dir.join("b.log"), "data").unwrap();

        let rotator = TraceRotator::new(config.clone());
        let report = rotator.rotate().expect("rotation succeeds");

        assert!(
            report.files_rotated >= 1,
            "should rotate when over size limit"
        );
    }

    #[test]
    fn archives_beyond_max_are_pruned() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.archive_dir).unwrap();

        // Create 5 archives, max is 3.
        for i in 0..5 {
            fs::write(config.archive_dir.join(format!("archive-{i}.log")), "data").unwrap();
        }

        let rotator = TraceRotator::new(config.clone());
        let report = rotator.rotate().expect("rotation succeeds");

        assert_eq!(report.files_pruned, 2);
        let remaining: Vec<_> = fs::read_dir(&config.archive_dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .collect();
        assert_eq!(remaining.len(), 3);
    }

    #[test]
    fn compress_creates_gz() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = make_config(tmp.path());
        config.compress = true;
        config.max_total_size_mb = 0; // rotate everything
        fs::create_dir_all(&config.trace_dir).unwrap();

        fs::write(config.trace_dir.join("trace.log"), "compressible data").unwrap();

        let rotator = TraceRotator::new(config.clone());
        let report = rotator.rotate().expect("rotation succeeds");

        assert_eq!(report.files_rotated, 1);
        // Original should be gone, .gz should exist.
        assert!(!config.archive_dir.join("trace.log").exists());
        assert!(config.archive_dir.join("trace.log.gz").exists());

        // Verify decompression.
        let compressed = fs::read(config.archive_dir.join("trace.log.gz")).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert_eq!(decompressed, "compressible data");
    }

    #[test]
    fn nonexistent_trace_dir_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = TraceRotationConfig {
            trace_dir: tmp.path().join("nonexistent"),
            ..make_config(tmp.path())
        };

        let rotator = TraceRotator::new(config);
        let report = rotator.rotate().expect("should not error");
        assert_eq!(report.files_rotated, 0);
        assert_eq!(report.files_pruned, 0);
    }
}
