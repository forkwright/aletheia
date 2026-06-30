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
    #[arg(short, long, default_value = koina::defaults::DEFAULT_AGENT_ID)]
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
    pub nous_id: String,
    /// Preview without mutating the knowledge store.
    #[arg(long)]
    pub dry_run: bool,
    /// Server URL for API routing when server is running.
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub url: String,
    /// Bearer token for API routes that require authentication.
    #[arg(long, env = "ALETHEIA_API_TOKEN")]
    pub token: Option<String>,
}

pub(crate) async fn run(args: &IngestArgs, instance_root: Option<&PathBuf>) -> Result<()> {
    validate_inputs(args)?;

    if is_server_running(&args.url).await? {
        return run_via_api(args).await;
    }

    #[cfg(feature = "recall")]
    {
        let oikos = super::resolve_oikos(instance_root)?;
        let knowledge_path = oikos.knowledge_cohort_db("shared");
        if !knowledge_path.exists() && !oikos.knowledge_db().exists() {
            whatever!(
                "knowledge store not initialized at {}\n  \
                 The store is created lazily by the running server. Either:\n    \
                   1. Start the server once to bootstrap it:  aletheia\n    \
                   2. Or route this command through a running server with --url",
                knowledge_path.display()
            );
        }

        let config = taxis::loader::load_config(&oikos).ok().map_or_else(
            mneme::knowledge_store::KnowledgeConfig::default,
            |config| {
                let embedding = config.embedding.to_embedding_config();
                mneme::knowledge_store::KnowledgeConfig {
                    dim: config.embedding.dimension,
                    embedding_model: embedding.effective_model_name(),
                    ..Default::default()
                }
            },
        );
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
    if let Err(e) = reqwest::Url::parse(url) {
        whatever!("--url is not a valid URL: {e} (got {:?})", url);
    }
    let endpoint = format!("{url}/api/health");
    match reqwest::get(&endpoint).await {
        Ok(resp) => Ok(resp.status().is_success() || resp.status().as_u16() == 503),
        Err(_) => Ok(false),
    }
}

/// Per-fact error returned by the ingest API.
#[derive(Debug, serde::Deserialize)]
struct IngestApiFactError {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    message: String,
}

/// Response body returned by the ingest API.
#[derive(Debug, serde::Deserialize)]
struct IngestApiResponse {
    inserted: usize,
    skipped: usize,
    #[serde(default)]
    errors: Vec<IngestApiFactError>,
}

#[expect(
    clippy::too_many_lines,
    reason = "API ingest preserves direct-mode per-file reporting in one command flow"
)]
async fn run_via_api(args: &IngestArgs) -> Result<()> {
    let files = files_to_ingest(&args.path)?;
    if files.is_empty() {
        println!("No supported files found in {}", args.path.display());
        return Ok(());
    }

    let endpoint = format!("{}/api/v1/knowledge/ingest", args.url);
    let client = reqwest::Client::new();

    let mut total_inserted = 0usize;
    let mut total_skipped = 0usize;
    let mut errored: Vec<(PathBuf, String)> = Vec::new();

    for file in &files {
        let content = match tokio::fs::read_to_string(file).await {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("failed to read {}: {e}", file.display());
                tracing::warn!(file = %file.display(), error = %msg, "ingest skipping file");
                eprintln!("[warn] {}: {msg}", file.display());
                errored.push((file.clone(), msg));
                continue;
            }
        };

        let format_str = if args.format == "auto" {
            detect_format(file).unwrap_or("text")
        } else {
            &args.format
        };

        if args.dry_run {
            #[cfg(feature = "recall")]
            {
                match count_facts(&content, format_str, &args.nous_id) {
                    Ok(n) => {
                        println!("[dry-run] {}: would insert {} facts", file.display(), n);
                    }
                    Err(e) => {
                        let msg = format!("failed to parse {}: {e}", file.display());
                        tracing::warn!(file = %file.display(), error = %msg, "ingest skipping file");
                        eprintln!("[warn] {}: {msg}", file.display());
                        errored.push((file.clone(), msg));
                    }
                }
            }
            #[cfg(not(feature = "recall"))]
            {
                println!(
                    "[dry-run] {}: would POST {} bytes as {}",
                    file.display(),
                    content.len(),
                    format_str
                );
            }
            continue;
        }

        if content.trim().is_empty() {
            println!("{}: inserted 0, skipped 0", file.display());
            continue;
        }

        let body = serde_json::json!({
            "content": content,
            "format": format_str,
            "nous_id": args.nous_id,
        });

        let mut request = client.post(&endpoint).json(&body);
        if let Some(t) = &args.token {
            request = request.header("Authorization", format!("Bearer {t}"));
        }

        let resp = request
            .send()
            .await
            .whatever_context("failed to send ingest request")?;

        if resp.status() == reqwest::StatusCode::BAD_REQUEST
            || resp.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY
        {
            let status = resp.status();
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "<unable to read response>".to_owned());
            let msg = format!("{status}: {text}");
            tracing::warn!(file = %file.display(), error = %msg, "ingest skipping file");
            eprintln!("[warn] {}: {msg}", file.display());
            errored.push((file.clone(), msg));
            continue;
        }

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            whatever!("authentication failed: API token required or invalid");
        }
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            whatever!("authorization failed: token lacks required permissions");
        }
        if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            whatever!("knowledge store is not enabled on the running server");
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "<unable to read response>".to_owned());
            whatever!("ingest API returned {status}: {text}");
        }

        let result: IngestApiResponse = resp
            .json()
            .await
            .whatever_context("failed to parse ingest response")?;

        println!(
            "{}: inserted {}, skipped {}",
            file.display(),
            result.inserted,
            result.skipped
        );

        for err in &result.errors {
            tracing::warn!(
                file = %file.display(),
                index = err.index,
                fact_id = ?err.id,
                error = %err.message,
                "fact insert failed"
            );
            eprintln!(
                "  [warn] fact {} ({}): {}",
                err.index,
                err.id.as_deref().unwrap_or("?"),
                err.message
            );
        }

        total_inserted += result.inserted;
        total_skipped += result.skipped;
    }

    println!(
        "\nTotal: inserted {total_inserted}, skipped {total_skipped}, errored {} (of {} files)",
        errored.len(),
        files.len()
    );
    if !errored.is_empty() {
        println!("\nFiles with errors:");
        for (path, err) in &errored {
            println!("  - {}: {err}", path.display());
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

    let files = files_to_ingest(path)?;

    let mut total_inserted = 0usize;
    let mut total_skipped = 0usize;
    let mut errored: Vec<(PathBuf, String)> = Vec::new();

    for file in &files {
        match process_file(file, args, store) {
            Ok((inserted, skipped)) => {
                total_inserted += inserted;
                total_skipped += skipped;
            }
            Err(e) => {
                // INVARIANT: per-file error is non-fatal — log + count + continue, so the rest of
                // the directory still lands. Previously a single bad file aborted the whole ingest
                // after partially mutating the store (#4164/B).
                let msg = e.to_string();
                tracing::warn!(file = %file.display(), error = %msg, "ingest skipping file");
                eprintln!("[warn] {}: {msg}", file.display());
                errored.push((file.clone(), msg));
            }
        }
    }

    println!(
        "\nTotal: inserted {total_inserted}, skipped {total_skipped}, errored {} (of {} files)",
        errored.len(),
        files.len()
    );
    if !errored.is_empty() {
        println!("\nFiles with errors:");
        for (path, err) in &errored {
            println!("  - {}: {err}", path.display());
        }
    }
    Ok(())
}

#[cfg(feature = "recall")]
fn process_file(
    file: &Path,
    args: &IngestArgs,
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> Result<(usize, usize)> {
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
        return Ok((0, 0));
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
    Ok((inserted, skipped))
}

/// Build the list of files to ingest from the supplied path.
///
/// A single file is returned as-is. A directory is walked recursively and only
/// files with supported extensions are included, matching the direct-path
/// behavior.
fn files_to_ingest(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        Ok(vec![path.to_path_buf()])
    } else if path.is_dir() {
        let mut files = Vec::new();
        collect_dir_files(path, &mut files)?;
        Ok(files)
    } else {
        whatever!("unsupported path type: {}", path.display());
    }
}

/// Recursively collect supported files from a directory.
fn collect_dir_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_whatever_context(|_| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry.whatever_context("failed to read directory entry")?;
        let path = entry.path();
        if path.is_dir() {
            collect_dir_files(&path, files)?;
        } else if path.is_file() && is_supported_extension(&path) {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(feature = "recall")]
fn count_facts(content: &str, format_str: &str, nous_id: &str) -> Result<usize> {
    let format = mneme::ingest::parse_format(format_str)
        .ok_or_else(|| crate::error::Error::msg(format!("unsupported format: {format_str}")))?;
    let facts = mneme::ingest::ingest_content(
        content,
        format,
        &mneme::ingest::IngestConfig::default(),
        nous_id,
    )
    .with_whatever_context(|_| format!("failed to parse content as {format_str}"))?;
    Ok(facts.len())
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
            token: None,
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
            token: None,
        }
    }

    /// Regression for #4245: the CLI `--nous-id` default must equal the
    /// agent id that `init -y` scaffolds. Both now resolve to the shared
    /// `koina::defaults::DEFAULT_AGENT_ID` constant, so this test plus the
    /// `scaffold_creates_pronoea_agent` assertion in `init::helpers` pin the
    /// two callsites against one source of truth.
    #[test]
    fn ingest_default_nous_id_matches_shared_default_agent_id() {
        use clap::Parser as _;

        let args = IngestArgs::try_parse_from(["ingest", "/tmp/x"]).unwrap();
        assert_eq!(args.nous_id, koina::defaults::DEFAULT_AGENT_ID);
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

    #[tokio::test]
    async fn is_server_running_rejects_empty_url() {
        let err = is_server_running("").await.unwrap_err();
        assert!(
            err.to_string().contains("--url is not a valid URL"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn is_server_running_rejects_malformed_url() {
        let err = is_server_running("not-a-url").await.unwrap_err();
        assert!(
            err.to_string().contains("--url is not a valid URL"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn is_server_running_returns_false_for_unreachable_well_formed_url() {
        let res = is_server_running("http://127.0.0.1:1").await.unwrap();
        assert!(!res, "expected false when no listener; got {res}");
    }

    /// Regression for #4164/B: a directory containing one unparseable file
    /// used to abort the whole ingest after partially mutating the store.
    /// Now the bad file is logged + counted as errored and the remaining
    /// files still go through. Uses dry-run so no store insert happens —
    /// the failure surface being tested is the parse step (`ingest_content`),
    /// which fires before the store call in `process_file`.
    #[test]
    fn run_direct_dry_run_continues_after_bad_file() {
        #[cfg(feature = "recall")]
        {
            let dir = tempfile::tempdir().unwrap();
            let docs = dir.path().join("docs");
            std::fs::create_dir(&docs).unwrap();
            std::fs::write(docs.join("good.md"), "# Section\nThe sky is blue.\n").unwrap();
            // Malformed JSON — used to abort the entire dir ingest.
            std::fs::write(docs.join("bad.json"), "{ not valid json").unwrap();
            std::fs::write(docs.join("more.md"), "## Heading\nMore content.\n").unwrap();

            let store_dir = dir.path().join("knowledge");
            let config = mneme::knowledge_store::KnowledgeConfig::default();
            let store =
                mneme::knowledge_store::KnowledgeStore::open_fjall(&store_dir, config).unwrap();

            let args = IngestArgs {
                path: docs,
                format: "auto".to_owned(),
                nous_id: "alice".to_owned(),
                dry_run: true,
                url: "http://127.0.0.1:1".to_owned(),
                token: None,
            };

            let result = run_direct(&args, &store);
            assert!(
                result.is_ok(),
                "run_direct should not propagate a per-file parse error; got {result:?}"
            );
        }
    }

    /// Same shape as the dry-run check, but exercise the live insert path:
    /// the bad file must not abort the loop. We assert only `Ok(())` —
    /// counting facts via the store would require a `CozoScript` query the
    /// public surface doesn't expose, which is out-of-scope for this fix.
    /// The dry-run case above covers the parse-error continuance contract.
    #[test]
    fn run_direct_live_continues_after_bad_file() {
        #[cfg(feature = "recall")]
        {
            let dir = tempfile::tempdir().unwrap();
            let docs = dir.path().join("docs");
            std::fs::create_dir(&docs).unwrap();
            std::fs::write(docs.join("a.md"), "# A\nfirst fact body.\n").unwrap();
            std::fs::write(docs.join("b.json"), "{ malformed").unwrap();
            std::fs::write(docs.join("c.md"), "# C\nthird fact body.\n").unwrap();

            let store_dir = dir.path().join("knowledge");
            let config = mneme::knowledge_store::KnowledgeConfig::default();
            let store =
                mneme::knowledge_store::KnowledgeStore::open_fjall(&store_dir, config).unwrap();

            let args = IngestArgs {
                path: docs,
                format: "auto".to_owned(),
                nous_id: "alice".to_owned(),
                dry_run: false,
                url: "http://127.0.0.1:1".to_owned(),
                token: None,
            };

            let result = run_direct(&args, &store);
            assert!(
                result.is_ok(),
                "run_direct should return Ok despite the bad file: {result:?}"
            );
        }
    }
}
