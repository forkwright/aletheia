//! `aletheia ingest`: file-based knowledge ingestion.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::Parser;
use snafu::prelude::*;

use crate::error::Result;

/// Arguments for the `ingest` subcommand.
#[derive(Debug, Clone, Parser)]
pub(crate) struct IngestArgs {
    /// Path to file or directory to ingest.
    pub path: PathBuf,
    /// Ingestion format (auto-detected by default).
    #[arg(short, long, default_value = "auto")]
    pub format: String,
    /// Nous agent ID that will own the extracted facts.
    #[arg(short, long, default_value = "default")]
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
    pub nous_id: String,
    /// Preview without mutating the knowledge store.
    #[arg(long)]
    pub dry_run: bool,
    /// Server URL for API routing when server is running.
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub url: String,
}

pub(crate) async fn run(args: &IngestArgs, instance_root: Option<&PathBuf>) -> Result<()> {
    validate_inputs(args)?;

    if let Ok(true) = is_server_running(&args.url).await {
        return run_via_api(args).await;
    }

    #[cfg(feature = "recall")]
    {
        let oikos = super::resolve_oikos(instance_root)?;
        let knowledge_path = oikos.knowledge_cohort_db("shared");
        if !knowledge_path.exists() && !oikos.knowledge_db().exists() {
            whatever!(
                "knowledge store not found at {}\n  \
                 Has this instance been initialized with recall enabled?",
                knowledge_path.display()
            );
        }

        let config = mneme::knowledge_store::KnowledgeConfig::default();
        let store = mneme::knowledge_store::KnowledgeStore::open_fjall(&knowledge_path, config)
            .whatever_context("failed to open knowledge store")?;

        run_direct(args, &store)
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = instance_root;
        whatever!(
            "ingest requires the 'recall' feature.\n  \
             Build with: cargo build --features recall"
        );
    }
}

fn validate_inputs(args: &IngestArgs) -> Result<()> {
    if args.nous_id.trim().is_empty() {
        whatever!("--nous-id must not be empty");
    }
    if !is_valid_format(&args.format) {
        whatever!(
            "unsupported --format: {}\n  \
             expected one of: auto, markdown, md, text, plain_text, json, jsonl",
            args.format
        );
    }
    if !args.path.exists() {
        whatever!("path does not exist: {}", args.path.display());
    }
    Ok(())
}

fn is_valid_format(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "auto"
            | "markdown"
            | "md"
            | "text"
            | "plain_text"
            | "plaintext"
            | "plain text"
            | "json"
            | "jsonl"
    )
}

async fn is_server_running(url: &str) -> Result<bool> {
    let endpoint = format!("{url}/api/health");
    match reqwest::get(&endpoint).await {
        Ok(resp) => Ok(resp.status().is_success() || resp.status().as_u16() == 503),
        Err(_) => Ok(false),
    }
}

async fn run_via_api(args: &IngestArgs) -> Result<()> {
    let content = read_path(&args.path).await?;
    let format = if args.format == "auto" {
        detect_format(&args.path).unwrap_or("text")
    } else {
        &args.format
    };

    let endpoint = format!("{}/api/v1/knowledge/ingest", args.url);
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "content": content,
        "format": format,
        "nous_id": args.nous_id,
    });

    if args.dry_run {
        println!("[dry-run] would POST to {endpoint}");
        println!(
            "{}",
            serde_json::to_string_pretty(&body)
                .whatever_context("failed to serialize dry-run request")?
        );
        return Ok(());
    }

    let resp = client
        .post(&endpoint)
        .json(&body)
        .send()
        .await
        .whatever_context("failed to send ingest request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read response>".to_owned());
        whatever!("ingest API returned {status}: {text}");
    }

    let result: serde_json::Value = resp
        .json()
        .await
        .whatever_context("failed to parse ingest response")?;

    let inserted = result
        .get("inserted")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let skipped = result
        .get("skipped")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    println!("Ingested: {inserted} facts, skipped: {skipped}");

    if let Some(errors) = result.get("errors").and_then(|v| v.as_array())
        && !errors.is_empty()
    {
        println!("\nErrors:");
        for err in errors {
            if let Some(msg) = err.get("message").and_then(|v| v.as_str()) {
                println!("  - {msg}");
            }
        }
    }

    Ok(())
}

