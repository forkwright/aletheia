//! System status display for the `aletheia status` CLI command.

use std::path::Path;

use anyhow::{Context, Result};
use owo_colors::OwoColorize;

/// Run the status command against a running (or stopped) instance.
pub async fn run(url: &str, instance_root: Option<&std::path::PathBuf>) -> Result<()> {
    let use_color = supports_color::on(supports_color::Stream::Stdout).is_some();
    let version = env!("CARGO_PKG_VERSION");

    if use_color {
        println!(
            "{} {} — {}",
            "Aletheia".bold(),
            format!("v{version}").dimmed(),
            "Status".bold()
        );
    } else {
        println!("Aletheia v{version} — Status");
    }
    println!("{}", "═".repeat(30));
    println!();

    // Try to connect to pylon
    let health = fetch_health(url).await;
    let nous_list = fetch_nous(url).await;

    match health {
        Ok(ref h) => print_gateway_up(url, h, use_color),
        Err(_) => print_gateway_down(url, use_color),
    }
    println!();

    if let Ok(ref h) = health {
        print_checks(&h.checks, use_color);
    }

    if let Ok(list) = nous_list {
        print_nous(&list, use_color);
    }

    // Disk stats
    let oikos = match instance_root {
        Some(root) => aletheia_taxis::oikos::Oikos::from_root(root),
        None => aletheia_taxis::oikos::Oikos::discover(),
    };
    print_storage(&oikos, use_color);

    Ok(())
}

#[derive(serde::Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_seconds: u64,
    checks: Vec<HealthCheck>,
}

#[derive(serde::Deserialize)]
struct HealthCheck {
    name: String,
    status: String,
    message: Option<String>,
}

#[derive(serde::Deserialize)]
struct NousInfo {
    id: String,
    lifecycle: String,
    session_count: usize,
}

async fn fetch_health(url: &str) -> Result<HealthResponse> {
    let endpoint = format!("{url}/api/health");
    let resp = reqwest::get(&endpoint).await.context("failed to connect")?;
    resp.json().await.context("failed to parse health response")
}

async fn fetch_nous(url: &str) -> Result<Vec<NousInfo>> {
    let endpoint = format!("{url}/api/v1/nous");
    let resp = reqwest::get(&endpoint).await.context("failed to connect")?;
    resp.json().await.context("failed to parse nous response")
}

fn print_gateway_up(url: &str, health: &HealthResponse, color: bool) {
    let uptime = format_uptime(health.uptime_seconds);
    if color {
        let status_colored = match health.status.as_str() {
            "healthy" => "UP".green().to_string(),
            "degraded" => "DEGRADED".yellow().to_string(),
            _ => "UNHEALTHY".red().to_string(),
        };
        println!(
            "  {:<12}{} — {} (uptime: {}, v{})",
            "Gateway:".bold(),
            url,
            status_colored,
            uptime,
            health.version
        );
    } else {
        let status_label = match health.status.as_str() {
            "healthy" => "UP",
            "degraded" => "DEGRADED",
            _ => "UNHEALTHY",
        };
        println!(
            "  {:<12}{} — {} (uptime: {}, v{})",
            "Gateway:", url, status_label, uptime, health.version
        );
    }
}

fn print_gateway_down(url: &str, color: bool) {
    if color {
        println!(
            "  {:<12}{} — {}",
            "Gateway:".bold(),
            url,
            "DOWN".red().bold()
        );
    } else {
        println!("  {:<12}{} — DOWN", "Gateway:", url);
    }
}

