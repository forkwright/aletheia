//! `aletheia ingest`: file-based knowledge ingestion.

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
    pub nous_id: String,
    /// Preview without mutating the knowledge store.
    #[arg(long)]
    pub dry_run: bool,
    /// Server URL for API routing when server is running.
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    pub url: String,
}

pub(crate) async fn run(args: &IngestArgs, instance_root: Option<&PathBuf>) -> Result<()> {
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
        let _ = (args, instance_root);
        whatever!(
            "ingest requires the 'recall' feature.\n  \
             Build with: cargo build --features recall"
        );
    }
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

    if args.dry_run {
        println!("--dry-run: would have ingested (via API)");
    }

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
                    use std::fmt::Write as _;
                    let _ = writeln!(combined, "\n\n--- {} ---\n", path.display());
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