#[cfg(feature = "recall")]
fn run_direct(
    args: &IngestArgs,
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> Result<()> {
    let path = &args.path;
    if !path.exists() {
        whatever!("path does not exist: {}", path.display());
    }

    let mut files: Vec<PathBuf> = Vec::new();
    if path.is_dir() {
        collect_files(path, &mut files)?;
    } else {
        files.push(path.clone());
    }

    let mut total_inserted = 0usize;
    let mut total_skipped = 0usize;

    for file in &files {
        let content = std::fs::read_to_string(file)
            .with_whatever_context(|_| format!("failed to read {}", file.display()))?;

        let format_str = if args.format == "auto" {
            detect_format(file).unwrap_or("text")
        } else {
            &args.format
        };

        let format = mneme::ingest::parse_format(format_str)
            .ok_or_else(|| crate::error::Error::msg(format!("unsupported format: {format_str}")))?;

        let config = mneme::ingest::IngestConfig::default();
        let facts = mneme::ingest::ingest_content(&content, format, &config, &args.nous_id)
            .with_whatever_context(|_| format!("failed to parse {}", file.display()))?;

        if args.dry_run {
            println!(
                "[dry-run] {}: would insert {} facts",
                file.display(),
                facts.len()
            );
            continue;
        }

        let mut inserted = 0usize;
        let mut skipped = 0usize;
        for fact in &facts {
            match store.insert_fact(fact) {
                Ok(()) => inserted += 1,
                Err(e) => {
                    tracing::warn!(error = %e, fact_id = %fact.id, "fact insert failed");
                    skipped += 1;
                }
            }
        }

        println!("{}: inserted {inserted}, skipped {skipped}", file.display());
        total_inserted += inserted;
        total_skipped += skipped;
    }

    println!("\nTotal: inserted {total_inserted}, skipped {total_skipped}");
    Ok(())
}

async fn read_path(path: &Path) -> Result<String> {
    use tokio::io::AsyncReadExt;

    if !path.exists() {
        whatever!("path does not exist: {}", path.display());
    }

    if path.is_file() {
        let mut content = String::new();
        tokio::fs::File::open(path)
            .await
            .with_whatever_context(|_| format!("failed to open {}", path.display()))?
            .read_to_string(&mut content)
            .await
            .with_whatever_context(|_| format!("failed to read {}", path.display()))?;
        return Ok(content);
    }

    if path.is_dir() {
        let mut entries = tokio::fs::read_dir(path)
            .await
            .whatever_context("failed to read directory")?;
        let mut combined = String::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .whatever_context("failed to read directory entry")?
        {
            let path = entry.path();
            if path.is_file() && is_supported_extension(&path) {
                let mut content = String::new();
                if let Ok(mut file) = tokio::fs::File::open(&path).await
                    && file.read_to_string(&mut content).await.is_ok()
                {
                    // kanon:ignore RUST/no-silent-result-swallow — writing to a String never fails; std::fmt::Write returns Result for trait uniformity
                    let _ = writeln!(combined, "\n\n--- {} ---", path.display());
                    combined.push_str(&content);
                }
            }
        }
        return Ok(combined);
    }

    whatever!("unsupported path type: {}", path.display());
}

fn detect_format(path: &Path) -> Option<&'static str> {
    path.extension().and_then(|ext| match ext.to_str()? {
        "md" | "markdown" => Some("markdown"),
        "json" => Some("json"),
        "jsonl" => Some("jsonl"),
        _ => Some("text"),
    })
}

fn is_supported_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "markdown" | "txt" | "text" | "json" | "jsonl")
    )
}

#[cfg(feature = "recall")]
fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).whatever_context("failed to read directory")? {
        let entry = entry.whatever_context("failed to read directory entry")?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.is_file() && is_supported_extension(&path) {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test fixture writes one temporary input file before exercising async ingest"
)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn api_dry_run_does_not_send_post() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.txt");
        std::fs::write(&input, "one fact").unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            if let Ok(Ok((mut socket, _))) =
                tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept()).await
            {
                seen.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let body = r#"{"inserted":1,"skipped":0,"errors":[]}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let args = IngestArgs {
            path: input,
            format: "auto".to_owned(),
            nous_id: "alice".to_owned(),
            dry_run: true,
            url: format!("http://{addr}"),
        };

        run_via_api(&args).await.unwrap();
        server.await.unwrap();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            0,
            "dry-run must not contact the API"
        );
    }

    fn args_with(path: PathBuf, format: &str, nous_id: &str) -> IngestArgs {
        IngestArgs {
            path,
            format: format.to_owned(),
            nous_id: nous_id.to_owned(),
            dry_run: true,
            url: "http://127.0.0.1:1".to_owned(),
        }
    }

    #[test]
    fn validate_inputs_rejects_empty_nous_id() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("x.txt");
        std::fs::write(&input, "hi").unwrap();
        let err = validate_inputs(&args_with(input, "auto", "  ")).unwrap_err();
        assert!(
            err.to_string().contains("--nous-id must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_inputs_rejects_unknown_format() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("x.txt");
        std::fs::write(&input, "hi").unwrap();
        let err = validate_inputs(&args_with(input, "wat", "alice")).unwrap_err();
        assert!(
            err.to_string().contains("unsupported --format"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_inputs_rejects_missing_path() {
        let err = validate_inputs(&args_with(
            PathBuf::from("/no/such/path/aletheia-test"),
            "auto",
            "alice",
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("path does not exist"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_inputs_accepts_known_formats() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("x.txt");
        std::fs::write(&input, "hi").unwrap();
        for fmt in [
            "auto",
            "markdown",
            "md",
            "text",
            "plain_text",
            "json",
            "jsonl",
        ] {
            validate_inputs(&args_with(input.clone(), fmt, "alice"))
                .unwrap_or_else(|e| panic!("format {fmt} should be valid: {e}"));
        }
    }
}
