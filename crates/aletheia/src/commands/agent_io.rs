// kanon:ignore RUST/file-too-long — module contains tightly-coupled agent I/O CLI command implementations; splitting would hurt cohesion
//! Agent import/export and skill management commands.

use std::collections::HashMap;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use crate::error::Result;

fn validate_nous_id(nous_id: &str) -> Result<()> {
    if nous_id.trim().is_empty() {
        whatever!("--nous-id must not be empty");
    }
    validate_agent_id_for_paths(nous_id, "--nous-id")?;
    Ok(())
}

/// Validate an agent ID that will be used to derive on-disk paths.
///
/// The ID is consumed as a directory name (`nous/<id>`) and embedded in
/// config — any traversal segment, separator, or NUL byte makes the
/// import able to write outside the instance root. Matches the rules
/// `add-nous` enforces on freshly created agents.
fn validate_agent_id_for_paths(id: &str, source: &str) -> Result<()> {
    if id.is_empty() {
        whatever!("{source} must not be empty");
    }
    if id.contains('\0') {
        whatever!("{source} must not contain NUL bytes");
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        whatever!("{source} must contain only alphanumeric characters and hyphens (got {id:?})");
    }
    if id.starts_with('-') || id.ends_with('-') {
        whatever!("{source} must not start or end with a hyphen (got {id:?})");
    }
    Ok(())
}

fn validate_target_id(target_id: Option<&str>) -> Result<()> {
    if let Some(id) = target_id {
        if id.trim().is_empty() {
            whatever!("--target-id must not be empty");
        }
        validate_agent_id_for_paths(id, "--target-id")?;
    }
    Ok(())
}

