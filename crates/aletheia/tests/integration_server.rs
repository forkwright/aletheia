#![cfg(feature = "storage-fjall")]
// Integration test: start the server, verify health, send a session request.
// Requires: compiled binary with default features. No LLM provider needed.
//
// This test exercises the deploy path: binary starts, opens stores, serves
// HTTP, and shuts down cleanly. It catches regressions like missing default
// features (embed-candle) and broken config parsing.
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn find_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to free port");
    listener.local_addr().unwrap().port()
}

fn binary_path() -> PathBuf {
    let mut path = std::env::current_exe()
        .expect("current exe")
        .parent()
        .expect("parent")
        .parent()
        .expect("grandparent")
        .to_path_buf();
    path.push("aletheia");
    if !path.exists() {
        // Fallback for different build layouts
        path = PathBuf::from(env!("CARGO_BIN_EXE_aletheia"));
    }
    path
}

fn setup_instance(port: u16) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let instance = tmp.path();

    // Minimal instance structure
    std::fs::create_dir_all(instance.join("config")).unwrap();
    std::fs::create_dir_all(instance.join("data")).unwrap();
    std::fs::create_dir_all(instance.join("logs")).unwrap();
    std::fs::create_dir_all(instance.join("nous/_default")).unwrap();

    // Minimal config: just gateway port and disabled auth
    let config = format!(
        r#"[gateway]
port = {port}
bind = "127.0.0.1"

[gateway.auth]
mode = "none"

[gateway.csrf]
enabled = false

[sandbox]
enabled = false
"#
    );
    std::fs::write(instance.join("config/aletheia.toml"), config).unwrap();

    tmp
}

#[test]
fn server_starts_serves_health_and_shuts_down() {
    let port = find_free_port();
    let instance = setup_instance(port);
    let binary = binary_path();

    if !binary.exists() {
        eprintln!("skipping: binary not found at {}", binary.display());
        return;
    }

    // Start server
    let mut child = Command::new(&binary)
        .args(["-r", instance.path().to_str().unwrap()])
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start aletheia");

    // Wait for server to be ready (up to 15 seconds)
    let url = format!("http://127.0.0.1:{port}/api/health");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    let mut ready = false;
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(resp) = client.get(&url).send()
            && resp.status().is_success()
        {
            ready = true;
            break;
        }
    }

    if !ready {
        // Capture stderr for debugging
        child.kill().ok();
        if let Some(stderr) = child.stderr.take() {
            let reader = BufReader::new(stderr);
            let lines: Vec<String> = reader
                .lines()
                .take(20)
                .filter_map(std::result::Result::ok)
                .collect();
            panic!(
                "server did not become healthy within 15s on port {port}. stderr:\n{}",
                lines.join("\n")
            );
        }
        panic!("server did not become healthy within 15s on port {port}");
    }

    // Verify health response
    let resp = client.get(&url).send().expect("health request");
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().expect("parse health json");
    assert_eq!(body["status"], "healthy");

    // Verify sessions endpoint exists
    let sessions_url = format!("http://127.0.0.1:{port}/api/v1/sessions");
    let resp = client.get(&sessions_url).send().expect("sessions request");
    assert!(resp.status().is_success());

    // Verify metrics endpoint
    let metrics_url = format!("http://127.0.0.1:{port}/metrics");
    let resp = client.get(&metrics_url).send().expect("metrics request");
    assert!(resp.status().is_success());
    let text = resp.text().unwrap();
    assert!(text.contains("aletheia_uptime_seconds"));

    // Clean shutdown
    child.kill().expect("kill server");
    child.wait().ok();
}
