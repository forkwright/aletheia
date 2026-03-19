//! Prosoche (προσοχή): "directed attention." Periodic check-in that monitors
//! calendar, tasks, and system health for a nous.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Prosoche attention check runner.
#[derive(Debug, Clone)]
pub struct ProsocheCheck {
    nous_id: String,
    /// Instance data directory to check for disk usage.
    data_dir: Option<PathBuf>,
    /// Database file paths to check sizes.
    db_paths: Vec<PathBuf>,
}

/// Result of a prosoche check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProsocheResult {
    /// Items requiring the nous's attention.
    pub items: Vec<AttentionItem>,
    /// ISO 8601 timestamp when the check was performed.
    pub checked_at: String,
}

/// A single item requiring attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionItem {
    /// What kind of attention is needed.
    pub category: AttentionCategory,
    /// Human-readable description of the item.
    pub summary: String,
    /// How urgently this needs attention.
    pub urgency: Urgency,
}

/// Categories of attention items.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttentionCategory {
    /// Calendar event or deadline.
    Calendar,
    /// Pending task or overdue item.
    Task,
    /// System health issue (disk, memory, service status).
    SystemHealth,
    /// Application-defined attention category.
    Custom(String),
}

/// Urgency level for attention items.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    /// Informational, no action needed soon.
    Low,
    /// Should be addressed within the current session.
    Medium,
    /// Needs attention soon (within hours).
    High,
    /// Requires immediate action.
    Critical,
}

impl AttentionItem {
    /// Short label for this item's category (used in prompt formatting).
    #[must_use]
    pub fn category_label(&self) -> &str {
        match &self.category {
            AttentionCategory::Calendar => "calendar",
            AttentionCategory::Task => "task",
            AttentionCategory::SystemHealth => "health",
            AttentionCategory::Custom(s) => s,
        }
    }
}

impl ProsocheCheck {
    /// Create a prosoche check for the given nous.
    pub fn new(nous_id: impl Into<String>) -> Self {
        Self {
            nous_id: nous_id.into(),
            data_dir: None,
            db_paths: Vec::new(),
        }
    }

    /// Set the instance data directory for disk space checks.
    #[must_use]
    pub fn with_data_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(path.into());
        self
    }

    /// Add database file paths to check sizes.
    #[must_use]
    pub fn with_db_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.db_paths = paths;
        self
    }

    /// Run the attention check. Returns items needing attention.
    ///
    /// Performs real system health checks:
    /// - Disk space on the data directory
    /// - Database file sizes
    /// - Process memory (RSS) via /proc/self/status
    pub async fn run(&self) -> crate::error::Result<ProsocheResult> {
        let mut items = Vec::new();

        if let Some(ref data_dir) = self.data_dir {
            items.extend(check_disk_space(data_dir).await);
        }

        items.extend(check_db_sizes(&self.db_paths));

        items.extend(check_memory());

        tracing::info!(
            nous_id = %self.nous_id,
            attention_items = items.len(),
            "prosoche check completed"
        );

        Ok(ProsocheResult {
            items,
            checked_at: jiff::Timestamp::now().to_string(),
        })
    }
}

/// Check disk space usage on the filesystem containing `path`.
///
/// Uses `df` command output. WARN at 80% usage, CRITICAL at 95%.
async fn check_disk_space(path: &Path) -> Vec<AttentionItem> {
    let mut items = Vec::new();

    match disk_usage_percent(path).await {
        Ok(percent) => {
            if percent >= 95.0 {
                items.push(AttentionItem {
                    category: AttentionCategory::SystemHealth,
                    summary: format!(
                        "Disk space critical: {percent:.1}% used on {}",
                        path.display()
                    ),
                    urgency: Urgency::Critical,
                });
            } else if percent >= 80.0 {
                items.push(AttentionItem {
                    category: AttentionCategory::SystemHealth,
                    summary: format!(
                        "Disk space warning: {percent:.1}% used on {}",
                        path.display()
                    ),
                    urgency: Urgency::High,
                });
            }
        }
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to check disk space"
            );
        }
    }

    items
}