fn print_checks(checks: &[HealthCheck], color: bool) {
    if checks.is_empty() {
        return;
    }
    if color {
        println!("  {}:", "Checks".bold());
    } else {
        println!("  Checks:");
    }
    for check in checks {
        let status = match check.status.as_str() {
            "pass" => {
                if color {
                    "PASS".green().to_string()
                } else {
                    "PASS".to_owned()
                }
            }
            "warn" => {
                if color {
                    "WARN".yellow().to_string()
                } else {
                    "WARN".to_owned()
                }
            }
            _ => {
                if color {
                    "FAIL".red().to_string()
                } else {
                    "FAIL".to_owned()
                }
            }
        };
        let msg = check
            .message
            .as_deref()
            .map(|m| format!(" ({m})"))
            .unwrap_or_default();
        println!("    {:<18}{}{}", check.name, status, msg);
    }
    println!();
}

fn print_nous(list: &[NousInfo], color: bool) {
    if list.is_empty() {
        return;
    }
    if color {
        println!("  {}:", "Nous".bold());
    } else {
        println!("  Nous:");
    }
    for nous in list {
        let lifecycle = match nous.lifecycle.as_str() {
            "idle" => {
                if color {
                    "IDLE".green().to_string()
                } else {
                    "IDLE".to_owned()
                }
            }
            "processing" => {
                if color {
                    "BUSY".yellow().to_string()
                } else {
                    "BUSY".to_owned()
                }
            }
            _ => nous.lifecycle.to_uppercase(),
        };
        println!(
            "    {:<14} {:<8} ({} sessions)",
            nous.id, lifecycle, nous.session_count
        );
    }
    println!();
}

fn print_storage(oikos: &aletheia_taxis::oikos::Oikos, color: bool) {
    if color {
        println!("  {}:", "Storage".bold());
    } else {
        println!("  Storage:");
    }

    let data_dir = oikos.data();
    if data_dir.exists() {
        print_file_size("sessions.db", &oikos.sessions_db());
        let plans_db = data_dir.join("plans.db");
        print_file_size("plans.db", &plans_db);
    } else {
        println!("    (data directory not found)");
    }
    println!();
}

fn print_file_size(label: &str, path: &Path) {
    match std::fs::metadata(path) {
        Ok(meta) => {
            println!("    {:<18}{}", label, format_bytes(meta.len()));
        }
        Err(_) => {
            println!("    {label:<18}(not found)");
        }
    }
}

/// Format seconds as human-readable duration.
pub fn format_uptime(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    let remaining_minutes = minutes % 60;
    if hours < 24 {
        return format!("{hours}h {remaining_minutes}m");
    }
    let days = hours / 24;
    let remaining_hours = hours % 24;
    format!("{days}d {remaining_hours}h")
}

/// Format bytes as human-readable size.
pub fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_owned();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    #[expect(
        clippy::cast_precision_loss,
        reason = "file sizes fit comfortably in f64 mantissa"
    )]
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < units.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", units[unit_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_uptime_seconds() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(59), "59s");
    }

    #[test]
    fn format_uptime_minutes() {
        assert_eq!(format_uptime(60), "1m");
        assert_eq!(format_uptime(3599), "59m");
    }

    #[test]
    fn format_uptime_hours() {
        assert_eq!(format_uptime(3600), "1h 0m");
        assert_eq!(format_uptime(7200 + 1800), "2h 30m");
    }

    #[test]
    fn format_uptime_days() {
        assert_eq!(format_uptime(86400 + 14 * 3600), "1d 14h");
        assert_eq!(format_uptime(3 * 86400 + 14 * 3600), "3d 14h");
    }

    #[test]
    fn format_bytes_zero() {
        assert_eq!(format_bytes(0), "0 B");
    }

    #[test]
    fn format_bytes_small() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn format_bytes_kb() {
        assert_eq!(format_bytes(1024), "1.0 KB");
    }

    #[test]
    fn format_bytes_mb() {
        assert_eq!(format_bytes(45 * 1024 * 1024), "45.0 MB");
    }

    #[test]
    fn format_bytes_gb() {
        let bytes = 1_610_612_736_u64; // 1.5 GB
        assert_eq!(format_bytes(bytes), "1.5 GB");
    }
}
