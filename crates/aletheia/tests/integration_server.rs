#![cfg(feature = "storage-fjall")]
// Integration test: start the server, verify health, send a session request.
// Uses raw TCP + HTTP/1.1 to avoid reqwest TLS provider requirements.
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
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
        path = PathBuf::from(env!("CARGO_BIN_EXE_aletheia"));
    }
    path
}

fn setup_instance(port: u16) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let instance = tmp.path();

    std::fs::create_dir_all(instance.join("config")).unwrap();
    std::fs::create_dir_all(instance.join("data")).unwrap();
    std::fs::create_dir_all(instance.join("logs")).unwrap();
    std::fs::create_dir_all(instance.join("nous/_default")).unwrap();

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

/// Send a raw HTTP GET request and return (status_code, body).
fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;

    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).ok()?;

    // Parse "HTTP/1.1 200 OK"
    let status_code: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;

    // Skip headers until blank line
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        if line.trim().is_empty() {
            break;
        }
    }

    // Read body
    let mut body = String::new();
    let _ = reader.read_line(&mut body);
    // For chunked encoding, read remaining lines too
    let mut rest = String::new();
    let _ = std::io::Read::read_to_string(&mut reader, &mut rest);
    body.push_str(&rest);

    Some((status_code, body))
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
    let mut ready = false;
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(500));
        if let Some((code, _)) = http_get(port, "/api/health") {
            if code == 200 {
                ready = true;
                break;
            }
        }
    }

    if !ready {
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
    let (code, body) = http_get(port, "/api/health").expect("health request");
    assert_eq!(code, 200);
    assert!(body.contains("\"healthy\""), "health body: {body}");

    // Verify sessions endpoint
    let (code, _) = http_get(port, "/api/v1/sessions").expect("sessions request");
    assert_eq!(code, 200);

    // Verify metrics endpoint
    let (code, body) = http_get(port, "/metrics").expect("metrics request");
    assert_eq!(code, 200);
    assert!(
        body.contains("aletheia_uptime_seconds"),
        "metrics body missing uptime"
    );

    // Clean shutdown
    child.kill().expect("kill server");
    child.wait().ok();
}