/// Validate a workspace-relative file path supplied by an imported
/// `.agent.json`. The path is joined to the nous directory; any
/// absolute, traversal, or NUL-containing path could escape the
/// instance root and write arbitrary files.
fn validate_workspace_relative_path(path: &str) -> Result<()> {
    if path.is_empty() {
        whatever!("workspace file path must not be empty");
    }
    if path.contains('\0') {
        whatever!("workspace file path must not contain NUL bytes (got {path:?})");
    }
    let p = Path::new(path);
    if p.is_absolute() {
        whatever!("workspace file path must be relative (got {path:?})");
    }
    for component in p.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                whatever!("workspace file path must not contain '..' segments (got {path:?})");
            }
            Component::RootDir | Component::Prefix(_) => {
                whatever!("workspace file path must be relative (got {path:?})");
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ExportArgs {
    /// Agent (nous) ID to export
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
    pub nous_id: String,
    /// Output file path (default: `{nous-id}-{date}.agent.json`)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Include archived/distilled sessions
    #[arg(long)]
    pub archived: bool,
    /// Max messages per session (0 = all)
    #[arg(long, default_value_t = 500)]
    pub max_messages: usize,
    /// Compact JSON (no pretty printing)
    #[arg(long)]
    pub compact: bool,
    /// Overwrite existing output file without prompting
    #[arg(long)]
    pub force: bool,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI flags — each bool is a distinct switch"
)]
#[derive(Debug, Clone, Args)]
pub(crate) struct ImportArgs {
    /// Path to .agent.json file
    pub file: PathBuf,
    /// Override the target agent ID
    #[arg(long)]
    pub target_id: Option<String>,
    /// Skip importing session history
    #[arg(long)]
    pub skip_sessions: bool,
    /// Skip restoring workspace files
    #[arg(long)]
    pub skip_workspace: bool,
    /// Overwrite existing workspace files
    #[arg(long)]
    pub force: bool,
    /// Show what would be imported without making changes
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct SeedSkillsArgs {
    /// Directory containing skill subdirectories (each with SKILL.md)
    #[arg(short, long)]
    pub dir: PathBuf,
    /// Agent (nous) ID to attribute skills to
    #[arg(short, long)]
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
    pub nous_id: String,
    /// Overwrite existing skills with the same name
    #[arg(long)]
    pub force: bool,
    /// Show what would be seeded without writing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ExportSkillsArgs {
    /// Agent (nous) ID whose skills to export
    #[arg(short, long)]
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
    pub nous_id: String,
    /// Output directory (default: .claude/skills)
    #[arg(short, long, default_value = ".claude/skills")]
    pub output: PathBuf,
    /// Filter by domain tags (comma-separated)
    #[arg(short, long)]
    pub domain: Option<String>,
    /// Server URL for lock detection
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub url: String,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ReviewSkillsArgs {
    /// Agent (nous) ID whose pending skills to review
    #[arg(short, long)]
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
    pub nous_id: String,
    /// Action: list, approve, reject
    #[arg(short, long, default_value = "list")]
    pub action: String,
    /// Fact ID of the pending skill (required for approve/reject)
    #[arg(short, long)]
    pub fact_id: Option<String>,
    /// Server URL for lock detection
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub url: String,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct MigrateMemoryArgs {
    /// Qdrant server URL
    #[arg(long, default_value = "http://localhost:6333", env = "QDRANT_URL")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub qdrant_url: String,
    /// Qdrant collection name
    #[arg(long, default_value = "aletheia_memories")]
    pub collection: String,
    /// Path to persistent knowledge store (fjall)
    #[arg(long, env = "ALETHEIA_KNOWLEDGE_PATH")]
    pub knowledge_path: Option<PathBuf>,
    /// Write flagged facts to a review file
    #[arg(long)]
    pub review_file: Option<PathBuf>,
    /// Report only, don't insert
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct TuiArgs {
    /// Gateway URL
    #[arg(short, long, env = "ALETHEIA_URL")]
    pub url: Option<String>,
    /// Bearer token for authentication
    #[arg(short, long, env = "ALETHEIA_TOKEN")]
    pub token: Option<String>,
    /// Agent to focus on startup
    #[arg(short, long)]
    pub agent: Option<String>,
    /// Session to open
    #[arg(short, long)]
    pub session: Option<String>,
    /// Clear saved credentials
    #[arg(long)]
    pub logout: bool,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct InitArgs {
    /// Instance root directory (default in interactive/-y mode: ./instance).
    /// Also reads `ALETHEIA_ROOT` as a fallback env var
    #[arg(
        short = 'r',
        long,
        visible_alias = "instance-path",
        env = "ALETHEIA_INSTANCE_PATH"
    )]
    pub instance_root: Option<PathBuf>,
    /// Accept all defaults without prompts
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// Run without interactive prompts; --instance-path is required
    #[arg(long)]
    pub non_interactive: bool,
    /// Anthropic API key. Sets credential source to 'api-key'.
    /// Omit to use 'auto' resolution (api-key -> env -> claude-code)
    #[arg(long, env = "ANTHROPIC_API_KEY")]
    pub api_key: Option<String>,
    /// Authentication mode: none, token (default: none)
    #[arg(long, env = "ALETHEIA_AUTH_MODE")]
    pub auth_mode: Option<String>,
    /// LLM API provider (default: anthropic)
    #[arg(long, env = "ALETHEIA_API_PROVIDER")]
    pub api_provider: Option<String>,
    /// Model identifier (default: claude-sonnet-4-6)
    #[arg(long, env = "ALETHEIA_MODEL")]
    pub model: Option<String>,
}

use taxis::oikos::Oikos;

use mneme::types::{Role, SessionMetrics, SessionOrigin, SessionStatus, SessionType};

#[cfg(feature = "recall")]
fn knowledge_path_for_nous(oikos: &Oikos, nous_id: &str) -> PathBuf {
    let cohort = taxis::loader::load_config(oikos).ok().map_or_else(
        || std::sync::Arc::from("shared"),
        |config| taxis::config::resolve_nous(&config, nous_id).episteme_cohort,
    );
    oikos.knowledge_cohort_db(cohort.as_ref())
}

/// Enumerate the agent's typed knowledge from the live store.
///
/// Returns `None` when the `recall` feature is disabled or when the
/// knowledge store on disk is empty/unavailable — the export still succeeds
/// without typed knowledge, the slot just stays unset.
#[cfg(feature = "recall")]
fn export_knowledge(
    oikos: &Oikos,
    nous_id: &str,
) -> Result<Option<mneme::portability::KnowledgeExport>> {
    use mneme::knowledge_store::KnowledgeStore;
    use mneme::portability::KnowledgeExport;

    let knowledge_path = knowledge_path_for_nous(oikos, nous_id);
    if !knowledge_path.exists() {
        return Ok(None);
    }

    let config = mneme::knowledge_store::KnowledgeConfig::default();
    let Ok(store) = KnowledgeStore::open_fjall(&knowledge_path, config) else {
        return Ok(None);
    };

    let facts: Vec<mneme::knowledge::Fact> = store
        .list_all_facts(i64::MAX)
        .with_whatever_context(|_| format!("failed to list facts for '{nous_id}'"))?
        .into_iter()
        .filter(|f| f.nous_id == nous_id)
        .collect();

    // Entities and relationships are workspace-global today (no nous_id
    // column). We round-trip the full set so a faithful restore reproduces
    // the source's graph view.
    let entities = store
        .list_entities()
        .with_whatever_context(|_| format!("failed to list entities for '{nous_id}'"))?;
    let relationships = store
        .list_all_relationships()
        .with_whatever_context(|_| format!("failed to list relationships for '{nous_id}'"))?;

    if facts.is_empty() && entities.is_empty() && relationships.is_empty() {
        return Ok(None);
    }

    Ok(Some(KnowledgeExport {
        facts,
        entities,
        relationships,
    }))
}

#[cfg(not(feature = "recall"))]
fn export_knowledge(
    _oikos: &Oikos,
    _nous_id: &str,
) -> Result<Option<mneme::portability::KnowledgeExport>> {
    Ok(None)
}

/// Hydrate typed knowledge into the live store.
#[cfg(feature = "recall")]
fn import_knowledge(
    oikos: &Oikos,
    nous_id: &str,
    knowledge: &mneme::portability::KnowledgeExport,
) -> Result<()> {
    use mneme::embedding::{EmbeddingConfig, create_provider};
    use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

    let loaded_config = match taxis::loader::load_config(oikos) {
        Ok(config) => Some(config),
        Err(err) => {
            tracing::warn!(
                nous_id,
                error = %err,
                "failed to load instance config; imported fact vectors will be skipped"
            );
            None
        }
    };

    let knowledge_config = loaded_config
        .as_ref()
        .map_or_else(KnowledgeConfig::default, |config| KnowledgeConfig {
            dim: config.embedding.dimension,
            ..Default::default()
        });

    let embedding_provider = loaded_config.as_ref().and_then(|config| {
        let embedding_config = EmbeddingConfig {
            provider: config.embedding.provider.clone(),
            model: config.embedding.model.clone(),
            dimension: Some(config.embedding.dimension),
            api_key: None,
            base_url: None,
        };
        match create_provider(&embedding_config) {
            Ok(provider) => Some(provider),
            Err(err) => {
                tracing::warn!(
                    nous_id,
                    provider = %config.embedding.provider,
                    error = %err,
                    "embedding provider unavailable; imported fact vectors will be skipped"
                );
                None
            }
        }
    });

    let knowledge_path = knowledge_path_for_nous(oikos, nous_id);
    let parent = knowledge_path
        .parent()
        .ok_or_else(|| crate::error::Error::msg("knowledge path has no parent directory"))?;
    std::fs::create_dir_all(parent)
        .with_whatever_context(|_| format!("failed to create knowledge dir for {nous_id}"))?;
    let store = KnowledgeStore::open_fjall(&knowledge_path, knowledge_config)
        .with_whatever_context(|_| format!("failed to open knowledge store for {nous_id}"))?;

    for fact in &knowledge.facts {
        store
            .insert_fact(fact)
            .with_whatever_context(|_| format!("import fact {:?}", fact.id))?;
    }
    for entity in &knowledge.entities {
        store
            .insert_entity(entity)
            .with_whatever_context(|_| format!("import entity {:?}", entity.id))?;
    }
    for rel in &knowledge.relationships {
        store
            .insert_relationship(rel)
            .with_whatever_context(|_| format!("import rel {:?} -> {:?}", rel.src, rel.dst))?;
    }

    if let Some(provider) = embedding_provider.as_ref() {
        let inserted = store.backfill_fact_embeddings(&knowledge.facts, provider.as_ref());
        tracing::info!(
            nous_id,
            fact_count = knowledge.facts.len(),
            inserted,
            "imported fact vector backfill complete"
        );
    } else if !knowledge.facts.is_empty() {
        tracing::info!(
            nous_id,
            fact_count = knowledge.facts.len(),
            "imported facts restored without vector embeddings"
        );
    }
    Ok(())
}

#[cfg(not(feature = "recall"))]
fn import_knowledge(
    _oikos: &Oikos,
    _nous_id: &str,
    _knowledge: &mneme::portability::KnowledgeExport,
) -> Result<()> {
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "agent export assembles config, workspace, sessions, messages, and notes into one portability file"
)]
pub(crate) fn export_agent(instance_root: Option<&PathBuf>, args: &ExportArgs) -> Result<()> {
    use mneme::portability::{AgentFile, ExportedMessage, ExportedNote, ExportedSession, NousInfo};

    let oikos = super::resolve_oikos(instance_root)?;
    let config =
        taxis::loader::load_config(&oikos).whatever_context("failed to load aletheia config")?;
    let resolved = taxis::config::resolve_nous(&config, &args.nous_id);

    if !config
        .agents
        .list
        .iter()
        .any(|agent| agent.id == args.nous_id)
    {
        whatever!("nous agent '{}' not found in configuration", args.nous_id);
    }

    let workspace_root = resolve_workspace_path(&oikos, &resolved.workspace);
    let workspace = export_workspace(&workspace_root).with_whatever_context(|_| {
        format!(
            "failed to export workspace for '{}' at {}",
            args.nous_id,
            workspace_root.display()
        )
    })?;

    let sessions_db = oikos.sessions_db();
    let store = mneme::store::SessionStore::open(&sessions_db).with_whatever_context(|_| {
        format!("failed to open session store at {}", sessions_db.display())
    })?;

    let limit = if args.max_messages == 0 {
        None
    } else {
        Some(i64::try_from(args.max_messages).unwrap_or(i64::MAX))
    };
    let mut sessions = Vec::new();
    for session in store
        .list_sessions(Some(&args.nous_id))
        .whatever_context("failed to list sessions")?
    {
        if !args.archived && session.status != SessionStatus::Active {
            continue;
        }

        // #4163/A — `get_history` filters `is_distilled == true`, dropping the
        // distilled tail. The portability raw entry point returns every row in
        // seq order so an export-then-import round-trip stays faithful.
        let messages = store
            .get_history_raw(&session.id, limit)
            .with_whatever_context(|_| format!("failed to read history for {}", session.id))?
            .into_iter()
            .map(|msg| ExportedMessage {
                role: msg.role.to_string(),
                content: msg.content,
                seq: msg.seq,
                token_estimate: msg.token_estimate,
                is_distilled: msg.is_distilled,
                created_at: msg.created_at,
            })
            .collect();

        let notes = store
            .get_notes(&session.id)
            .with_whatever_context(|_| format!("failed to read notes for {}", session.id))?
            .into_iter()
            .map(|note| ExportedNote {
                category: note.category,
                content: note.content,
                created_at: note.created_at,
            })
            .collect();

        // #4163/B — populate working_state from the live blackboard. The key
        // convention `ws:{nous_id}:{session_id}` mirrors
        // `nous::working_state::WorkingState::persist_key`. `distillation_priming`
        // has a schema slot for forward compatibility but no live producer
        // today; left `None` until that store materialises.
        let ws_key = format!("ws:{}:{}", args.nous_id, session.id);
        let working_state = match store
            .blackboard_read(&ws_key)
            .with_whatever_context(|_| format!("failed to read working_state for {}", session.id))?
        {
            Some(row) => serde_json::from_str::<serde_json::Value>(&row.value).ok(),
            None => None,
        };

        sessions.push(ExportedSession {
            id: session.id,
            session_key: session.session_key,
            status: session.status.to_string(),
            session_type: session.session_type.to_string(),
            message_count: session.metrics.message_count,
            token_count_estimate: session.metrics.token_count_estimate,
            distillation_count: session.metrics.distillation_count,
            created_at: session.created_at,
            updated_at: session.updated_at,
            working_state,
            distillation_priming: None,
            notes,
            messages,
        });
    }

    // #4163/B — populate top-level knowledge from the live store (typed
    // Facts/Entities/Relationships). Vectors + opaque graph (`memory`) are
    // still gapped; tracked as the v2 known-gap so PR3/PR4 / a follow-up can
    // close them without another version bump.
    let knowledge = export_knowledge(&oikos, &args.nous_id)?;

    let exported_at = jiff::Timestamp::now().to_string();
    let agent_file = AgentFile {
        version: mneme::portability::AGENT_FILE_VERSION,
        exported_at: exported_at.clone(),
        generator: format!("aletheia-rust/{}", env!("CARGO_PKG_VERSION")),
        nous: NousInfo {
            id: args.nous_id.clone(),
            name: resolved.name.clone(),
            model: Some(resolved.model.primary.to_string()),
            config: serde_json::json!({
                "workspace": resolved.workspace,
                "domains": resolved.domains,
                "epistemeCohort": resolved.episteme_cohort.to_string(),
                "private": resolved.private,
            }),
        },
        workspace,
        sessions,
        memory: None,
        knowledge,
    };

    let output = args
        .output
        .clone()
        .unwrap_or_else(|| default_export_path(&args.nous_id, &exported_at));
    if output.exists() && !args.force {
        whatever!(
            "output file already exists: {}\nUse --force to overwrite.",
            output.display()
        );
    }
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_whatever_context(|_| format!("failed to create {}", parent.display()))?;
    }

    let json = if args.compact {
        serde_json::to_string(&agent_file).whatever_context("failed to serialize agent file")?
    } else {
        serde_json::to_string_pretty(&agent_file)
            .whatever_context("failed to serialize agent file")?
    };
    koina::fs::write_restricted(&output, json.as_bytes())
        .with_whatever_context(|_| format!("failed to write {}", output.display()))?;

    println!("Exported agent '{}' to {}", args.nous_id, output.display());
    println!("  Workspace: {} files", agent_file.workspace.files.len());
    println!("  Sessions: {}", agent_file.sessions.len());

    Ok(())
}

fn default_export_path(nous_id: &str, exported_at: &str) -> PathBuf {
    let date = exported_at.split('T').next().unwrap_or("now");
    PathBuf::from(format!("{nous_id}-{date}.agent.json"))
}

fn resolve_workspace_path(oikos: &Oikos, workspace: &str) -> PathBuf {
    let path = Path::new(workspace);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        oikos.root().join(path)
    }
}

fn export_workspace(root: &Path) -> Result<mneme::portability::WorkspaceData> {
    let mut files = HashMap::new();
    let mut binary_files = Vec::new();
    if !root.exists() {
        return Ok(mneme::portability::WorkspaceData {
            files,
            binary_files,
        });
    }
    collect_workspace_entries(root, root, &mut files, &mut binary_files)?;
    Ok(mneme::portability::WorkspaceData {
        files,
        binary_files,
    })
}

