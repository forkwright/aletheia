//! `aletheia poiesis`: CLI verb canon for the poiesis report-authoring tools
//! (forkwright/aletheia#7172, poiesis-evolution B-010).
//!
//! Wires the already-shipped organon poiesis builtins into a standalone CLI
//! surface -- no new MCP crate or MCP surface. Verb -> implementation, per
//! the B-010 spec's verb -> operation table
//! (`dev/planning/aletheia/planning/poiesis-evolution/B-010-poiesis-mcp-cli.md`
//! §1.2):
//!
//! - `create`  -> the `scaffold_report` organon tool: "Scaffold a
//!   `DeliverableSpec` skeleton" per the canon table. `scaffold_report`
//!   (`crates/organon/src/builtins/scaffold_report.rs`) generates a new
//!   report project (Typst and/or XLSX) from embedded templates given a
//!   slug/description -- the closest existing engine equivalent to the
//!   spec's brief.toml-driven scaffold. `create` is never a render: it
//!   either writes a project skeleton to `--dir` or prints the file
//!   manifest as JSON.
//! - `run`     -> the `generate_document` organon tool: the actual render
//!   verb (multi-format deliverable: odt, docx, xlsx, pdf, html, md, latex,
//!   epub). The canon table's `run` ("render a validated spec ... calls qa
//!   first, refuses on `has_issues`") additionally gates render on a prior
//!   `qa` pass and consumes a validated spec type neither of which exists
//!   in the shipped organon surface; there is no `poiesis.validate` tool,
//!   no `DeliverableSpec` parser, and no qa-then-render composite tool.
//!   This is a residual gap flagged on the issue (see the module's
//!   `create`/`run`/`preview --png` note below), not implemented here as
//!   new engine logic -- out of this issue's scope per its own text
//!   ("this does not include a new poiesis-mcp crate or any new MCP
//!   surface").
//! - `preview` -> the `render_typst_report` organon tool (fast, Typst-driven
//!   PDF preview; this is the only rendering backend shipped so far that
//!   supports a template-driven preview -- there is no PNG/raster export
//!   anywhere in the poiesis stack yet, so `--format` here is PDF-only).
//! - `qa`      -> the `qa_gate` organon tool (composite prose-lint plus
//!   optional factbase validation), gating: exits non-zero when the parsed
//!   `QaReport.has_issues` is true, per canon §2.3 ("qa ... exit
//!   non-zero iff the operation failed its gate").
//! - `lint`    -> the `lint_report` organon tool, standalone (canon's
//!   decomposition of `qa`'s prose half), gating: exits non-zero when any
//!   findings are returned.
//! - `verify`  -> the `verify_report` organon tool, standalone (canon's
//!   decomposition of `qa`'s citation half); the tool itself already
//!   signals failure via `ToolResult::error` on a failed or
//!   under-populated manifest, so no extra parsing is needed here.
//! - `list` / `get` -> `poiesis-core`'s `ComponentRegistry` directly. There
//!   is no organon tool for component-registry introspection (the organon
//!   poiesis builtins are document/report renderers, not a catalog), so
//!   this reaches `poiesis-core` the same way `aletheia ingest` reaches
//!   `poiesis-inspect` directly (see the Cargo.toml WHY note): going through
//!   the tool-registry crate that merely also depends on it would be a
//!   dependency on the wrong thing.
//!
//! Residual risk (forkwright/aletheia#7172's own clause: "if implementing
//! the CLI surfaces a verb with no MCP equivalent, that is a new finding to
//! raise on this issue"): three canon verbs have no exact engine
//! equivalent in the shipped organon `poiesis`/`scaffold_report` tools --
//! `create` as a brief.toml-driven `DeliverableSpec` scaffold (mapped
//! instead to the closest existing tool, `scaffold_report`), `run`'s
//! qa-gate-before-render composite (mapped instead to the plain render
//! tool, `generate_document`, with the render/qa ordering left to the
//! caller), and `preview --png` (no raster pipeline exists; `preview` is
//! PDF-only via `render_typst_report`). Raised on the issue rather than
//! built as new engine logic.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use clap::Subcommand;
use snafu::prelude::*;

use organon::registry::ToolRegistry;
use organon::types::{ToolContext, ToolInput, ToolResult, ToolResultBlock, ToolResultContent};

use crate::error::Result;