/// Get disk usage percentage for the filesystem containing `path` via `df`.
async fn disk_usage_percent(path: &Path) -> std::io::Result<f64> {
    let output = tokio::process::Command::new("df")
        .args(["--output=pcent", "--"])
        .arg(path)
        .output()
        .await?;

    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_df_percent(&stdout)
}

/// Parse the percentage from `df --output=pcent` output.
fn parse_df_percent(output: &str) -> std::io::Result<f64> {
    // NOTE: df --output=pcent output has a header line then lines like " 42%".
    for line in output.lines().skip(1) {
        let trimmed = line.trim().trim_end_matches('%');
        if let Ok(val) = trimmed.parse::<f64>() {
            return Ok(val);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "failed to parse df output",
    ))
}

/// Check database file sizes. WARN if any file > 1 GB.
fn check_db_sizes(paths: &[PathBuf]) -> Vec<AttentionItem> {
    const ONE_GB: u64 = 1024 * 1024 * 1024;
    let mut items = Vec::new();

    for path in paths {
        match std::fs::metadata(path) {
            Ok(meta) => {
                let size = meta.len();
                if size > ONE_GB {
                    #[expect(
                        clippy::cast_precision_loss,
                        clippy::as_conversions,
                        reason = "u64→f64: file sizes don't need exact precision for display"
                    )]
                    let size_gb = size as f64 / ONE_GB as f64;
                    items.push(AttentionItem {
                        category: AttentionCategory::SystemHealth,
                        summary: format!(
                            "Database file large: {} is {size_gb:.1} GB",
                            path.display(),
                        ),
                        urgency: Urgency::High,
                    });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // NOTE: File doesn't exist: not an error for health checks.
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to check database file size"
                );
            }
        }
    }

    items
}

/// Check process memory (RSS) via `/proc/self/status` on Linux.
///
/// Returns an attention item if RSS exceeds reasonable thresholds.
fn check_memory() -> Vec<AttentionItem> {
    match read_process_rss_kb() {
        Ok(resident_kb) => {
            let rss_mb = resident_kb / 1024;
            let mut items = Vec::new();

            if rss_mb >= 2048 {
                items.push(AttentionItem {
                    category: AttentionCategory::SystemHealth,
                    summary: format!("Process memory critical: {rss_mb} MB RSS"),
                    urgency: Urgency::Critical,
                });
            } else if rss_mb >= 1024 {
                items.push(AttentionItem {
                    category: AttentionCategory::SystemHealth,
                    summary: format!("Process memory high: {rss_mb} MB RSS"),
                    urgency: Urgency::High,
                });
            }

            items
        }
        Err(e) => {
            tracing::debug!(error = %e, "failed to read process RSS — skipping memory check");
            Vec::new()
        }
    }
}

/// Read the `VmRSS` value from `/proc/self/status`.
fn read_process_rss_kb() -> std::io::Result<u64> {
    let contents = std::fs::read_to_string("/proc/self/status")?;
    parse_vmrss(&contents)
}

