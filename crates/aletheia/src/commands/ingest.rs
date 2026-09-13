//! `aletheia ingest`: file-based knowledge ingestion.

use std::path::{Path, PathBuf};

use clap::Parser;
use snafu::prelude::*;

use pylon::client::GatewayClient;

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
    pub nous_id: koina::id::NousId,
    /// Preview without mutating the knowledge store.
    #[arg(long)]
    pub dry_run: bool,
    /// Server URL for API routing when server is running.
    #[arg(long, default_value = crate::cli::DEFAULT_GATEWAY_URL)]
    pub url: String,
    /// Bearer token for API routes that require authentication.
    #[arg(long, env = "ALETHEIA_API_TOKEN")]
    pub token: Option<String>,
}

pub(crate) async fn run(args: &IngestArgs, instance_root: Option<&PathBuf>) -> Result<()> {
    validate_inputs(args)?;

    if crate::commands::is_knowledge_server_running(&args.url).await? {
        return run_via_api(args).await;
    }

    #[cfg(feature = "recall")]
    {
        let oikos = super::resolve_oikos(instance_root)?;
        let knowledge_path = oikos.knowledge_cohort_db("shared");
        crate::commands::require_knowledge_store_exists(&oikos, &knowledge_path)?;

        let config = crate::knowledge_config::knowledge_config_from_loaded(
            taxis::loader::load_config(&oikos).ok().as_ref(),
            false,
        );
        let store = mneme::knowledge_store::KnowledgeStore::open_fjall(&knowledge_path, config)
            .whatever_context("failed to open knowledge store")?;

        run_direct(args, &store).await
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

// `is_knowledge_server_running` is canonical at `crate::commands` (#7023) —
// its own tests live there rather than being restated per call site.

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

    let client = GatewayClient::new(&args.url, args.token.clone())
        .whatever_context("failed to build HTTP client")?;

    let mut total_inserted = 0usize;
    let mut total_skipped = 0usize;
    let mut errored: Vec<(PathBuf, String)> = Vec::new();

    for file in &files {
        let content = match read_ingest_text(file).await {
            Ok(c) => c,
            Err(msg) => {
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
                match count_facts(&content, format_str, args.nous_id.as_str()) {
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

        // WHY(#5100): auth (bearer token), CSRF, and content-type headers are
        // no longer built per call site — `GatewayClient` applies them once,
        // from the token this client was constructed with.
        match client
            .ingest(&content, format_str, args.nous_id.as_str())
            .await
        {
            Ok(result) => {
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
            // A malformed request (400) or a validation failure (422) is
            // this file's problem, not the run's -- log it and keep going,
            // matching every other per-file failure in this loop.
            Err(pylon::client::Error::Server {
                status, message, ..
            }) if matches!(status, 400 | 422) => {
                let msg = format!("{status}: {message}");
                tracing::warn!(file = %file.display(), error = %msg, "ingest skipping file");
                eprintln!("[warn] {}: {msg}", file.display());
                errored.push((file.clone(), msg));
            }
            // Everything else (auth failure, the knowledge store being
            // unavailable, a transport error) is fatal for the whole run --
            // `GatewayClient`'s `Error` already renders it clearly.
            Err(e) => {
                whatever!("ingest API request failed: {e}");
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
async fn run_direct(
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
        match process_file(file, args, store).await {
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
async fn process_file(
    file: &Path,
    args: &IngestArgs,
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> Result<(usize, usize)> {
    // WHY(#6751) the same PDF decode as the async path: this is the second read site,
    // and fixing only one would make `aletheia ingest` support PDFs on one code path
    // and fail on invalid UTF-8 on the other, which reads as an intermittent bug.
    let content = read_ingest_text(file)
        .await
        .map_err(crate::error::Error::msg)?;

    let format_str = if args.format == "auto" {
        detect_format(file).unwrap_or("text")
    } else {
        &args.format
    };

    let format = mneme::ingest::parse_format(format_str)
        .ok_or_else(|| crate::error::Error::msg(format!("unsupported format: {format_str}")))?;

    let config = mneme::ingest::IngestConfig::default();
    let facts = mneme::ingest::ingest_content(&content, format, &config, args.nous_id.as_str())
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
        Some("md" | "markdown" | "txt" | "text" | "json" | "jsonl" | "pdf")
    )
}

/// Read a file's text, decoding a PDF rather than failing on its bytes.
///
/// WHY(#6751) this exists rather than a `parse_format` arm: `IngestFormat` names how
/// text is CHUNKED -- markdown, plain text, json, jsonl. A PDF is not a chunking
/// strategy, it is a container that yields text, so it belongs at the read boundary and
/// the format stays whatever the extracted text is. That is also why nothing here
/// touches pylon's ingest endpoint: its `content` field is a `String` over JSON, so a
/// PDF cannot reach it at all and rejecting `"pdf"` there is correct.
///
/// Before this, `read_to_string` on a PDF failed on invalid UTF-8, so the operator got
/// "stream did not contain valid UTF-8" for a file the workspace can read perfectly
/// well one crate away.
async fn read_ingest_text(file: &Path) -> std::result::Result<String, String> {
    if file.extension().and_then(|e| e.to_str()) == Some("pdf") {
        let bytes = tokio::fs::read(file)
            .await
            .map_err(|e| format!("failed to read {}: {e}", file.display()))?;
        // WHY `extract_pdf_text` and not `inspect_pdf`: the latter caps its output at
        // 100 lines because it summarises. Ingesting that would record the first
        // hundred lines of a PDF as the whole document.
        return poiesis_inspect::extract_pdf_text(&bytes)
            .map_err(|e| format!("failed to extract text from {}: {e}", file.display()));
    }
    tokio::fs::read_to_string(file)
        .await
        .map_err(|e| format!("failed to read {}: {e}", file.display()))
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

    /// Spawn a one-shot stub server: accept a single connection, read
    /// whatever the caller sends, and reply with `status_line` + a JSON
    /// `body`. Returns the address to point `--url` at and a handle that
    /// resolves to the raw request text once awaited — shared by every test
    /// below that only cares about one request/response round trip.
    async fn spawn_stub_response(
        status_line: &str,
        body: &str,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let status_line = status_line.to_owned();
        let body = body.to_owned();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 4096];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(buf.get(..n).unwrap_or_default()).into_owned();
            let response = format!(
                "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            request
        });
        (addr, handle)
    }

    /// `IngestArgs` for a live (non-dry-run) run against `url`, format and
    /// agent id fixed — shared by every test below that only varies the
    /// stub response and the bearer token.
    fn live_args(input: PathBuf, url: String, token: Option<String>) -> IngestArgs {
        IngestArgs {
            path: input,
            format: "auto".to_owned(),
            nous_id: koina::id::NousId::new("alice").unwrap(),
            dry_run: false,
            url,
            token,
        }
    }

    #[tokio::test]
    async fn api_dry_run_does_not_send_post() {
        organon::testing::install_crypto_provider();
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
            nous_id: koina::id::NousId::new("alice").unwrap(),
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

    /// PROOF(#5100): `ingest` now goes through `pylon::client::GatewayClient`
    /// instead of a hand-rolled `reqwest::Client` — the shared client
    /// attaches the bearer token and CSRF header on every request rather
    /// than `run_via_api` building an `Authorization` header itself. This
    /// fails before the migration (no header would be attached without the
    /// removed manual `.header("Authorization", ...)` call, and the CSRF
    /// header was never sent at all) and passes after.
    #[tokio::test]
    async fn api_live_run_sends_through_the_shared_client() {
        organon::testing::install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.txt");
        std::fs::write(&input, "one fact").unwrap();

        let (addr, server) = spawn_stub_response(
            "HTTP/1.1 200 OK",
            r#"{"inserted":1,"skipped":0,"errors":[]}"#,
        )
        .await;
        let args = live_args(input, format!("http://{addr}"), Some("tok".to_owned()));

        run_via_api(&args).await.unwrap();
        let request = server.await.unwrap();

        assert!(
            request.starts_with("POST /api/v1/knowledge/ingest"),
            "got: {request}"
        );
        assert!(
            request.to_lowercase().contains("authorization: bearer tok"),
            "shared client must attach the bearer token: {request}"
        );
        assert!(
            request.contains("x-requested-with"),
            "shared client must attach the CSRF header: {request}"
        );
    }

    /// PROOF(review #5100): a 422 (validation failure) from the shared
    /// client is per-file recoverable, exactly like every other per-file
    /// failure in this loop — it must not abort the run. Reaching `Ok(())`
    /// here is only possible if the loop matched the `Error::Server{422}`
    /// arm and `continue`d rather than falling through to the fatal `Err(e)`
    /// arm, since a single-file run that hit the fatal arm would return
    /// `Err` instead.
    #[tokio::test]
    async fn api_live_run_recovers_from_a_422_and_continues() {
        organon::testing::install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.txt");
        std::fs::write(&input, "one fact").unwrap();

        let (addr, server) = spawn_stub_response(
            "HTTP/1.1 422 Unprocessable Entity",
            r#"{"error":{"code":"validation_failed","message":"bad fact shape","request_id":null}}"#,
        )
        .await;
        let args = live_args(input, format!("http://{addr}"), None);

        let result = run_via_api(&args).await;
        server.await.unwrap();

        assert!(
            result.is_ok(),
            "a 422 must be recoverable per-file, not fatal to the whole run: {result:?}"
        );
    }

    /// PROOF(review #5100): everything other than a per-file-recoverable
    /// 400/422 is fatal for the whole run, including an auth failure — the
    /// shared client's `Error::Auth` must propagate out of `run_via_api`
    /// rather than being swallowed as another skipped file.
    #[tokio::test]
    async fn api_live_run_fails_the_whole_run_on_a_401() {
        organon::testing::install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.txt");
        std::fs::write(&input, "one fact").unwrap();

        let (addr, server) = spawn_stub_response(
            "HTTP/1.1 401 Unauthorized",
            r#"{"error":{"code":"unauthorized","message":"missing or invalid token","request_id":null}}"#,
        )
        .await;
        let args = live_args(input, format!("http://{addr}"), None);

        let err = run_via_api(&args).await.unwrap_err();
        server.await.unwrap();

        assert!(
            err.to_string().to_lowercase().contains("auth"),
            "got: {err}"
        );
    }

    /// PROOF(#5100): `ingest`'s `--url` no longer restates its own
    /// gateway-URL default — it resolves to the one constant every
    /// HTTP-backed command shares.
    #[test]
    fn default_url_matches_the_shared_gateway_default() {
        use clap::Parser as _;

        let args = IngestArgs::try_parse_from(["ingest", "/tmp/x"]).unwrap();
        assert_eq!(args.url, crate::cli::DEFAULT_GATEWAY_URL);
    }

    fn args_with(path: PathBuf, format: &str, nous_id: &str) -> IngestArgs {
        IngestArgs {
            path,
            format: format.to_owned(),
            nous_id: koina::id::NousId::new(nous_id).unwrap(),
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
        assert_eq!(args.nous_id.as_str(), koina::defaults::DEFAULT_AGENT_ID);
    }

    #[test]
    fn parse_rejects_empty_nous_id() {
        use clap::Parser as _;

        // WHY: --nous-id is a validated `NousId`, so an empty or whitespace
        // value is rejected by clap at parse time rather than by a downstream
        // check (#6755).
        assert!(IngestArgs::try_parse_from(["ingest", "/tmp/x", "--nous-id", "   "]).is_err());
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

    /// Regression for #4164/B: a directory containing one unparseable file
    /// used to abort the whole ingest after partially mutating the store.
    /// Now the bad file is logged + counted as errored and the remaining
    /// files still go through. Uses dry-run so no store insert happens —
    /// the failure surface being tested is the parse step (`ingest_content`),
    /// which fires before the store call in `process_file`.
    #[tokio::test]
    async fn run_direct_dry_run_continues_after_bad_file() {
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
                nous_id: koina::id::NousId::new("alice").unwrap(),
                dry_run: true,
                url: "http://127.0.0.1:1".to_owned(),
                token: None,
            };

            let result = run_direct(&args, &store).await;
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
    #[tokio::test]
    async fn run_direct_live_continues_after_bad_file() {
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
                nous_id: koina::id::NousId::new("alice").unwrap(),
                dry_run: false,
                url: "http://127.0.0.1:1".to_owned(),
                token: None,
            };

            let result = run_direct(&args, &store).await;
            assert!(
                result.is_ok(),
                "run_direct should return Ok despite the bad file: {result:?}"
            );
        }
    }
}