fn collect_workspace_entries(
    root: &Path,
    dir: &Path,
    files: &mut HashMap<String, String>,
    binary_files: &mut Vec<String>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_whatever_context(|_| format!("failed to read {}", dir.display()))?
    {
        let entry = entry
            .with_whatever_context(|_| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_workspace_entries(root, &path, files, binary_files)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                files.insert(relative, content);
            }
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                binary_files.push(relative);
            }
            Err(e) => {
                return Err(crate::error::Error::msg(format!(
                    "failed to read {}: {e}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut result: String = c.to_uppercase().collect();
            result.push_str(chars.as_str());
            result
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "import orchestrates config, workspace, and session store — sequential by nature"
)]
pub(crate) fn import_agent(instance_root: Option<&PathBuf>, args: &ImportArgs) -> Result<()> {
    validate_target_id(args.target_id.as_deref())?;
    let json = std::fs::read_to_string(&args.file)
        .with_whatever_context(|_| format!("failed to read {}", args.file.display()))?;
    let agent_file: mneme::portability::AgentFile =
        serde_json::from_str(&json).whatever_context("failed to parse agent file")?;

    if agent_file.version < mneme::portability::AGENT_FILE_VERSION {
        whatever!(
            "agent file is version {} but importer requires v{}.\n\
             v1 files are silently lossy on session status, timestamps, and \
             message metadata. Re-export from the source instance — there is no \
             in-place migration. See #4163.",
            agent_file.version,
            mneme::portability::AGENT_FILE_VERSION,
        );
    }

    // SECURITY (#4241): if --target-id is absent, the imported nous.id is
    // used directly to derive on-disk paths. Validate before any I/O.
    if args.target_id.is_none() {
        validate_agent_id_for_paths(&agent_file.nous.id, "imported nous.id")?;
    }
    for path in agent_file.workspace.files.keys() {
        validate_workspace_relative_path(path)?;
    }
    for path in &agent_file.workspace.binary_files {
        validate_workspace_relative_path(path)?;
    }

    let nous_id = args
        .target_id
        .clone()
        .unwrap_or_else(|| agent_file.nous.id.clone());

    if args.dry_run {
        println!("Dry run — no changes will be made\n");
        println!(
            "Agent: {} ({})",
            nous_id,
            agent_file.nous.name.as_deref().unwrap_or("unnamed")
        );
        println!("Generator: {}", agent_file.generator);
        println!("Exported at: {}", agent_file.exported_at);
        println!(
            "Workspace: {} text files, {} binary files",
            agent_file.workspace.files.len(),
            agent_file.workspace.binary_files.len()
        );
        println!("Sessions: {}", agent_file.sessions.len());
        let total_msgs: usize = agent_file.sessions.iter().map(|s| s.messages.len()).sum();
        let total_notes: usize = agent_file.sessions.iter().map(|s| s.notes.len()).sum();
        println!("Messages: {total_msgs}");
        println!("Notes: {total_notes}");
        if let Some(ref memory) = agent_file.memory {
            let vectors = memory.vectors.as_ref().map_or(0, Vec::len);
            let graph = memory.graph.is_some();
            println!("Memory: {vectors} vectors, graph={graph}");
        }
        return Ok(());
    }

    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let nous_dir = oikos.nous_dir(&nous_id);
    if nous_dir.exists() && !args.force {
        whatever!(
            "nous directory already exists: {}\nUse --force to overwrite workspace files.",
            nous_dir.display()
        );
    }

    // Scaffold workspace from agent file.
    if !args.skip_workspace {
        std::fs::create_dir_all(&nous_dir)
            .with_whatever_context(|_| format!("failed to create {}", nous_dir.display()))?;
        for (path, content) in &agent_file.workspace.files {
            let target = nous_dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).with_whatever_context(|_| {
                    format!("failed to create directory: {}", parent.display())
                })?;
            }
            koina::fs::write_restricted(&target, content.as_bytes())
                .with_whatever_context(|_| format!("failed to write {}", target.display()))?;
        }
    }

    // Write config entry.
    let config_path = oikos.config().join("aletheia.toml");
    let existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .with_whatever_context(|_| format!("failed to read {}", config_path.display()))?
    } else {
        String::new()
    };

    let mut doc: toml_edit::DocumentMut = existing
        .parse()
        .with_whatever_context(|_| format!("failed to parse {}", config_path.display()))?;

    let already_listed = doc
        .get("agents")
        .and_then(|a| a.as_table())
        .and_then(|a| a.get("list"))
        .and_then(|l| l.as_array_of_tables())
        .is_some_and(|list| {
            list.iter()
                .any(|t| t.get("id").and_then(|v| v.as_str()) == Some(nous_id.as_str()))
        });

    if already_listed && !args.force {
        whatever!(
            "agent '{}' already exists in the configuration file.\n\
             Use --force to overwrite the existing entry.",
            nous_id
        );
    }

    if already_listed && args.force {
        // Remove existing entry so we can replace it.
        if let Some(list) = doc
            .get_mut("agents")
            .and_then(|a| a.as_table_mut())
            .and_then(|a| a.get_mut("list"))
            .and_then(|l| l.as_array_of_tables_mut())
        {
            let mut idx = None;
            for (i, t) in list.iter().enumerate() {
                if t.get("id").and_then(|v| v.as_str()) == Some(nous_id.as_str()) {
                    idx = Some(i);
                    break;
                }
            }
            if let Some(i) = idx {
                list.remove(i);
            }
        }
    }

    let workspace = format!("nous/{nous_id}");
    let display_name = agent_file
        .nous
        .name
        .clone()
        .unwrap_or_else(|| capitalize(&nous_id));
    let model = agent_file
        .nous
        .model
        .clone()
        .unwrap_or_else(|| koina::defaults::DEFAULT_MODEL.to_owned());

    let mut entry = toml_edit::Table::new();
    entry.insert("id", toml_edit::value(nous_id.clone()));
    entry.insert("name", toml_edit::value(display_name));
    entry.insert("workspace", toml_edit::value(workspace));
    entry.insert("default", toml_edit::value(false));

    let mut model_table = toml_edit::Table::new();
    model_table.insert("primary", toml_edit::value(model));
    model_table.insert(
        "fallbacks",
        toml_edit::Item::Value(toml_edit::Value::Array(toml_edit::Array::new())),
    );
    entry.insert("model", toml_edit::Item::Table(model_table));

    if doc.get("agents").and_then(|i| i.as_table()).is_none() {
        doc.insert("agents", toml_edit::Item::Table(toml_edit::Table::new()));
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "key 'agents' was just inserted if absent, so indexing is valid"
    )]
    let agents = doc["agents"]
        .as_table_mut()
        .ok_or_else(|| crate::error::Error::msg("[agents] in config is not a table"))?;

    if agents
        .get("list")
        .and_then(|i| i.as_array_of_tables())
        .is_none()
    {
        agents.insert(
            "list",
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
        );
    }

    let list = agents["list"].as_array_of_tables_mut().ok_or_else(|| {
        crate::error::Error::msg("agents.list in config is not an array of tables")
    })?;

    list.push(entry);

    koina::fs::write_restricted(&config_path, doc.to_string().as_bytes())
        .with_whatever_context(|_| format!("failed to write {}", config_path.display()))?;

    // Import sessions into graphe.
    if !args.skip_sessions {
        let sessions_db = oikos.sessions_db();
        let store = mneme::store::SessionStore::open(&sessions_db).with_whatever_context(|_| {
            format!("failed to open session store at {}", sessions_db.display())
        })?;

        for session in &agent_file.sessions {
            let status = match session.status.as_str() {
                "active" => SessionStatus::Active,
                "archived" => SessionStatus::Archived,
                "distilled" => SessionStatus::Distilled,
                other => {
                    eprintln!("  WARN: unknown status '{other}', defaulting to 'active'");
                    SessionStatus::Active
                }
            };
            let session_type = match session.session_type.as_str() {
                "primary" => SessionType::Primary,
                "background" => SessionType::Background,
                "ephemeral" => SessionType::Ephemeral,
                other => {
                    eprintln!("  WARN: unknown session_type '{other}', defaulting to 'primary'");
                    SessionType::Primary
                }
            };

            let imported = store
                .import_session(
                    &mneme::types::Session {
                        id: session.id.clone(),
                        nous_id: nous_id.clone(),
                        session_key: session.session_key.clone(),
                        status,
                        model: agent_file.nous.model.clone(),
                        session_type,
                        created_at: session.created_at.clone(),
                        updated_at: session.updated_at.clone(),
                        metrics: SessionMetrics {
                            token_count_estimate: session.token_count_estimate,
                            message_count: session.message_count,
                            last_input_tokens: 0,
                            bootstrap_hash: None,
                            distillation_count: session.distillation_count,
                            last_distilled_at: None,
                            computed_context_tokens: 0,
                        },
                        origin: SessionOrigin {
                            parent_session_id: None,
                            thread_id: None,
                            transport: Some("import".to_owned()),
                            display_name: None,
                        },
                        artefact_meta: None,
                    },
                    args.force,
                )
                .with_whatever_context(|_| format!("failed to import session {}", session.id))?;

            for msg in &session.messages {
                let role = match msg.role.as_str() {
                    "system" => Role::System,
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    "tool_result" => Role::ToolResult,
                    other => {
                        eprintln!("  WARN: unknown role '{other}', defaulting to 'user'");
                        Role::User
                    }
                };
                store
                    .insert_message_raw(&mneme::types::Message {
                        id: msg.seq,
                        session_id: imported.id.clone(),
                        seq: msg.seq,
                        role,
                        content: msg.content.clone(),
                        tool_call_id: None,
                        tool_name: None,
                        token_estimate: msg.token_estimate,
                        is_distilled: msg.is_distilled,
                        created_at: msg.created_at.clone(),
                    })
                    .with_whatever_context(|_| {
                        format!(
                            "failed to insert message seq {} into session {}",
                            msg.seq, session.id
                        )
                    })?;
            }

            if let Some(ws) = &session.working_state {
                let ws_key = format!("ws:{nous_id}:{}", session.id);
                let ws_value = serde_json::to_string(ws).with_whatever_context(|_| {
                    format!("failed to serialize working_state for {}", session.id)
                })?;
                store
                    .blackboard_write(&ws_key, &ws_value, &nous_id, 86_400)
                    .with_whatever_context(|_| {
                        format!("failed to hydrate working_state for {}", session.id)
                    })?;
            }

            for note in &session.notes {
                let category = if mneme::store::SessionStore::VALID_CATEGORIES
                    .contains(&note.category.as_str())
                {
                    note.category.as_str()
                } else {
                    eprintln!(
                        "  WARN: note category '{}' not valid, using 'context'",
                        note.category
                    );
                    "context"
                };
                if let Err(e) = store.add_note(&imported.id, &nous_id, category, &note.content) {
                    eprintln!("  WARN: failed to add note: {e}");
                }
            }
        }

        if let Some(knowledge) = &agent_file.knowledge {
            import_knowledge(&oikos, &nous_id, knowledge)?;
        }
    }

    println!("Imported agent '{nous_id}' from {}", args.file.display());
    println!("  Workspace: {} files", agent_file.workspace.files.len());
    println!("  Sessions: {}", agent_file.sessions.len());

    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "CLI dispatch is inherently verbose — splitting would hurt readability"
)]
pub(crate) fn seed_skills(instance_root: Option<&PathBuf>, args: &SeedSkillsArgs) -> Result<()> {
    use mneme::skill::{SkillContent, parse_skill_md, scan_skill_dir};

    validate_nous_id(&args.nous_id)?;
    let dir = &args.dir;
    let nous_id = &args.nous_id;
    let entries = scan_skill_dir(dir)
        .with_whatever_context(|_| format!("failed to scan skill directory: {}", dir.display()))?;

    if entries.is_empty() {
        println!("No SKILL.md files found in {}", dir.display());
        return Ok(());
    }

    println!("Found {} skill(s) in {}", entries.len(), dir.display());

    let mut parsed: Vec<(String, SkillContent)> = Vec::new();
    let mut parse_errors = 0u32;
    for (slug, content) in &entries {
        match parse_skill_md(content, slug) {
            Ok(skill) => parsed.push((slug.clone(), skill)),
            Err(e) => {
                eprintln!("  SKIP {slug}: {e}");
                parse_errors += 1;
            }
        }
    }

    if args.dry_run {
        println!(
            "\n[dry-run] Would seed {} skill(s) for nous '{nous_id}':",
            parsed.len()
        );
        for (slug, skill) in &parsed {
            println!(
                "  {slug}: {} steps, {} tools, tags: [{}]",
                skill.steps.len(),
                skill.tools_used.len(),
                skill.domain_tags.join(", ")
            );
        }
        if parse_errors > 0 {
            println!("\n{parse_errors} skill(s) skipped due to parse errors");
        }
        return Ok(());
    }

    #[cfg(feature = "recall")]
    {
        use mneme::knowledge::{
            EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
            default_stability_hours,
        };
        use mneme::knowledge_store::KnowledgeStore;

        let oikos = super::resolve_oikos(instance_root)?;
        let knowledge_path = knowledge_path_for_nous(&oikos, nous_id);
        let config = mneme::knowledge_store::KnowledgeConfig::default();
        let store =
            KnowledgeStore::open_fjall(&knowledge_path, config).with_whatever_context(|_| {
                format!(
                    "failed to open knowledge store at {}",
                    knowledge_path.display()
                )
            })?;

        let now = jiff::Timestamp::now();
        let mut seeded = 0u32;
        let mut skipped = 0u32;
        let mut overwritten = 0u32;

        for (slug, skill) in &parsed {
            let existing = store
                .find_skill_by_name(nous_id, &skill.name)
                .whatever_context("failed to query existing skills")?;

            if let Some(existing_id) = existing {
                if args.force {
                    if let Err(e) = store.forget_fact(
                        &mneme::id::FactId::new(existing_id).whatever_context("invalid fact id")?,
                        mneme::knowledge::ForgetReason::Outdated,
                    ) {
                        eprintln!("  WARN: failed to supersede {slug}: {e}");
                    }
                    overwritten += 1;
                } else {
                    println!("  SKIP {slug}: already exists (use --force to overwrite)");
                    skipped += 1;
                    continue;
                }
            }

            let content_json = serde_json::to_string(skill)
                .with_whatever_context(|_| format!("failed to serialize skill: {slug}"))?;

            let fact_id = koina::ulid::Ulid::new().to_string();
            let fact = Fact {
                id: mneme::id::FactId::new(fact_id.clone()).whatever_context("invalid fact id")?,
                nous_id: nous_id.to_owned(),
                content: content_json.clone(),
                fact_type: "skill".to_owned(),
                scope: None,
                project_id: None,
                temporal: FactTemporal {
                    valid_from: now,
                    valid_to: mneme::knowledge::far_future(),
                    recorded_at: now,
                },
                provenance: FactProvenance {
                    confidence: 0.5,
                    tier: EpistemicTier::Assumed,
                    source_session_id: None,
                    stability_hours: default_stability_hours("skill"),
                },
                lifecycle: FactLifecycle {
                    superseded_by: None,
                    is_forgotten: false,
                    forgotten_at: None,
                    forget_reason: None,
                },
                access: FactAccess {
                    access_count: 0,
                    last_accessed_at: None,
                },
                sensitivity: mneme::knowledge::FactSensitivity::Public,
                visibility: mneme::knowledge::Visibility::Private,
            };

            store
                .insert_fact(&fact)
                .with_whatever_context(|_| format!("failed to insert skill {slug}"))?;

            let embedding_text = format!("{}: {}", skill.name, skill.description);
            let emb_id = koina::ulid::Ulid::new().to_string();
            let chunk = mneme::knowledge::EmbeddedChunk {
                id: mneme::id::EmbeddingId::new(emb_id).whatever_context("invalid embedding id")?,
                content: embedding_text,
                source_type: "fact".to_owned(),
                source_id: fact_id,
                nous_id: nous_id.to_owned(),
                embedding: generate_simple_embedding(&content_json),
                created_at: now,
            };
            if let Err(e) = store.insert_embedding(&chunk) {
                eprintln!("  WARN: failed to insert embedding for {slug}: {e}");
            }

            println!("  SEED {slug}");
            seeded += 1;
        }

        println!(
            "\nDone: {seeded} seeded, {skipped} skipped, {overwritten} overwritten, {parse_errors} parse errors"
        );

        Ok(())
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (instance_root, args, nous_id, parsed, parse_errors);
        whatever!(
            "seed-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }
}

/// Export skills from the knowledge store to Claude Code's native format.
///
/// Reads skill facts from an in-process `KnowledgeStore`, converts them to
/// `SkillContent`, and writes `.claude/skills/<slug>/SKILL.md` files.
pub(crate) async fn export_skills(
    instance_root: Option<&PathBuf>,
    args: &ExportSkillsArgs,
) -> Result<()> {
    validate_nous_id(&args.nous_id)?;
    if let Err(e) = reqwest::Url::parse(&args.url) {
        whatever!("--url is not a valid URL: {e} (got {:?})", args.url);
    }
    guard_knowledge_lock(&args.url).await?;
    #[cfg(feature = "recall")]
    {
        use mneme::knowledge_store::KnowledgeStore;
        use mneme::skill::{SkillContent, export_skills_to_cc};

        let oikos = match instance_root {
            Some(root) => Oikos::from_root(root),
            None => Oikos::discover(),
        };
        let knowledge_path = knowledge_path_for_nous(&oikos, &args.nous_id);

        let config = mneme::knowledge_store::KnowledgeConfig::default();
        let store =
            KnowledgeStore::open_fjall(&knowledge_path, config).with_whatever_context(|_| {
                format!(
                    "failed to open knowledge store at {}",
                    knowledge_path.display()
                )
            })?;

        let nous_id = &args.nous_id;
        let facts = store
            .find_skills_for_nous(nous_id, 500)
            .whatever_context("failed to query skills")?;

        if facts.is_empty() {
            println!("No skills found for nous '{nous_id}'");
            return Ok(());
        }

        let mut skills: Vec<SkillContent> = Vec::new();
        let mut parse_errors = 0u32;
        for fact in &facts {
            match serde_json::from_str::<SkillContent>(&fact.content) {
                Ok(skill) => skills.push(skill),
                Err(e) => {
                    eprintln!("  SKIP {}: failed to parse content: {e}", fact.id);
                    parse_errors += 1;
                }
            }
        }

        let domain_tags: Vec<&str> = match args.domain.as_deref() {
            Some(domain) => domain.split(',').map(str::trim).collect(),
            None => Vec::new(),
        };
        let filter = if domain_tags.is_empty() {
            None
        } else {
            Some(domain_tags.as_slice())
        };

        let output = &args.output;
        let exported =
            export_skills_to_cc(&skills, output, filter).with_whatever_context(|_| {
                format!("failed to export skills to {}", output.display())
            })?;

        println!(
            "Exported {} skill(s) to {}",
            exported.len(),
            output.display()
        );
        for ex in &exported {
            println!("  {} → {}", ex.name, ex.path.display());
        }
        if parse_errors > 0 {
            println!("\n{parse_errors} skill(s) skipped due to parse errors");
        }

        Ok(())
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (instance_root, args);
        whatever!(
            "export-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }
}

pub(crate) async fn review_skills(
    instance_root: Option<&PathBuf>,
    args: &ReviewSkillsArgs,
) -> Result<()> {
    validate_nous_id(&args.nous_id)?;
    guard_knowledge_lock(&args.url).await?;

    #[cfg(feature = "recall")]
    {
        use mneme::knowledge_store::KnowledgeStore;
        use mneme::skills::extract::PendingSkill;

        let oikos = match instance_root {
            Some(root) => Oikos::from_root(root),
            None => Oikos::discover(),
        };
        let knowledge_path = knowledge_path_for_nous(&oikos, &args.nous_id);

        let config = mneme::knowledge_store::KnowledgeConfig::default();
        let store =
            KnowledgeStore::open_fjall(&knowledge_path, config).with_whatever_context(|_| {
                format!(
                    "failed to open knowledge store at {}",
                    knowledge_path.display()
                )
            })?;

        let nous_id = &args.nous_id;
        match args.action.as_str() {
            "list" => {
                let pending = store
                    .find_pending_skills(nous_id)
                    .whatever_context("failed to query pending skills")?;

                if pending.is_empty() {
                    println!("No pending skills for nous '{nous_id}'");
                    return Ok(());
                }

                println!(
                    "Found {} pending skill(s) for nous '{nous_id}':\n",
                    pending.len()
                );
                for fact in &pending {
                    match PendingSkill::from_json(&fact.content) {
                        Ok(ps) => {
                            println!("  ID: {}", fact.id);
                            println!("  Name: {}", ps.skill.name);
                            println!(
                                "  Description: {}",
                                ps.skill.description.lines().next().unwrap_or("")
                            );
                            println!("  Tools: {}", ps.skill.tools_used.join(", "));
                            println!("  Tags: {}", ps.skill.domain_tags.join(", "));
                            println!("  Steps: {}", ps.skill.steps.len());
                            println!("  Status: {}", ps.status);
                            println!("  Candidate: {}", ps.candidate_id);
                            println!("  Extracted: {}", ps.extracted_at);
                            println!();
                        }
                        Err(e) => {
                            eprintln!("  SKIP {}: failed to parse: {e}", fact.id);
                        }
                    }
                }
            }
            "approve" => {
                let fid = args.fact_id.as_deref().ok_or_else(|| {
                    crate::error::Error::msg("--fact-id required for approve action")
                })?;
                let fact_id = mneme::id::FactId::new(fid).whatever_context("invalid fact id")?;
                let new_id = store
                    .approve_pending_skill(&fact_id, nous_id)
                    .whatever_context("failed to approve skill")?;
                println!("Approved: {fid} → new skill fact: {new_id}");
            }
            "reject" => {
                let fid = args.fact_id.as_deref().ok_or_else(|| {
                    crate::error::Error::msg("--fact-id required for reject action")
                })?;
                let fact_id = mneme::id::FactId::new(fid).whatever_context("invalid fact id")?;
                store
                    .reject_pending_skill(&fact_id)
                    .whatever_context("failed to reject skill")?;
                println!("Rejected: {fid}");
            }
            other => {
                whatever!("unknown action '{other}'. Use: list, approve, reject");
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (instance_root, args);
        whatever!(
            "review-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }
}

#[expect(
    clippy::unused_async,
    reason = "async required when migrate-qdrant feature is enabled"
)]
pub(crate) async fn migrate_memory(
    instance_root: Option<&PathBuf>,
    args: MigrateMemoryArgs,
) -> Result<()> {
    #[cfg(feature = "migrate-qdrant")]
    {
        return crate::migrate_memory::run(
            instance_root,
            &args.qdrant_url,
            &args.collection,
            args.knowledge_path.as_ref(),
            args.review_file.as_ref(),
            args.dry_run,
        )
        .await;
    }
    #[cfg(not(feature = "migrate-qdrant"))]
    {
        let _ = (instance_root, args);
        whatever!(
            "migrate-memory requires the `migrate-qdrant` feature.\n\
             Rebuild with: cargo build --features migrate-qdrant"
        );
    }
}

#[cfg(feature = "recall")]
/// Generate a deterministic pseudo-embedding for seeding (384-dim).
///
/// Uses a simple hash-based approach. Real embeddings come from the
/// candle embedding provider at runtime.
fn generate_simple_embedding(text: &str) -> Vec<f32> {
    use sha2::{Digest, Sha256};
    let dim = 384;
    let mut embedding = Vec::with_capacity(dim);
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());

    let mut seed = hasher.finalize().to_vec();
    while embedding.len() < dim {
        for byte in &seed {
            if embedding.len() >= dim {
                break;
            }
            // WHY: map byte to [-1.0, 1.0]: value fits without overflow, truncation is harmless
            #[expect(
                clippy::cast_possible_truncation,
                clippy::as_conversions,
                reason = "f64→f32: result fits in f32 range"
            )]
            embedding.push((f64::from(*byte) / 127.5 - 1.0) as f32);
        }
        let mut h = Sha256::new();
        h.update(&seed);
        seed = h.finalize().to_vec();
    }

    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut embedding {
            *v /= norm;
        }
    }

    embedding
}

/// Check if the server is running and holding the knowledge store lock.
///
/// Returns an error with a helpful message if the server is reachable,
/// preventing a confusing `FjallError::Locked` crash. A malformed `url`
/// is rejected up-front so a parse failure does not silently coerce to
/// "server not running" and let the caller proceed past the guard.
pub(crate) async fn guard_knowledge_lock(url: &str) -> Result<()> {
    if let Err(e) = reqwest::Url::parse(url) {
        whatever!("--url is not a valid URL: {e} (got {:?})", url);
    }
    let endpoint = format!("{url}/api/health");
    if let Ok(resp) = reqwest::get(&endpoint).await
        && (resp.status().is_success() || resp.status().as_u16() == 503)
    {
        whatever!(
            "The server at {url} is running and holds an exclusive lock on the knowledge store.\n  \
             Stop the server first to use this subcommand, or use the REST API:\n  \
             GET {url}/api/v1/knowledge/facts"
        );
    }
    Ok(())
}
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test setup writes files to temp directories; synchronous I/O is required in test contexts"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after len assertions"
)]
mod tests {
    use std::collections::HashMap;
    use std::fmt::Write as _;