/// Parse `VmRSS` value in kB from proc status content.
pub(crate) fn parse_vmrss(contents: &str) -> std::io::Result<u64> {
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let value_str = rest.trim().trim_end_matches(" kB").trim();
            return value_str
                .parse::<u64>()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e));
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "VmRSS not found in proc status",
    ))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prosoche_returns_items_for_default() {
        let check = ProsocheCheck::new("test-nous");
        let result = check.run().await.expect("should succeed");
        assert!(!result.checked_at.is_empty());
    }

    #[test]
    fn prosoche_check_new() {
        let check = ProsocheCheck::new("alice-nous");
        let debug = format!("{check:?}");
        assert!(
            debug.contains("alice-nous"),
            "ProsocheCheck should store the nous_id"
        );
    }

    #[test]
    fn attention_item_category_label_calendar() {
        let item = AttentionItem {
            category: AttentionCategory::Calendar,
            summary: "meeting".to_owned(),
            urgency: Urgency::Medium,
        };
        assert_eq!(item.category_label(), "calendar");
    }

    #[test]
    fn attention_item_category_label_task() {
        let item = AttentionItem {
            category: AttentionCategory::Task,
            summary: "review PR".to_owned(),
            urgency: Urgency::Low,
        };
        assert_eq!(item.category_label(), "task");
    }

    #[test]
    fn attention_item_category_label_health() {
        let item = AttentionItem {
            category: AttentionCategory::SystemHealth,
            summary: "disk full".to_owned(),
            urgency: Urgency::Critical,
        };
        assert_eq!(item.category_label(), "health");
    }

    #[test]
    fn attention_item_category_label_custom() {
        let item = AttentionItem {
            category: AttentionCategory::Custom("foo".to_owned()),
            summary: "custom item".to_owned(),
            urgency: Urgency::Low,
        };
        assert_eq!(item.category_label(), "foo");
    }

    #[test]
    fn urgency_ordering() {
        assert!(Urgency::Low < Urgency::Medium);
        assert!(Urgency::Medium < Urgency::High);
        assert!(Urgency::High < Urgency::Critical);
    }

    #[test]
    fn prosoche_result_serialization() {
        let result = ProsocheResult {
            items: vec![AttentionItem {
                category: AttentionCategory::Task,
                summary: "test".to_owned(),
                urgency: Urgency::High,
            }],
            checked_at: "2026-01-01T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let back: ProsocheResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.checked_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn attention_item_serialization() {
        let item = AttentionItem {
            category: AttentionCategory::Calendar,
            summary: "standup".to_owned(),
            urgency: Urgency::Medium,
        };
        let json = serde_json::to_string(&item).expect("serialize");
        assert!(json.contains("Calendar"));
        assert!(json.contains("standup"));
        assert!(json.contains("Medium"));
    }

    #[test]
    fn parse_vmrss_extracts_value() {
        let content = "\
Name:   aletheia
VmPeak:   500000 kB
VmRSS:   123456 kB
Threads:  8
";
        let rss = parse_vmrss(content).expect("should parse");
        assert_eq!(rss, 123_456);
    }

    #[test]
    fn parse_vmrss_missing_returns_error() {
        let content = "Name: aletheia\nThreads: 8\n";
        assert!(parse_vmrss(content).is_err());
    }

    #[test]
    fn check_memory_runs_without_panic() {
        let items = check_memory();
        assert!(
            items.len() <= 1,
            "test process should not exceed memory thresholds"
        );
    }

    #[test]
    fn check_db_sizes_empty_paths() {
        let items = check_db_sizes(&[]);
        assert!(items.is_empty());
    }

    #[test]
    fn check_db_sizes_nonexistent_file() {
        let items = check_db_sizes(&[PathBuf::from("/tmp/nonexistent-db-file-for-test.db")]);
        assert!(
            items.is_empty(),
            "nonexistent file should not produce items"
        );
    }

    #[test]
    fn check_db_sizes_small_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db_path = dir.path().join("test.db");
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        std::fs::write(&db_path, b"small content").expect("write test file");

        let items = check_db_sizes(&[db_path]);
        assert!(items.is_empty(), "small file should not trigger warning");
    }

    #[tokio::test]
    async fn prosoche_with_data_dir_runs_disk_check() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let check = ProsocheCheck::new("test-nous").with_data_dir(dir.path());
        let result = check.run().await.expect("should succeed");
        assert!(!result.checked_at.is_empty());
    }

    #[tokio::test]
    async fn prosoche_with_db_paths_runs_size_check() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db_path = dir.path().join("test.db");
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        std::fs::write(&db_path, b"data").expect("write");
        let check = ProsocheCheck::new("test-nous").with_db_paths(vec![db_path]);
        let result = check.run().await.expect("should succeed");
        assert!(!result.checked_at.is_empty());
    }

    #[test]
    fn parse_df_percent_valid() {
        let output = "Use%\n 42%\n";
        let percent = parse_df_percent(output).expect("should parse");
        assert!((percent - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_df_percent_no_data() {
        let output = "Use%\n";
        assert!(parse_df_percent(output).is_err());
    }
}