/// `generate_document` output format.
#[derive(Debug, Clone, clap::ValueEnum)]
pub(crate) enum DocumentFormat {
    Odt,
    Docx,
    Xlsx,
    Pdf,
    Html,
    Md,
    Latex,
    Epub,
}

impl DocumentFormat {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Odt => "odt",
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pdf => "pdf",
            Self::Html => "html",
            Self::Md => "md",
            Self::Latex => "latex",
            Self::Epub => "epub",
        }
    }
}

/// `scaffold_report` output format.
#[derive(Debug, Clone, clap::ValueEnum)]
pub(crate) enum ScaffoldFormat {
    Typst,
    Xlsx,
    Both,
}

impl ScaffoldFormat {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Typst => "typst",
            Self::Xlsx => "xlsx",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Scaffold a new report project skeleton from embedded templates.
    Create {
        /// Project slug / short name (used for filenames).
        #[arg(long)]
        slug: String,
        /// Project description.
        #[arg(long, default_value = "")]
        description: String,
        /// Output format: typst, xlsx, or both.
        #[arg(long, value_enum, default_value = "typst")]
        format: ScaffoldFormat,
        /// Inject CONFIDENTIAL headers/footers.
        #[arg(long)]
        confidential: bool,
        /// Directory to write the scaffolded files to. Omit to print a
        /// JSON manifest (path + base64 contents) to stdout instead.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Enumerate the shipped component packs (`title`, `bullet`, `stat`, `chart`, ...).
    #[command(visible_alias = "list-components")]
    List {
        /// Emit JSON instead of one id per line.
        #[arg(long)]
        json: bool,
    },
    /// Show one component's schema, defaults, and theme tokens.
    #[command(visible_alias = "get-component")]
    Get {
        /// Component id, e.g. `title`, `bullet`, `stat`, `chart`.
        id: String,
    },
    /// Render a fast Typst-driven PDF preview.
    Preview {
        /// Built-in Typst template slug. Ignored when `--source` is given.
        #[arg(long, conflicts_with = "source")]
        template: Option<String>,
        /// Path to inline Typst source. Mutually exclusive with `--template`.
        #[arg(long)]
        source: Option<PathBuf>,
        /// Path to a JSON file exposed to the template as `data.json`.
        #[arg(long)]
        data: Option<PathBuf>,
        /// Theme identifier. Defaults to the built-in `protos` theme.
        #[arg(long)]
        theme: Option<String>,
        /// Where to write the rendered PDF. Defaults to `./poiesis-preview.pdf`.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Run the composite QA gate: prose lint plus optional factbase validation.
    /// Exits non-zero when the returned `QaReport.has_issues` is true.
    Qa {
        /// Path to the prose to check: plain text, or JSON whose leaf
        /// strings are walked.
        #[arg(long)]
        prose: PathBuf,
        /// Path to a JSON factbase to validate citations against.
        #[arg(long)]
        factbase: Option<PathBuf>,
    },
    /// Check report prose quality: banned words, citation coverage,
    /// structure. Exits non-zero when any findings are returned.
    Lint {
        /// Path to the prose to lint.
        #[arg(long)]
        prose: PathBuf,
        /// Emit findings as a JSON array instead of human-readable lines.
        #[arg(long)]
        json: bool,
    },
    /// Validate numeric claims in a verify manifest against derived and
    /// reference sources. Exits non-zero when any claim fails.
    Verify {
        /// Path to a verify-manifest JSON file to validate.
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Render a document descriptor to a deliverable format.
    Run {
        /// Output format.
        #[arg(long, value_enum)]
        format: DocumentFormat,
        /// Document title.
        #[arg(long, default_value = "Untitled Document")]
        title: String,
        /// Document author.
        #[arg(long)]
        author: Option<String>,
        /// Path to a JSON file holding the block content: an array of
        /// `{"type": ...}` block objects (`heading`, `paragraph`,
        /// `page_break`).
        #[arg(long)]
        content: PathBuf,
        /// Where to write the rendered bytes. Defaults to
        /// `./poiesis-run.<format>`.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

pub(crate) async fn run(action: Action) -> Result<()> {
    match action {
        Action::Create {
            slug,
            description,
            format,
            confidential,
            dir,
        } => create(&slug, &description, &format, confidential, dir.as_deref()).await,
        Action::List { json } => list_components(json),
        Action::Get { id } => get_component(&id),
        Action::Preview {
            template,
            source,
            data,
            theme,
            out,
        } => {
            preview(
                template.as_deref(),
                source.as_deref(),
                data.as_deref(),
                theme.as_deref(),
                out.as_ref(),
            )
            .await
        }
        Action::Qa { prose, factbase } => qa(&prose, factbase.as_deref()).await,
        Action::Lint { prose, json } => lint(&prose, json).await,
        Action::Verify { manifest } => verify(&manifest).await,
        Action::Run {
            format,
            title,
            author,
            content,
            out,
        } => render_document(&format, &title, author.as_deref(), &content, out.as_ref()).await,
    }
}

// ── organon tool plumbing ─────────────────────────────────────────────────────

/// Build a fresh registry carrying every organon built-in, including the
/// poiesis family (`organon`'s `poiesis` Cargo feature is unconditional for
/// this binary; see `crates/aletheia/Cargo.toml`).
fn build_registry() -> Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    organon::builtins::register_all(&mut registry)
        .whatever_context("failed to register organon built-in tools")?;
    Ok(registry)
}

/// A minimal, offline `ToolContext` for a single CLI-invoked tool call --
/// no running agent turn, session, or server behind it. Mirrors the
/// `http_test_ctx()` helper in `external_tools.rs`, which documents the
/// same minimal-construction shape for calling an organon executor directly.
fn build_context() -> Result<ToolContext> {
    let cwd = std::env::current_dir().whatever_context("failed to resolve current directory")?;
    Ok(ToolContext {
        nous_id: koina::id::NousId::from_static(koina::defaults::DEFAULT_AGENT_ID),
        session_id: koina::id::SessionId::new(),
        turn_identity: koina::turn_identity::TurnEventIdentity {
            turn_id: koina::ulid::Ulid::new(),
            session_id: "cli".to_owned(),
            request_id: None,
            turn_number: 0,
            client_turn_id: None,
        },
        receipt_signer: organon::receipts::ReceiptSigner::new_session(),
        workspace: cwd.clone(),
        allowed_roots: vec![cwd],
        services: None,
        active_tools: Arc::new(RwLock::new(std::collections::HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    })
}

/// Call one organon tool by name with the given (already-inline, never
/// path-shaped) arguments. Every poiesis tool this module calls accepts its
/// file-like inputs as inline strings (`text`, `manifest`, `prose`,
/// `source`, `data`, `factbase_json`, `content`, `slug`/`description`)
/// rather than `path` fields, so the CLI reads files itself and never has
/// to satisfy the registry's agent-sandbox path/`allowed_roots` validation.
async fn call_tool(name: &'static str, arguments: serde_json::Value) -> Result<ToolResult> {
    let registry = build_registry()?;
    let ctx = build_context()?;
    let input = ToolInput {
        name: koina::id::ToolName::from_static(name),
        tool_use_id: "cli".to_owned(),
        arguments,
    };
    registry
        .execute(&input, &ctx)
        .await
        .with_whatever_context(|_| format!("{name} execution failed"))
}

/// Print a tool result's text, write out any returned document bytes to
/// `out` (or `default_out` when absent), and fail loud if the tool itself
/// reported failure.
async fn emit_result(result: &ToolResult, out: Option<&PathBuf>, default_out: &str) -> Result<()> {
    match &result.content {
        ToolResultContent::Text(text) => println!("{text}"),
        ToolResultContent::Blocks(blocks) => {
            for block in blocks {
                match block {
                    ToolResultBlock::Text { text } => println!("{text}"),
                    ToolResultBlock::Document { source } => {
                        let bytes = koina::base64::decode(&source.data)
                            .whatever_context("failed to decode returned document bytes")?;
                        let path = out.cloned().unwrap_or_else(|| PathBuf::from(default_out));
                        tokio::fs::write(&path, &bytes)
                            .await
                            .with_whatever_context(|_| {
                                format!("failed to write {}", path.display())
                            })?;
                        println!("wrote {} bytes to {}", bytes.len(), path.display());
                    }
                    // WARNING: no poiesis tool this module calls returns an
                    // image block today; fail loud rather than silently
                    // drop content the tool actually returned.
                    ToolResultBlock::Image { .. } => {
                        whatever!(
                            "poiesis tool returned an unexpected image content block: {block:?}"
                        );
                    }
                    // WARNING: `ToolResultBlock` is `#[non_exhaustive]`; this
                    // arm covers any future variant. Fail loud instead of
                    // silently dropping it.
                    _ => {
                        whatever!("poiesis tool returned an unexpected content block: {block:?}");
                    }
                }
            }
        }
        // WARNING: `ToolResultContent` is `#[non_exhaustive]`; every current
        // organon tool returns `Text` or `Blocks`. Fail loud instead of
        // silently dropping a future variant's content.
        _ => {
            whatever!(
                "poiesis tool returned unexpected result content: {:?}",
                result.content
            );
        }
    }
    if result.is_error {
        whatever!("poiesis tool reported failure (see output above)");
    }
    Ok(())
}

/// Extract the plain-text payload from a tool result whose content is
/// always `ToolResultContent::Text` -- `qa_gate` and `lint_report` (called
/// with `json: true`) never return `Blocks` or a document.
fn text_payload(result: &ToolResult) -> Result<&str> {
    match &result.content {
        ToolResultContent::Text(text) => Ok(text),
        other => whatever!("expected a text tool result, got {other:?}"),
    }
}

// ── create (scaffold_report) ───────────────────────────────────────────────────

/// One scaffolded file as returned by `scaffold_report`'s JSON manifest
/// mode: `path` (relative) plus base64-encoded `contents`.
#[derive(serde::Deserialize)]
struct ScaffoldFile {
    path: String,
    contents_base64: String,
}

async fn create(
    slug: &str,
    description: &str,
    format: &ScaffoldFormat,
    confidential: bool,
    dir: Option<&Path>,
) -> Result<()> {
    // WHY: `directory` is deliberately never sent to the tool (see the
    // `call_tool` doc comment) -- the tool always returns the JSON file
    // manifest, and this CLI writes the files itself when `--dir` is given.
    let args = serde_json::json!({
        "slug": slug,
        "description": description,
        "format": format.as_str(),
        "confidential": confidential,
    });
    let result = call_tool("scaffold_report", args).await?;
    let text = text_payload(&result)?;
    if result.is_error {
        println!("{text}");
        whatever!("poiesis tool reported failure (see output above)");
    }

    let Some(dir) = dir else {
        println!("{text}");
        return Ok(());
    };

    let manifest: Vec<ScaffoldFile> = serde_json::from_str(text)
        .whatever_context("failed to parse scaffold_report output as a file manifest")?;
    for file in &manifest {
        let bytes = koina::base64::decode(&file.contents_base64)
            .whatever_context("failed to decode scaffolded file contents")?;
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_whatever_context(|_| {
                    format!("failed to create directory {}", parent.display())
                })?;
        }
        tokio::fs::write(&path, &bytes)
            .await
            .with_whatever_context(|_| format!("failed to write {}", path.display()))?;
    }
    println!("wrote {} file(s) to {}", manifest.len(), dir.display());
    Ok(())
}

// ── list / get (poiesis-core, no organon tool) ────────────────────────────────

/// Materialize the embedded component packs into a scratch directory and
/// discover them into a registry. See `poiesis_core::embedded::extract_to`:
/// the returned registry's `schema`/`defaults` are fully parsed into owned
/// values at discovery time, so the scratch directory can be dropped
/// immediately after this call returns.
fn load_component_registry() -> Result<poiesis_core::ComponentRegistry> {
    let scratch = tempfile::tempdir()
        .whatever_context("failed to create a scratch directory for component packs")?;
    poiesis_core::embedded::extract_to(scratch.path())
        .whatever_context("failed to load the embedded component packs")
}

fn list_components(json: bool) -> Result<()> {
    let registry = load_component_registry()?;
    let ids: Vec<String> = registry
        .list_components()
        .into_iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    if json {
        let payload = serde_json::json!({ "components": ids });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .whatever_context("failed to serialize component list")?
        );
    } else {
        for id in &ids {
            println!("{id}");
        }
    }
    Ok(())
}

fn get_component(id: &str) -> Result<()> {
    let component_id = poiesis_core::ComponentId::new(id)
        .with_whatever_context(|_| format!("invalid component id {id:?}"))?;
    let registry = load_component_registry()?;
    let def = registry.get(&component_id).with_whatever_context(|| {
        format!("unknown component {id:?}; run `aletheia poiesis list` for the shipped set")
    })?;
    let payload = serde_json::json!({
        "id": def.id.as_str(),
        "schema": def.schema,
        "defaults": def.defaults,
        "tokens": def.tokens,
        "html_template": def.html.display().to_string(),
        "ooxml_recipe": def.ooxml.display().to_string(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).whatever_context("failed to serialize component")?
    );
    Ok(())
}

// ── preview ───────────────────────────────────────────────────────────────────

async fn preview(
    template: Option<&str>,
    source: Option<&Path>,
    data: Option<&Path>,
    theme: Option<&str>,
    out: Option<&PathBuf>,
) -> Result<()> {
    let data_value: serde_json::Value = if let Some(path) = data {
        let raw = std::fs::read_to_string(path)
            .with_whatever_context(|_| format!("failed to read --data {}", path.display()))?;
        serde_json::from_str(&raw).whatever_context("--data must be valid JSON")?
    } else {
        serde_json::json!({})
    };

    let mut args = serde_json::Map::new();
    args.insert("data".to_owned(), data_value);
    if let Some(theme) = theme {
        args.insert(
            "theme".to_owned(),
            serde_json::Value::String(theme.to_owned()),
        );
    }
    if let Some(source_path) = source {
        let source_text = std::fs::read_to_string(source_path).with_whatever_context(|_| {
            format!("failed to read --source {}", source_path.display())
        })?;
        args.insert("source".to_owned(), serde_json::Value::String(source_text));
    } else {
        let slug = template.unwrap_or("default");
        args.insert(
            "template".to_owned(),
            serde_json::Value::String(slug.to_owned()),
        );
    }

    let result = call_tool("render_typst_report", serde_json::Value::Object(args)).await?;
    emit_result(&result, out, "poiesis-preview.pdf").await
}

// ── qa ────────────────────────────────────────────────────────────────────────

async fn qa(prose: &Path, factbase: Option<&Path>) -> Result<()> {
    let prose_text = std::fs::read_to_string(prose)
        .with_whatever_context(|_| format!("failed to read --prose {}", prose.display()))?;

    let mut args = serde_json::Map::new();
    args.insert("prose".to_owned(), serde_json::Value::String(prose_text));
    if let Some(fb_path) = factbase {
        let fb_text = std::fs::read_to_string(fb_path).with_whatever_context(|_| {
            format!("failed to read --factbase {}", fb_path.display())
        })?;
        args.insert(
            "factbase_json".to_owned(),
            serde_json::Value::String(fb_text),
        );
    }

    let result = call_tool("qa_gate", serde_json::Value::Object(args)).await?;
    let text = text_payload(&result)?;
    println!("{text}");
    if result.is_error {
        whatever!("poiesis tool reported failure (see output above)");
    }

    // INVARIANT (B-010 §2.3 / §1.5): `qa` exits non-zero iff the report
    // has issues -- the agent/CI caller reads `has_issues`, never prose,
    // to learn whether the gate passed.
    let report: poiesis_core::QaReport = serde_json::from_str(text)
        .whatever_context("failed to parse qa_gate output as a QaReport")?;
    if report.has_issues {
        whatever!(
            "poiesis qa found {} issue(s); see the QaReport above",
            report.issue_count
        );
    }
    Ok(())
}

// ── lint ──────────────────────────────────────────────────────────────────────

/// One lint finding as returned by `lint_report`'s JSON mode. Only the
/// fields this CLI needs (location + message) are parsed; `kind` and the
/// optional `fix` payload are left to whatever prints the raw JSON.
#[derive(serde::Deserialize)]
struct LintFindingView {
    line_start: usize,
    line_end: usize,
    message: String,
}

async fn lint(prose: &Path, json: bool) -> Result<()> {
    let text_input = std::fs::read_to_string(prose)
        .with_whatever_context(|_| format!("failed to read --prose {}", prose.display()))?;

    // WHY: always request JSON from the tool, regardless of `--json`, so
    // this CLI can parse the finding count to decide the exit code; the
    // human-readable rendering below is reconstructed from the same parse.
    let args = serde_json::json!({ "text": text_input, "json": true });
    let result = call_tool("lint_report", args).await?;
    let text = text_payload(&result)?;
    if result.is_error {
        println!("{text}");
        whatever!("poiesis tool reported failure (see output above)");
    }

    let findings: Vec<LintFindingView> = serde_json::from_str(text)
        .whatever_context("failed to parse lint_report output as a findings array")?;

    if json {
        println!("{text}");
    } else if findings.is_empty() {
        println!("LINT: no findings");
    } else {
        for f in &findings {
            if f.line_start == f.line_end {
                println!("LINT: line {}: {}", f.line_start, f.message);
            } else {
                println!("LINT: lines {}-{}: {}", f.line_start, f.line_end, f.message);
            }
        }
    }

    if !findings.is_empty() {
        whatever!("poiesis lint found {} finding(s)", findings.len());
    }
    Ok(())
}

// ── verify ────────────────────────────────────────────────────────────────────

async fn verify(manifest: &Path) -> Result<()> {
    let text = std::fs::read_to_string(manifest)
        .with_whatever_context(|_| format!("failed to read --manifest {}", manifest.display()))?;
    let args = serde_json::json!({ "manifest": text });
    let result = call_tool("verify_report", args).await?;
    // WHY: unlike `qa_gate`/`lint_report`, `verify_report` already signals
    // a failed or under-populated manifest via `ToolResult::error` (see
    // `crates/organon/src/builtins/poiesis.rs` `VerifyReportExecutor`), so
    // `emit_result`'s existing `is_error` check gates the exit code
    // correctly without any extra parsing here.
    emit_result(&result, None, "poiesis-verify.json").await
}

// ── run (generate_document render) ─────────────────────────────────────────────

async fn render_document(
    format: &DocumentFormat,
    title: &str,
    author: Option<&str>,
    content: &PathBuf,
    out: Option<&PathBuf>,
) -> Result<()> {
    let content_text = std::fs::read_to_string(content)
        .with_whatever_context(|_| format!("failed to read --content {}", content.display()))?;
    serde_json::from_str::<serde_json::Value>(&content_text)
        .whatever_context("--content must be a JSON array of block objects")?;

    let args = build_document_args(title, author, format.as_str(), content_text);
    let result = call_tool("generate_document", serde_json::Value::Object(args)).await?;
    let default_out = format!("poiesis-run.{}", format.as_str());
    emit_result(&result, out, &default_out).await
}

/// Build the `generate_document` argument map. `author` is omitted from the
/// map entirely when absent -- `InputSchema::validate` rejects a present
/// `null` for a `String`-typed property (`author` is optional, not
/// nullable), so a bare `"author": author` with `author: None` would fail
/// registry validation on every call that omits `--author`.
fn build_document_args(
    title: &str,
    author: Option<&str>,
    format: &str,
    content_text: String,
) -> serde_json::Map<String, serde_json::Value> {
    let mut args = serde_json::Map::new();
    args.insert(
        "title".to_owned(),
        serde_json::Value::String(title.to_owned()),
    );
    if let Some(author) = author {
        args.insert(
            "author".to_owned(),
            serde_json::Value::String(author.to_owned()),
        );
    }
    args.insert(
        "format".to_owned(),
        serde_json::Value::String(format.to_owned()),
    );
    args.insert(
        "content".to_owned(),
        serde_json::Value::String(content_text),
    );
    args
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn list_components_includes_the_shipped_packs() {
        let registry = load_component_registry().expect("embedded packs must load");
        let ids: Vec<String> = registry
            .list_components()
            .into_iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        for expected in ["title", "bullet", "stat", "chart", "blank"] {
            assert!(
                ids.iter().any(|id| id == expected),
                "expected component {expected:?} in {ids:?}"
            );
        }
    }

    #[test]
    fn get_component_returns_schema_for_known_id() {
        get_component("title").expect("known component id must resolve");
    }

    #[test]
    fn get_component_fails_loud_for_unknown_id() {
        let err = get_component("no-such-component").expect_err("unknown id must error");
        assert!(
            err.to_string().contains("unknown component"),
            "error should name the problem: {err}"
        );
    }

    #[test]
    fn document_args_omit_author_key_when_author_is_absent() {
        let args = build_document_args("Untitled Document", None, "pdf", "[]".to_owned());
        assert!(
            !args.contains_key("author"),
            "author key must be absent, not null, when --author is omitted: {args:?}"
        );
    }

    #[test]
    fn document_args_include_author_key_when_author_is_present() {
        let args = build_document_args(
            "Untitled Document",
            Some("Ada Lovelace"),
            "pdf",
            "[]".to_owned(),
        );
        assert_eq!(
            args.get("author"),
            Some(&serde_json::Value::String("Ada Lovelace".to_owned())),
        );
    }

    #[tokio::test]
    async fn emit_result_returns_err_when_tool_reports_failure() {
        let result = ToolResult::error("synthetic failure".to_owned());
        let err = emit_result(&result, None, "unused.bin")
            .await
            .expect_err("emit_result must fail when the tool result reports an error");
        assert!(
            err.to_string().contains("reported failure"),
            "error should explain the tool failed: {err}"
        );
    }
}