    use mneme::portability::{
        AgentFile, ExportedMessage, ExportedNote, ExportedSession, NousInfo, WorkspaceData,
    };

    use super::*;

    #[test]
    fn capitalize_first_letter() {
        assert_eq!(capitalize("analyst"), "Analyst");
        assert_eq!(capitalize("my-agent"), "My-agent");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("A"), "A");
    }

    #[test]
    fn validate_nous_id_rejects_empty() {
        let err = validate_nous_id("").unwrap_err();
        assert!(
            err.to_string().contains("--nous-id must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_nous_id_rejects_whitespace_only() {
        let err = validate_nous_id("   ").unwrap_err();
        assert!(
            err.to_string().contains("--nous-id must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_nous_id_accepts_well_formed() {
        assert!(validate_nous_id("pronoea").is_ok());
        assert!(validate_nous_id("agent-with-hyphens").is_ok());
    }

    #[test]
    fn validate_nous_id_rejects_path_traversal() {
        let err = validate_nous_id("../escape").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--nous-id") && msg.contains("alphanumeric"),
            "got: {msg}"
        );
    }

    #[test]
    fn validate_nous_id_rejects_absolute_path() {
        let err = validate_nous_id("/etc/passwd").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--nous-id") && msg.contains("alphanumeric"),
            "got: {msg}"
        );
    }

    #[test]
    fn validate_target_id_accepts_absent() {
        assert!(validate_target_id(None).is_ok());
    }

    #[test]
    fn validate_target_id_accepts_well_formed() {
        assert!(validate_target_id(Some("pronoea")).is_ok());
        assert!(validate_target_id(Some("agent-with-hyphens")).is_ok());
    }

    #[test]
    fn validate_target_id_rejects_empty() {
        let err = validate_target_id(Some("")).unwrap_err();
        assert!(
            err.to_string().contains("--target-id must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_target_id_rejects_whitespace_only() {
        let err = validate_target_id(Some("   ")).unwrap_err();
        assert!(
            err.to_string().contains("--target-id must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_target_id_rejects_path_traversal() {
        let err = validate_target_id(Some("../../../tmp/escaped")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--target-id") && msg.contains("alphanumeric"),
            "got: {msg}"
        );
    }

    #[test]
    fn validate_target_id_rejects_absolute_path() {
        let err = validate_target_id(Some("/tmp/abs")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--target-id") && msg.contains("alphanumeric"),
            "got: {msg}"
        );
    }

    #[test]
    fn validate_target_id_rejects_separators() {
        for bad in ["a/b", "a\\b", "..", "."] {
            let err = validate_target_id(Some(bad)).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("--target-id") && msg.contains("alphanumeric"),
                "for {bad:?} got: {msg}"
            );
        }
    }

    #[test]
    fn validate_target_id_rejects_nul_byte() {
        let err = validate_target_id(Some("agent\0name")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("NUL"), "got: {msg}");
    }

    #[test]
    fn validate_agent_id_for_paths_rejects_leading_hyphen() {
        let err = validate_agent_id_for_paths("-agent", "id").unwrap_err();
        assert!(err.to_string().contains("hyphen"), "got: {err}");
    }

    #[test]
    fn validate_workspace_relative_path_accepts_well_formed() {
        assert!(validate_workspace_relative_path("SOUL.md").is_ok());
        assert!(validate_workspace_relative_path("subdir/file.txt").is_ok());
        assert!(validate_workspace_relative_path("a/b/c.md").is_ok());
    }

    #[test]
    fn validate_workspace_relative_path_rejects_parent_traversal() {
        let err = validate_workspace_relative_path("../escape.txt").unwrap_err();
        assert!(err.to_string().contains(".."), "got: {err}");
        let err = validate_workspace_relative_path("subdir/../../escape").unwrap_err();
        assert!(err.to_string().contains(".."), "got: {err}");
    }

    #[test]
    fn validate_workspace_relative_path_rejects_absolute() {
        let err = validate_workspace_relative_path("/etc/passwd").unwrap_err();
        assert!(err.to_string().contains("relative"), "got: {err}");
    }

    #[test]
    fn validate_workspace_relative_path_rejects_nul() {
        let err = validate_workspace_relative_path("a\0b").unwrap_err();
        assert!(err.to_string().contains("NUL"), "got: {err}");
    }

    #[test]
    fn validate_workspace_relative_path_rejects_empty() {
        let err = validate_workspace_relative_path("").unwrap_err();
        assert!(err.to_string().contains("empty"), "got: {err}");
    }

    #[tokio::test]
    async fn guard_knowledge_lock_rejects_empty_url() {
        let err = guard_knowledge_lock("").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--url is not a valid URL"), "got: {msg}");
    }

    #[tokio::test]
    async fn guard_knowledge_lock_rejects_malformed_url() {
        let err = guard_knowledge_lock("not-a-url").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--url is not a valid URL"), "got: {msg}");
    }

    #[tokio::test]
    async fn guard_knowledge_lock_rejects_whitespace_url() {
        let err = guard_knowledge_lock("   ").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--url is not a valid URL"), "got: {msg}");
    }

    #[tokio::test]
    async fn guard_knowledge_lock_accepts_well_formed_url_when_no_server() {
        // 127.0.0.1:1 is not bound — the reqwest call fails, but a well-formed
        // URL should pass the parse check and return Ok(()) (no server detected).
        let res = guard_knowledge_lock("http://127.0.0.1:1").await;
        assert!(
            res.is_ok(),
            "expected Ok for well-formed URL with no listener; got: {res:?}"
        );
    }

    fn sample_agent_file() -> AgentFile {
        AgentFile {
            version: mneme::portability::AGENT_FILE_VERSION,
            exported_at: "2026-03-05T12:00:00Z".to_owned(),
            generator: "aletheia-rust/0.10.0".to_owned(),
            nous: NousInfo {
                id: "imported-agent".to_owned(),
                name: Some("Imported Agent".to_owned()),
                model: Some("claude-sonnet-4-6".to_owned()),
                config: serde_json::json!({"domains": ["general"]}),
            },
            workspace: WorkspaceData {
                files: HashMap::from([(
                    "SOUL.md".to_owned(),
                    "# Imported Agent\n\nYou are imported.\n".to_owned(),
                )]),
                binary_files: vec![],
            },
            sessions: vec![ExportedSession {
                id: "ses-001".to_owned(),
                session_key: "main".to_owned(),
                status: "active".to_owned(),
                session_type: "primary".to_owned(),
                message_count: 2,
                token_count_estimate: 150,
                distillation_count: 0,
                created_at: "2026-03-05T10:00:00Z".to_owned(),
                updated_at: "2026-03-05T11:00:00Z".to_owned(),
                working_state: None,
                distillation_priming: None,
                notes: vec![ExportedNote {
                    category: "task".to_owned(),
                    content: "working on import".to_owned(),
                    created_at: "2026-03-05T10:30:00Z".to_owned(),
                }],
                messages: vec![
                    ExportedMessage {
                        role: "user".to_owned(),
                        content: "hello".to_owned(),
                        seq: 1,
                        token_estimate: 50,
                        is_distilled: false,
                        created_at: "2026-03-05T10:00:00Z".to_owned(),
                    },
                    ExportedMessage {
                        role: "assistant".to_owned(),
                        content: "hi there".to_owned(),
                        seq: 2,
                        token_estimate: 100,
                        is_distilled: false,
                        created_at: "2026-03-05T10:00:01Z".to_owned(),
                    },
                ],
            }],
            memory: None,
            knowledge: None,
        }
    }

    fn write_agent_config(root: &Path, agent_id: &str, name: &str) {
        let oikos = Oikos::from_root(root);
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();
        std::fs::create_dir_all(oikos.nous_dir(agent_id)).unwrap();
        std::fs::write(
            oikos.config().join("aletheia.toml"),
            format!(
                r#"
[agents.defaults.model]
primary = "mock-model"

[[agents.list]]
id = "{agent_id}"
name = "{name}"
workspace = "nous/{agent_id}"
"#
            ),
        )
        .unwrap();
    }

    fn write_mock_embedding_config(root: &Path, dimension: usize) {
        let oikos = Oikos::from_root(root);
        let config_path = oikos.config().join("aletheia.toml");
        let mut config = std::fs::read_to_string(&config_path).unwrap();
        write!(
            config,
            "\n[embedding]\nprovider = \"mock\"\ndimension = {dimension}\n"
        )
        .unwrap();
        std::fs::write(config_path, config).unwrap();
    }

    #[test]
    fn export_agent_writes_portable_file_from_fjall_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");
        std::fs::write(oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let store = mneme::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let session = store
            .create_session("ses-export", "alice", "main", None, Some("mock-model"))
            .unwrap();
        store
            .append_message(&session.id, Role::User, "hello", None, None, 5)
            .unwrap();
        store
            .append_message(&session.id, Role::Assistant, "hi", None, None, 7)
            .unwrap();
        store
            .add_note(&session.id, "alice", "task", "remember this")
            .unwrap();
        drop(store);

        let output = dir.path().join("alice.agent.json");
        let args = ExportArgs {
            nous_id: "alice".to_owned(),
            output: Some(output.clone()),
            archived: false,
            max_messages: 0,
            compact: false,
            force: false,
        };
        export_agent(Some(&dir.path().to_path_buf()), &args).unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
        assert_eq!(exported.nous.id, "alice");
        assert_eq!(exported.nous.name.as_deref(), Some("Alice"));
        assert_eq!(
            exported.workspace.files.get("SOUL.md").map(String::as_str),
            Some("# Alice\n")
        );
        assert_eq!(exported.sessions.len(), 1);
        assert_eq!(exported.sessions[0].messages.len(), 2);
        assert_eq!(exported.sessions[0].notes.len(), 1);
    }

    #[test]
    fn export_agent_output_round_trips_through_import() {
        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let source_store = mneme::store::SessionStore::open(&source_oikos.sessions_db()).unwrap();
        let session = source_store
            .create_session("ses-roundtrip", "alice", "main", None, Some("mock-model"))
            .unwrap();
        source_store
            .append_message(&session.id, Role::User, "round trip", None, None, 10)
            .unwrap();
        drop(source_store);

        let export_path = source.path().join("alice.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export_path.clone()),
                archived: false,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let dest = tempfile::tempdir().unwrap();
        let dest_oikos = Oikos::from_root(dest.path());
        std::fs::create_dir_all(dest_oikos.config()).unwrap();
        std::fs::create_dir_all(dest_oikos.data()).unwrap();
        import_agent(
            Some(&dest.path().to_path_buf()),
            &ImportArgs {
                file: export_path,
                target_id: None,
                skip_sessions: false,
                skip_workspace: false,
                force: false,
                dry_run: false,
            },
        )
        .unwrap();

        let dest_store = mneme::store::SessionStore::open(&dest_oikos.sessions_db()).unwrap();
        let sessions = dest_store.list_sessions(Some("alice")).unwrap();
        assert_eq!(sessions.len(), 1);
        let history = dest_store.get_history(&sessions[0].id, None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "round trip");
    }

    /// #4163/C — import preserves session status, timestamps, and metrics
    /// via [`import_session`].
    #[test]
    fn import_preserves_session_status_4163_c() {
        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let source_store = mneme::store::SessionStore::open(&source_oikos.sessions_db()).unwrap();
        let session = source_store
            .create_session("ses-resurrect", "alice", "main", None, Some("mock-model"))
            .unwrap();
        source_store
            .append_message(&session.id, Role::User, "before archive", None, None, 7)
            .unwrap();
        source_store
            .update_session_status(&session.id, SessionStatus::Archived)
            .unwrap();

        let source_session_before_export = source_store
            .find_session_by_id(&session.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            source_session_before_export.status,
            SessionStatus::Archived,
            "source session should be archived before export"
        );
        let source_created_at = source_session_before_export.created_at.clone();
        let source_updated_at = source_session_before_export.updated_at.clone();
        let source_metrics = source_session_before_export.metrics.clone();
        drop(source_store);

        let export_path = source.path().join("alice.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export_path.clone()),
                archived: true,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let dest = tempfile::tempdir().unwrap();
        let dest_oikos = Oikos::from_root(dest.path());
        std::fs::create_dir_all(dest_oikos.config()).unwrap();
        std::fs::create_dir_all(dest_oikos.data()).unwrap();
        import_agent(
            Some(&dest.path().to_path_buf()),
            &ImportArgs {
                file: export_path,
                target_id: None,
                skip_sessions: false,
                skip_workspace: false,
                force: false,
                dry_run: false,
            },
        )
        .unwrap();

        let dest_store = mneme::store::SessionStore::open(&dest_oikos.sessions_db()).unwrap();
        let dest_sessions = dest_store.list_sessions(Some("alice")).unwrap();
        assert_eq!(dest_sessions.len(), 1);
        let dest_session = &dest_sessions[0];

        assert_eq!(
            dest_session.status,
            SessionStatus::Archived,
            "#4163/C: imported session must preserve archived status"
        );
        assert_eq!(
            dest_session.created_at, source_created_at,
            "#4163/C: imported session must preserve created_at"
        );
        assert_eq!(
            dest_session.updated_at, source_updated_at,
            "#4163/C: imported session must preserve updated_at"
        );
        assert_eq!(
            dest_session.metrics.distillation_count, source_metrics.distillation_count,
            "#4163/C: imported session must preserve distillation_count"
        );
        assert_eq!(
            dest_session.metrics.message_count, source_metrics.message_count,
            "#4163/C: imported session must preserve message_count"
        );
        assert_eq!(
            dest_session.metrics.token_count_estimate, source_metrics.token_count_estimate,
            "#4163/C: imported session must preserve token_count_estimate"
        );
    }

    /// #4163/D — import preserves per-message `seq`, `is_distilled`, and
    /// `created_at` via [`insert_message_raw`].
    #[test]
    fn import_preserves_message_metadata_4163_d() {
        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let source_store = mneme::store::SessionStore::open(&source_oikos.sessions_db()).unwrap();
        let session = source_store
            .create_session("ses-meta", "alice", "main", None, Some("mock-model"))
            .unwrap();
        source_store
            .append_message(&session.id, Role::User, "msg one", None, None, 10)
            .unwrap();
        source_store
            .append_message(&session.id, Role::Assistant, "msg two", None, None, 20)
            .unwrap();
        source_store
            .append_message(&session.id, Role::User, "msg three", None, None, 30)
            .unwrap();
        // Distill seqs 1 and 2 so the export includes both distilled and
        // non-distilled messages.
        source_store
            .mark_messages_distilled(&session.id, &[1, 2])
            .unwrap();

        let source_history = source_store.get_history_raw(&session.id, None).unwrap();
        assert_eq!(source_history.len(), 3);
        drop(source_store);

        let export_path = source.path().join("alice.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export_path.clone()),
                archived: false,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let dest = tempfile::tempdir().unwrap();
        let dest_oikos = Oikos::from_root(dest.path());
        std::fs::create_dir_all(dest_oikos.config()).unwrap();
        std::fs::create_dir_all(dest_oikos.data()).unwrap();
        import_agent(
            Some(&dest.path().to_path_buf()),
            &ImportArgs {
                file: export_path,
                target_id: None,
                skip_sessions: false,
                skip_workspace: false,
                force: false,
                dry_run: false,
            },
        )
        .unwrap();

        let dest_store = mneme::store::SessionStore::open(&dest_oikos.sessions_db()).unwrap();
        let dest_history = dest_store.get_history_raw(&session.id, None).unwrap();
        assert_eq!(
            dest_history.len(),
            3,
            "#4163/D: all messages must be imported"
        );

        for (src, dst) in source_history.iter().zip(dest_history.iter()) {
            assert_eq!(
                dst.id, src.id,
                "#4163/D: message id must match seq {}",
                src.seq
            );
            assert_eq!(dst.seq, src.seq, "#4163/D: seq must be preserved");
            assert_eq!(
                dst.is_distilled, src.is_distilled,
                "#4163/D: is_distilled must be preserved for seq {}",
                src.seq
            );
            assert_eq!(
                dst.created_at, src.created_at,
                "#4163/D: created_at must be preserved for seq {}",
                src.seq
            );
            assert_eq!(
                dst.role, src.role,
                "#4163/D: role must be preserved for seq {}",
                src.seq
            );
            assert_eq!(
                dst.content, src.content,
                "#4163/D: content must be preserved for seq {}",
                src.seq
            );
            assert_eq!(
                dst.token_estimate, src.token_estimate,
                "#4163/D: token_estimate must be preserved for seq {}",
                src.seq
            );
        }
    }

    /// #4163/A — `export_agent` now reads session history via
    /// `get_history_raw`, so distilled messages survive the export. This was
    /// previously a pinning test for the bug; with PR2 it flips into a
    /// fidelity proof. The detailed per-field assertions (seq order,
    /// `is_distilled` preservation, `created_at` exactness) are tightened
    /// further in PR4.
    #[test]
    fn export_preserves_distilled_messages_4163_a() {
        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let source_store = mneme::store::SessionStore::open(&source_oikos.sessions_db()).unwrap();
        let session = source_store
            .create_session("ses-distill", "alice", "main", None, Some("mock-model"))
            .unwrap();
        source_store
            .append_message(&session.id, Role::User, "old 1", None, None, 100)
            .unwrap();
        source_store
            .append_message(&session.id, Role::User, "old 2", None, None, 150)
            .unwrap();
        source_store
            .append_message(&session.id, Role::User, "keep this", None, None, 50)
            .unwrap();
        // Distill seqs 1 and 2 — only "keep this" should remain visible to
        // `get_history`, and only "keep this" should land in the export.
        source_store
            .mark_messages_distilled(&session.id, &[1, 2])
            .unwrap();
        drop(source_store);

        let export_path = source.path().join("alice.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export_path.clone()),
                archived: false,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
        assert_eq!(exported.sessions.len(), 1);
        assert_eq!(
            exported.sessions[0].messages.len(),
            3,
            "#4163/A: export must preserve distilled messages alongside the tail"
        );
        let mut by_seq: Vec<_> = exported.sessions[0].messages.iter().collect();
        by_seq.sort_by_key(|m| m.seq);
        assert_eq!(by_seq[0].content, "old 1");
        assert!(by_seq[0].is_distilled, "distilled flag preserved on seq 1");
        assert_eq!(by_seq[1].content, "old 2");
        assert!(by_seq[1].is_distilled, "distilled flag preserved on seq 2");
        assert_eq!(by_seq[2].content, "keep this");
        assert!(
            !by_seq[2].is_distilled,
            "non-distilled tail still flagged correctly"
        );
        assert_eq!(
            exported.version,
            mneme::portability::AGENT_FILE_VERSION,
            "version bump to v2 declares the fidelity contract"
        );
    }

    /// #4163/B — `export_agent` reads the per-session working state from the
    /// blackboard at `ws:{nous_id}:{session_id}` and serializes it into the
    /// `workingState` slot. Before PR2 this slot was hardcoded to `None`,
    /// silently dropping any task stack / focus state on round-trip.
    #[test]
    fn export_preserves_working_state_4163_b() {
        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let source_store = mneme::store::SessionStore::open(&source_oikos.sessions_db()).unwrap();
        let session = source_store
            .create_session("ses-ws", "alice", "main", None, Some("mock-model"))
            .unwrap();
        let ws_value = serde_json::json!({
            "task_stack": [{"description": "ship #4163", "started_at": "2026-05-29T00:00:00Z"}],
            "focus": {"file": "agent_io.rs"},
            "updated_at": "2026-05-29T00:00:00Z"
        });
        let ws_key = format!("ws:alice:{}", session.id);
        source_store
            .blackboard_write(&ws_key, &ws_value.to_string(), "alice", 86_400)
            .unwrap();
        drop(source_store);

        let export_path = source.path().join("alice.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export_path.clone()),
                archived: false,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
        assert_eq!(exported.sessions.len(), 1);
        let ws = exported.sessions[0].working_state.as_ref();
        assert!(
            ws.is_some(),
            "working_state should be populated from blackboard"
        );
        let ws = ws.unwrap();
        assert_eq!(
            ws.get("focus").and_then(|f| f.get("file")),
            Some(&serde_json::Value::String("agent_io.rs".to_owned()))
        );
        assert_eq!(
            ws.get("task_stack")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn seed_skills_persists_to_fjall_store() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");

        let skills_dir = dir.path().join("skills");
        let skill_path = skills_dir.join("rust-errors");
        std::fs::create_dir_all(&skill_path).unwrap();
        std::fs::write(
            skill_path.join("SKILL.md"),
            "# Rust Errors\n\nUse this when diagnosing Rust errors.\n\n## Steps\n1. Read the compiler output\n",
        )
        .unwrap();

        seed_skills(
            Some(&dir.path().to_path_buf()),
            &SeedSkillsArgs {
                dir: skills_dir,
                nous_id: "alice".to_owned(),
                force: false,
                dry_run: false,
            },
        )
        .unwrap();

        let store = mneme::knowledge_store::KnowledgeStore::open_fjall(
            oikos.knowledge_cohort_db("shared"),
            mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .unwrap();
        let facts = store.find_skills_for_nous("alice", 10).unwrap();
        assert_eq!(facts.len(), 1);
        let persisted: mneme::skill::SkillContent =
            serde_json::from_str(&facts[0].content).unwrap();
        assert_eq!(persisted.name, "rust-errors");
    }

    #[test]
    fn import_agent_writes_config_and_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let agent_file = sample_agent_file();
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("test.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: false,
            skip_workspace: false,
            force: false,
            dry_run: false,
        };

        import_agent(Some(&dir.path().to_path_buf()), &args).unwrap();

        // Verify config was written.
        let config = std::fs::read_to_string(oikos.config().join("aletheia.toml")).unwrap();
        assert!(config.contains(r#"id = "imported-agent""#));
        assert!(config.contains(r#"name = "Imported Agent""#));

        // Verify workspace was written.
        let soul =
            std::fs::read_to_string(oikos.nous_dir("imported-agent").join("SOUL.md")).unwrap();
        assert!(soul.contains("Imported Agent"));

        // Verify sessions were imported.
        let store = mneme::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let sessions = store.list_sessions(Some("imported-agent")).unwrap();
        assert_eq!(sessions.len(), 1, "one session should be imported");

        let history = store.get_history(&sessions[0].id, None).unwrap();
        assert_eq!(history.len(), 2, "two messages should be imported");
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[1].content, "hi there");
    }

    #[test]
    fn import_agent_skips_sessions_when_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let agent_file = sample_agent_file();
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("test.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: true,
            skip_workspace: false,
            force: false,
            dry_run: false,
        };

        import_agent(Some(&dir.path().to_path_buf()), &args).unwrap();

        let store = mneme::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let sessions = store.list_sessions(Some("imported-agent")).unwrap();
        assert!(sessions.is_empty(), "sessions should be skipped");
    }

    /// Regression for #4241: a `.agent.json` whose `nous.id` contains
    /// a traversal pattern must be rejected before any I/O.
    #[test]
    fn import_agent_rejects_traversal_nous_id() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let mut agent_file = sample_agent_file();
        agent_file.nous.id = "../../../tmp/evil-from-file".to_owned();
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("evil.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: true,
            skip_workspace: true,
            force: false,
            dry_run: false,
        };
        let err = import_agent(Some(&dir.path().to_path_buf()), &args).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("imported nous.id") && msg.contains("alphanumeric"),
            "got: {msg}"
        );
    }

    /// Regression for #4241: a workspace file path with `..` must be
    /// rejected before any file is written.
    #[test]
    fn import_agent_rejects_traversal_workspace_filename() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let mut agent_file = sample_agent_file();
        agent_file.workspace.files.insert(
            "../../../tmp/evil-by-filename.txt".to_owned(),
            "PWNED".to_owned(),
        );
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("evil2.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: true,
            skip_workspace: false,
            force: false,
            dry_run: false,
        };
        let err = import_agent(Some(&dir.path().to_path_buf()), &args).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("workspace file path"), "got: {msg}");
    }

    /// Regression for #4241: `--target-id ../escaped` must be rejected
    /// before any file is written, regardless of the file contents.
    #[test]
    fn import_agent_rejects_traversal_target_id() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let agent_file = sample_agent_file();
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("benign.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: Some("../../../../tmp/escaped".to_owned()),
            skip_sessions: true,
            skip_workspace: true,
            force: false,
            dry_run: false,
        };
        let err = import_agent(Some(&dir.path().to_path_buf()), &args).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--target-id") && msg.contains("alphanumeric"),
            "got: {msg}"
        );
    }

    #[test]
    fn import_agent_rejects_duplicate_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let agent_file = sample_agent_file();
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("test.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path.clone(),
            target_id: None,
            skip_sessions: true,
            skip_workspace: true,
            force: false,
            dry_run: false,
        };

        import_agent(Some(&dir.path().to_path_buf()), &args).unwrap();

        let result = import_agent(Some(&dir.path().to_path_buf()), &args);
        assert!(
            result.is_err(),
            "duplicate import without force should fail"
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("already exists"), "got: {msg}");
    }

    #[cfg(feature = "recall")]
    fn seed_typed_knowledge(oikos: &Oikos, nous_id: &str) {
        use mneme::id::{EntityId, FactId};
        use mneme::knowledge::{
            Entity, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance,
            FactSensitivity, FactTemporal, Relationship, Visibility, far_future, parse_timestamp,
        };
        use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

        let knowledge_path = knowledge_path_for_nous(oikos, nous_id);
        let parent = knowledge_path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let store =
            KnowledgeStore::open_fjall(&knowledge_path, KnowledgeConfig::default()).unwrap();

        let fact = Fact {
            id: FactId::new("fact-rt-001").unwrap(),
            nous_id: nous_id.to_owned(),
            content: "Alice likes Rust".to_owned(),
            fact_type: "preference".to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: parse_timestamp("2026-01-01T00:00:00Z").unwrap(),
                valid_to: far_future(),
                recorded_at: parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
            },
            provenance: FactProvenance {
                confidence: 0.95,
                tier: EpistemicTier::Verified,
                source_session_id: None,
                stability_hours: 720.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        };

        let entity1 = Entity {
            id: EntityId::new("entity-rt-001").unwrap(),
            name: "Rust".to_owned(),
            entity_type: "programming_language".to_owned(),
            aliases: vec!["rust-lang".to_owned()],
            created_at: parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
            updated_at: parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
        };

        let entity2 = Entity {
            id: EntityId::new("entity-rt-002").unwrap(),
            name: "Aletheia".to_owned(),
            entity_type: "project".to_owned(),
            aliases: vec![],
            created_at: parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
            updated_at: parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
        };

        let relationship = Relationship {
            src: EntityId::new("entity-rt-001").unwrap(),
            dst: EntityId::new("entity-rt-002").unwrap(),
            relation: "powers".to_owned(),
            weight: 1.0,
            created_at: parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
        };

        store.insert_fact(&fact).unwrap();
        store.insert_entity(&entity1).unwrap();
        store.insert_entity(&entity2).unwrap();
        store.insert_relationship(&relationship).unwrap();
    }

    /// #4163/PR4 — typed knowledge (Fact, Entity, Relationship) round-trips
    /// through export → import.
    #[cfg(feature = "recall")]
    #[test]
    fn roundtrip_preserves_typed_knowledge_4163() {
        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        seed_typed_knowledge(&source_oikos, "alice");

        let export_path = source.path().join("alice.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export_path.clone()),
                archived: false,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let dest = tempfile::tempdir().unwrap();
        let dest_oikos = Oikos::from_root(dest.path());
        std::fs::create_dir_all(dest_oikos.config()).unwrap();
        std::fs::create_dir_all(dest_oikos.data()).unwrap();
        import_agent(
            Some(&dest.path().to_path_buf()),
            &ImportArgs {
                file: export_path,
                target_id: None,
                skip_sessions: false,
                skip_workspace: false,
                force: false,
                dry_run: false,
            },
        )
        .unwrap();

        let knowledge_path = knowledge_path_for_nous(&dest_oikos, "alice");
        let store = mneme::knowledge_store::KnowledgeStore::open_fjall(
            &knowledge_path,
            mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .unwrap();

        let dest_facts: Vec<mneme::knowledge::Fact> = store
            .list_all_facts(i64::MAX)
            .unwrap()
            .into_iter()
            .filter(|f| f.nous_id == "alice")
            .collect();
        assert_eq!(dest_facts.len(), 1, "one fact must be imported");
        let dest_fact = &dest_facts[0];
        assert_eq!(dest_fact.id.as_str(), "fact-rt-001");
        assert_eq!(dest_fact.content, "Alice likes Rust");
        assert_eq!(dest_fact.fact_type, "preference");

        let dest_entities = store.list_entities().unwrap();
        assert_eq!(dest_entities.len(), 2, "two entities must be imported");
        let mut dest_entities_by_id: std::collections::HashMap<&str, &mneme::knowledge::Entity> =
            dest_entities.iter().map(|e| (e.id.as_str(), e)).collect();
        assert_eq!(
            dest_entities_by_id.remove("entity-rt-001").unwrap().name,
            "Rust"
        );
        assert_eq!(
            dest_entities_by_id.remove("entity-rt-002").unwrap().name,
            "Aletheia"
        );
        assert!(dest_entities_by_id.is_empty());

        let dest_relationships = store.list_all_relationships().unwrap();
        assert_eq!(
            dest_relationships.len(),
            1,
            "one relationship must be imported"
        );
        let dest_rel = &dest_relationships[0];
        assert_eq!(dest_rel.src.as_str(), "entity-rt-001");
        assert_eq!(dest_rel.dst.as_str(), "entity-rt-002");
        assert_eq!(dest_rel.relation, "powers");
        assert!((dest_rel.weight - 1.0).abs() < f64::EPSILON);

        // NOTE: HNSW vectors are covered by the #4399 regression below.
    }

    /// #4399 / ADR-006 v2 — import must rebuild the HNSW index from restored facts.
    #[cfg(feature = "recall")]
    #[test]
    fn import_rebuilds_fact_embeddings_4399() {
        use mneme::embedding::{EmbeddingConfig, create_provider};

        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        seed_typed_knowledge(&source_oikos, "alice");

        let export_path = source.path().join("alice.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export_path.clone()),
                archived: false,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let dest = tempfile::tempdir().unwrap();
        let dest_oikos = Oikos::from_root(dest.path());
        std::fs::create_dir_all(dest_oikos.config()).unwrap();
        std::fs::create_dir_all(dest_oikos.data()).unwrap();
        write_agent_config(dest.path(), "alice", "Alice");
        write_mock_embedding_config(dest.path(), 4);

        import_agent(
            Some(&dest.path().to_path_buf()),
            &ImportArgs {
                file: export_path,
                target_id: None,
                skip_sessions: false,
                skip_workspace: false,
                // WHY: write_agent_config above pre-creates the dest nous dir, so
                // import must overwrite it; the [embedding] config still applies.
                force: true,
                dry_run: false,
            },
        )
        .unwrap();

        let knowledge_path = knowledge_path_for_nous(&dest_oikos, "alice");
        let store = mneme::knowledge_store::KnowledgeStore::open_fjall(
            &knowledge_path,
            mneme::knowledge_store::KnowledgeConfig {
                dim: 4,
                ..Default::default()
            },
        )
        .unwrap();

        let provider = create_provider(&EmbeddingConfig {
            provider: "mock".to_owned(),
            model: None,
            dimension: Some(4),
            api_key: None,
            base_url: None,
        })
        .unwrap();
        let query_vec = provider.embed("Alice likes Rust").unwrap();
        let results = store.search_vectors(query_vec, 5, 20).unwrap();

        assert!(
            results.iter().any(|r| r.source_id == "fact-rt-001"),
            "imported fact must be indexed for vector recall: {results:?}"
        );
    }

    /// #4163/PR4 — a full export → import → re-export cycle produces identical
    /// JSON on every field except `exported_at` and `generator`.
    #[cfg(feature = "recall")]
    #[expect(
        clippy::too_many_lines,
        reason = "test needs full source setup, export, import, re-export, and assertion"
    )]
    #[test]
    fn roundtrip_is_byte_stable_4163() {
        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        seed_typed_knowledge(&source_oikos, "alice");

        let source_store = mneme::store::SessionStore::open(&source_oikos.sessions_db()).unwrap();

        // Active session.
        let active_session = source_store
            .create_session("ses-active", "alice", "main", None, Some("mock-model"))
            .unwrap();
        source_store
            .append_message(&active_session.id, Role::User, "active msg", None, None, 10)
            .unwrap();

        // Archived session with distilled + non-distilled messages.
        let archived_session = source_store
            .create_session(
                "ses-archived",
                "alice",
                "archived",
                None,
                Some("mock-model"),
            )
            .unwrap();
        source_store
            .append_message(
                &archived_session.id,
                Role::User,
                "old distilled",
                None,
                None,
                20,
            )
            .unwrap();
        source_store
            .append_message(
                &archived_session.id,
                Role::Assistant,
                "keep this",
                None,
                None,
                30,
            )
            .unwrap();
        source_store
            .mark_messages_distilled(&archived_session.id, &[1])
            .unwrap();
        source_store
            .update_session_status(&archived_session.id, SessionStatus::Archived)
            .unwrap();

        // Working state for active session.
        let ws_value = serde_json::json!({
            "task_stack": [{"description": "ship #4163", "started_at": "2026-05-29T00:00:00Z"}],
            "focus": {"file": "agent_io.rs"}
        });
        let ws_key = format!("ws:alice:{}", active_session.id);
        source_store
            .blackboard_write(&ws_key, &ws_value.to_string(), "alice", 86_400)
            .unwrap();

        drop(source_store);

        let export1 = source.path().join("export1.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export1.clone()),
                archived: true,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let dest = tempfile::tempdir().unwrap();
        let dest_oikos = Oikos::from_root(dest.path());
        std::fs::create_dir_all(dest_oikos.config()).unwrap();
        std::fs::create_dir_all(dest_oikos.data()).unwrap();
        import_agent(
            Some(&dest.path().to_path_buf()),
            &ImportArgs {
                file: export1.clone(),
                target_id: None,
                skip_sessions: false,
                skip_workspace: false,
                force: false,
                dry_run: false,
            },
        )
        .unwrap();

        let export2 = dest.path().join("export2.agent.json");
        export_agent(
            Some(&dest.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export2.clone()),
                archived: true,
                max_messages: 0,
                compact: true,
                force: false,
            },
        )
        .unwrap();

        let mut v1: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&export1).unwrap()).unwrap();
        let mut v2: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&export2).unwrap()).unwrap();

        let obj1 = v1.as_object_mut().unwrap();
        obj1.remove("exportedAt");
        obj1.remove("generator");
        let obj2 = v2.as_object_mut().unwrap();
        obj2.remove("exportedAt");
        obj2.remove("generator");

        assert_eq!(v1, v2, "second export must be identical to first");
    }
}
