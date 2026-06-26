// kanon:ignore RUST/file-too-long — module contains tightly-coupled agent I/O CLI command implementations; splitting would hurt cohesion
//! Agent import/export and skill management commands.

use std::collections::HashMap;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use clap::Args;
use mneme::types::parse_session_or_agent_id;
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

#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI flags — each bool is a distinct switch"
)]
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
    /// Allow the export to succeed when a populated data slot (e.g. typed
    /// knowledge) cannot be enumerated. The output file is marked as partial.
    #[arg(long)]
    pub allow_partial: bool,
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
    /// Skip importing typed knowledge (facts, entities, relationships)
    #[arg(long)]
    pub skip_knowledge: bool,
    /// Overwrite existing workspace files
    #[arg(long)]
    pub force: bool,
    /// Show what would be imported without making changes
    #[arg(long)]
    pub dry_run: bool,
    /// Allow unknown session status, session type, or message role values
    /// to be silently defaulted instead of failing the import.
    #[arg(long)]
    pub allow_unknown_values: bool,
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
    /// Reviewer actor recorded on approve/reject decisions
    #[arg(long)]
    pub reviewer: Option<String>,
    /// Optional review reason recorded on approve/reject decisions
    #[arg(long)]
    pub reason: Option<String>,
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

#[cfg(feature = "recall")]
fn knowledge_config_for_oikos(oikos: &Oikos) -> mneme::knowledge_store::KnowledgeConfig {
    taxis::loader::load_config(oikos).ok().map_or_else(
        mneme::knowledge_store::KnowledgeConfig::default,
        |config| {
            let embedding = mneme::embedding::EmbeddingConfig {
                provider: config.embedding.provider.clone(),
                model: config.embedding.model.clone(),
                dimension: Some(config.embedding.dimension),
                api_key: None,
                base_url: None,
            };
            mneme::knowledge_store::KnowledgeConfig {
                dim: config.embedding.dimension,
                embedding_model: embedding.effective_model_name(),
                ..Default::default()
            }
        },
    )
}

#[derive(Debug, Default)]
struct KnowledgeImportCounts {
    facts: usize,
    entities: usize,
    relationships: usize,
}

/// Enumerate the agent's typed knowledge from the live store.
///
/// Returns `(None, false)` when the `recall` feature is disabled or when no
/// knowledge store exists on disk for this agent. Returns an error when a store
/// exists but cannot be opened or enumerated, so callers can decide whether to
/// fail or produce a partial export. The `bool` is `true` when the store
/// contains vector embeddings that are not included in the typed export and
/// cannot be regenerated solely from the exported facts.
#[cfg(feature = "recall")]
fn export_knowledge(
    oikos: &Oikos,
    nous_id: &str,
) -> Result<(Option<mneme::portability::KnowledgeExport>, bool)> {
    use mneme::knowledge_store::KnowledgeStore;
    use mneme::portability::{FactEntityEdge, KnowledgeExport};

    let knowledge_path = knowledge_path_for_nous(oikos, nous_id);
    if !knowledge_path.exists() {
        return Ok((None, false));
    }

    let config = knowledge_config_for_oikos(oikos);
    let store =
        KnowledgeStore::open_fjall(&knowledge_path, config).with_whatever_context(|_| {
            format!(
                "failed to open knowledge store at {}",
                knowledge_path.display()
            )
        })?;

    // INVARIANT: export arrays must be deterministically ordered so that two
    // consecutive exports of the same agent produce byte-identical JSON after
    // removing the exportedAt/generator header fields (#6015).
    let mut facts: Vec<mneme::knowledge::Fact> = store
        .list_all_facts(i64::MAX)
        .with_whatever_context(|_| format!("failed to list facts for '{nous_id}'"))?
        .into_iter()
        .filter(|f| f.nous_id == nous_id)
        .collect();
    facts.sort_by(|a, b| a.id.cmp(&b.id));

    let fact_ids: Vec<mneme::id::FactId> = facts.iter().map(|fact| fact.id.clone()).collect();
    let mut entities = store
        .list_entities_for_facts(&fact_ids)
        .with_whatever_context(|_| format!("failed to list entities for '{nous_id}'"))?;
    entities.sort_by(|a, b| a.id.cmp(&b.id));

    let mut fact_entity_edges: Vec<FactEntityEdge> = store
        .list_fact_entity_edges_for_facts(&fact_ids)
        .with_whatever_context(|_| format!("failed to list fact/entity links for '{nous_id}'"))?
        .into_iter()
        .map(|(fact_id, entity_id)| FactEntityEdge { fact_id, entity_id })
        .collect();
    fact_entity_edges.sort_by(|a, b| {
        a.fact_id
            .cmp(&b.fact_id)
            .then(a.entity_id.cmp(&b.entity_id))
    });

    let entity_ids: std::collections::HashSet<String> = entities
        .iter()
        .map(|entity| entity.id.as_str().to_owned())
        .collect();
    let mut relationships = store
        .list_relationships_between_entities(&entity_ids)
        .with_whatever_context(|_| format!("failed to list relationships for '{nous_id}'"))?;
    relationships.sort_by(|a, b| {
        a.src
            .cmp(&b.src)
            .then(a.dst.cmp(&b.dst))
            .then(a.relation.cmp(&b.relation))
    });

    // WHY: Fact embeddings are regenerated by backfill_fact_embeddings on import,
    // so they are not a true omission. Only embeddings whose source text is not
    // carried in the typed export (e.g. skill or workspace chunks) are unexportable.
    // Excluding source_type = 'fact' prevents import-time backfill from causing
    // subsequent exports to report a spurious memory omission (#6015).
    let has_unexported_vectors = {
        use std::collections::BTreeMap;
        const HAS_NON_FACT_EMBEDDINGS: &str = "?[id] := *embeddings{id, nous_id: $nous_id, source_type}, source_type != 'fact'\n:limit 1";
        let mut params = BTreeMap::new();
        params.insert(
            "nous_id".to_owned(),
            mneme::engine::DataValue::Str(nous_id.into()),
        );
        store
            .run_script_read_only(HAS_NON_FACT_EMBEDDINGS, params)
            .is_ok_and(|r| r.row_count() > 0)
    };

    Ok((
        Some(KnowledgeExport {
            facts,
            entities,
            relationships,
            fact_entity_edges,
        }),
        has_unexported_vectors,
    ))
}

#[cfg(not(feature = "recall"))]
fn export_knowledge(
    _oikos: &Oikos,
    _nous_id: &str,
) -> Result<(Option<mneme::portability::KnowledgeExport>, bool)> {
    Ok((None, false))
}

/// Hydrate typed knowledge into the live store.
#[cfg(feature = "recall")]
#[expect(
    clippy::too_many_lines,
    reason = "assembles embedding provider, knowledge store, and inserts facts/entities/relationships with backfill"
)]
fn import_knowledge(
    oikos: &Oikos,
    nous_id: &str,
    knowledge: &mneme::portability::KnowledgeExport,
) -> Result<KnowledgeImportCounts> {
    use mneme::embedding::{EmbeddingConfig, create_provider};
    use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

    // WHY(#4741): load_config returns Ok(defaults) even when no config file
    // exists; the candle default provider then downloads BAAI/bge-small-en-v1.5
    // on first use. Gate provider creation behind explicit config-file existence
    // so environments without a deployed instance (test, fresh install) do not
    // block on a network fetch.
    let config_file_exists = oikos.config().join("aletheia.toml").exists()
        || oikos.config().join("aletheia.yaml").exists();
    let loaded_config = if config_file_exists {
        match taxis::loader::load_config(oikos) {
            Ok(config) => Some(config),
            Err(err) => {
                tracing::warn!(
                    nous_id,
                    error = %err,
                    "failed to load instance config; imported fact vectors will be skipped"
                );
                None
            }
        }
    } else {
        None
    };

    let knowledge_config = loaded_config
        .as_ref()
        .map_or_else(KnowledgeConfig::default, |config| KnowledgeConfig {
            dim: config.embedding.dimension,
            embedding_model: EmbeddingConfig {
                provider: config.embedding.provider.clone(),
                model: config.embedding.model.clone(),
                dimension: Some(config.embedding.dimension),
                api_key: None,
                base_url: None,
            }
            .effective_model_name(),
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

    // WHY: retarget every fact's nous_id to the destination agent. Without
    // this rewrite, facts imported via --target-id are stored under the
    // source agent's nous_id and will never surface in recall for the target.
    let facts: Vec<_> = knowledge
        .facts
        .iter()
        .map(|f| {
            if f.nous_id == nous_id {
                f.clone()
            } else {
                let mut retargeted = f.clone();
                nous_id.clone_into(&mut retargeted.nous_id);
                retargeted
            }
        })
        .collect();

    for fact in &facts {
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
    for edge in &knowledge.fact_entity_edges {
        store
            .insert_fact_entity(&edge.fact_id, &edge.entity_id)
            .with_whatever_context(|_| {
                format!(
                    "import fact/entity link {:?} -> {:?}",
                    edge.fact_id, edge.entity_id
                )
            })?;
    }

    if let Some(provider) = embedding_provider.as_ref() {
        let inserted = store.backfill_fact_embeddings(&facts, provider.as_ref());
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
    Ok(KnowledgeImportCounts {
        facts: facts.len(),
        entities: knowledge.entities.len(),
        relationships: knowledge.relationships.len(),
    })
}

#[cfg(not(feature = "recall"))]
fn import_knowledge(
    _oikos: &Oikos,
    _nous_id: &str,
    _knowledge: &mneme::portability::KnowledgeExport,
) -> Result<KnowledgeImportCounts> {
    Ok(KnowledgeImportCounts::default())
}

#[derive(Debug, Default)]
struct ImportSummary {
    workspace_files: usize,
    sessions: usize,
    facts: usize,
    entities: usize,
    relationships: usize,
    skipped_categories: Vec<&'static str>,
}

fn print_import_summary(nous_id: &str, source: &Path, summary: &ImportSummary) {
    println!("Imported agent '{nous_id}' from {}", source.display());
    println!("  Workspace: {} files", summary.workspace_files);
    println!("  Sessions: {}", summary.sessions);
    println!("  Facts: {}", summary.facts);
    println!("  Entities: {}", summary.entities);
    println!("  Relationships: {}", summary.relationships);
    if summary.skipped_categories.is_empty() {
        println!("  Skipped: none");
    } else {
        println!("  Skipped:");
        for category in &summary.skipped_categories {
            println!("    - {category}");
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "agent export assembles config, workspace, sessions, messages, and notes into one portability file"
)]
pub(crate) fn export_agent(instance_root: Option<&PathBuf>, args: &ExportArgs) -> Result<()> {
    use mneme::portability::{
        AgentFile, ExportMetadata, ExportedMessage, ExportedNote, ExportedSession,
        ExportedUsageRecord, NousInfo, OmittedSection, TruncationRecord,
    };

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
    let mut archived_skipped = 0usize;
    let mut truncations: Vec<TruncationRecord> = Vec::new();
    for session in store
        .list_sessions(Some(&args.nous_id))
        .whatever_context("failed to list sessions")?
    {
        if !args.archived && session.status != SessionStatus::Active {
            archived_skipped += 1;
            continue;
        }

        // WHY(#4163): `get_history` filters `is_distilled == true`, dropping
        // the distilled tail. The portability raw entry point returns every
        // row in seq order so an export-then-import round-trip stays faithful.
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
                tool_call_id: msg.tool_call_id,
                tool_name: msg.tool_name,
            })
            .collect();

        if let Some(limit) = limit
            && session.metrics.message_count > limit
        {
            truncations.push(TruncationRecord {
                section: "session_messages".to_owned(),
                item_id: Some(session.id.clone()),
                limit: args.max_messages,
                original: usize::try_from(session.metrics.message_count).ok(),
            });
        }

        let usage_records = store
            .get_usage_for_session(&session.id)
            .with_whatever_context(|_| format!("failed to read usage for {}", session.id))?
            .into_iter()
            .map(|record| ExportedUsageRecord {
                turn_seq: record.turn_seq,
                input_tokens: record.input_tokens,
                output_tokens: record.output_tokens,
                cache_read_tokens: record.cache_read_tokens,
                cache_write_tokens: record.cache_write_tokens,
                model: record.model,
            })
            .collect::<Vec<_>>();

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

        // WHY(#4163): working_state comes from the live blackboard. The key
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
            usage_records: Some(usage_records),
            // WHY(#5783): faithfully round-trip session identity metadata that v1
            // silently dropped; artefact_meta stays excluded (store-internal).
            parent_session_id: session.origin.parent_session_id,
            thread_id: session.origin.thread_id,
            transport: session.origin.transport,
            display_name: session.origin.display_name,
            last_input_tokens: Some(session.metrics.last_input_tokens),
            bootstrap_hash: session.metrics.bootstrap_hash,
            last_distilled_at: session.metrics.last_distilled_at,
            computed_context_tokens: Some(session.metrics.computed_context_tokens),
        });
    }

    // WHY(#4163): top-level knowledge comes from the live store (typed
    // Facts/Entities/Relationships). Vectors + opaque graph (`memory`) are a
    // known v2 gap; the schema slots let them be closed without another
    // version bump.
    let mut omitted_sections: Vec<OmittedSection> = Vec::new();
    if archived_skipped > 0 {
        omitted_sections.push(OmittedSection {
            section: "sessions".to_owned(),
            reason: "archived_excluded".to_owned(),
            count: Some(archived_skipped),
        });
    }

    // WHY(#5102): binary workspace files are enumerated but their contents are
    // not serialized; record them as omitted so importers can warn/reject.
    if !workspace.binary_files.is_empty() {
        omitted_sections.push(OmittedSection {
            section: "workspace_binary_files".to_owned(),
            reason: "binary_content_not_exported".to_owned(),
            count: Some(workspace.binary_files.len()),
        });
    }

    let (knowledge, has_unexported_vectors) = match export_knowledge(&oikos, &args.nous_id) {
        Ok(pair) => pair,
        Err(err) => {
            if args.allow_partial {
                eprintln!(
                    "  WARN: typed knowledge omitted because the knowledge store \
                     could not be opened. Re-run without --allow-partial to fail. \
                     error={err}"
                );
                omitted_sections.push(OmittedSection {
                    section: "knowledge".to_owned(),
                    reason: "store_unavailable".to_owned(),
                    count: None,
                });
                (None, false)
            } else {
                return Err(err);
            }
        }
    };

    // WHY: Only mark the memory section omitted when the store actually contains
    // vector embeddings. Missing or empty stores have no memory data to lose, and
    // exports that carry all facts/entities/relationships are genuinely lossless
    // because fact embeddings are regenerated by backfill on import.
    if has_unexported_vectors {
        omitted_sections.push(OmittedSection {
            section: "memory".to_owned(),
            reason: "vectors_and_graph_not_exported".to_owned(),
            count: None,
        });
    }

    let exported_at = jiff::Timestamp::now().to_string();
    let export_metadata = Some(ExportMetadata {
        lossless: omitted_sections.is_empty() && truncations.is_empty(),
        omitted_sections,
        truncations,
    });
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
        export_metadata,
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
    if let Some(k) = &agent_file.knowledge {
        println!("  Facts: {}", k.facts.len());
        println!("  Entities: {}", k.entities.len());
        println!("  Relationships: {}", k.relationships.len());
    }
    if let Some(meta) = &agent_file.export_metadata {
        if meta.lossless {
            println!("  Export: lossless");
        } else {
            println!("  Export: PARTIAL");
            for omitted in &meta.omitted_sections {
                if let Some(count) = omitted.count {
                    let unit = if count == 1 { "item" } else { "items" };
                    println!(
                        "  Omitted: {} ({}) — {} {}",
                        omitted.section, omitted.reason, count, unit
                    );
                } else {
                    println!("  Omitted: {} ({})", omitted.section, omitted.reason);
                }
            }
            for trunc in &meta.truncations {
                let original = trunc
                    .original
                    .map_or_else(String::new, |o| format!(" (was {o})"));
                println!(
                    "  Truncated: {} for {:?} to {}{}",
                    trunc.section, trunc.item_id, trunc.limit, original
                );
            }
        }
    }

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
    // INVARIANT: binary_files must be sorted so that two exports of the same
    // workspace produce identical arrays regardless of read_dir traversal order.
    binary_files.sort();
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

/// Typed errors raised while validating and importing an agent file.
#[derive(Debug, Snafu)]
enum ImportError {
    #[snafu(display(
        "agent file is version {version} but importer requires v{expected}. \
         Re-export from the source instance; there is no in-place migration. See #4163."
    ))]
    VersionMismatch { version: u32, expected: u32 },

    #[snafu(display("unknown {field} value {value:?} in session {session_id}"))]
    UnknownValue {
        field: &'static str,
        value: String,
        session_id: String,
    },
}

fn import_error(err: &ImportError) -> crate::error::Error {
    crate::error::Error::msg(err.to_string())
}

fn parse_session_status(
    value: &str,
    session_id: &str,
    allow_unknown: bool,
) -> std::result::Result<SessionStatus, ImportError> {
    match value {
        "active" => Ok(SessionStatus::Active),
        "archived" => Ok(SessionStatus::Archived),
        "distilled" => Ok(SessionStatus::Distilled),
        other if allow_unknown => {
            eprintln!("  WARN: unknown status '{other}', defaulting to 'active'");
            Ok(SessionStatus::Active)
        }
        other => Err(ImportError::UnknownValue {
            field: "session_status",
            value: other.to_owned(),
            session_id: session_id.to_owned(),
        }),
    }
}

fn parse_session_type(
    value: &str,
    session_id: &str,
    allow_unknown: bool,
) -> std::result::Result<SessionType, ImportError> {
    match value {
        "primary" => Ok(SessionType::Primary),
        "background" => Ok(SessionType::Background),
        "ephemeral" => Ok(SessionType::Ephemeral),
        other if allow_unknown => {
            eprintln!("  WARN: unknown session_type '{other}', defaulting to 'primary'");
            Ok(SessionType::Primary)
        }
        other => Err(ImportError::UnknownValue {
            field: "session_type",
            value: other.to_owned(),
            session_id: session_id.to_owned(),
        }),
    }
}

fn parse_message_role(
    value: &str,
    session_id: &str,
    allow_unknown: bool,
) -> std::result::Result<Role, ImportError> {
    match value {
        "system" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool_result" => Ok(Role::ToolResult),
        other if allow_unknown => {
            eprintln!("  WARN: unknown role '{other}', defaulting to 'user'");
            Ok(Role::User)
        }
        other => Err(ImportError::UnknownValue {
            field: "message_role",
            value: other.to_owned(),
            session_id: session_id.to_owned(),
        }),
    }
}

/// Validate an agent file's data integrity before making any filesystem changes.
///
/// Catches malformed session IDs, blank session keys, and empty fact content
/// while the operation is still fully reversible. Call this before any I/O so
/// a corrupt export is rejected without leaving partial state on disk.
fn preflight_agent_file(file: &mneme::portability::AgentFile) -> Result<()> {
    for (i, session) in file.sessions.iter().enumerate() {
        if session.id.trim().is_empty() {
            whatever!("session[{i}].id must not be empty");
        }
        if session.session_key.trim().is_empty() {
            whatever!(
                "session[{i}] (id: {:?}) session_key must not be empty",
                session.id
            );
        }
    }

    if let Some(knowledge) = &file.knowledge {
        for (i, fact) in knowledge.facts.iter().enumerate() {
            if fact.content.trim().is_empty() {
                whatever!(
                    "knowledge.facts[{i}] (id: {:?}) content must not be empty",
                    fact.id
                );
            }
        }
    }
    Ok(())
}

/// Report partial-export metadata before any filesystem or store mutation.
///
/// WHY(#5102/#4965): the import must not silently restore an export that the
/// producer already marked as truncated or incomplete. Reporting happens after
/// validation but before the first write so the operator can abort.
fn report_partial_export(file: &mneme::portability::AgentFile) {
    let Some(meta) = &file.export_metadata else {
        return;
    };
    if meta.lossless {
        return;
    }

    eprintln!("  WARN: importing a partial agent export.");
    for omitted in &meta.omitted_sections {
        if let Some(count) = omitted.count {
            eprintln!(
                "    Omitted section: {} — {} ({} items)",
                omitted.section, omitted.reason, count
            );
        } else {
            eprintln!(
                "    Omitted section: {} — {}",
                omitted.section, omitted.reason
            );
        }
    }
    for trunc in &meta.truncations {
        let original = trunc
            .original
            .map_or_else(String::new, |o| format!(" (was {o})"));
        eprintln!(
            "    Truncated: {} for {:?} to {}{}",
            trunc.section, trunc.item_id, trunc.limit, original
        );
    }
}

/// Name of the resume-marker file written inside the nous directory when a
/// session import begins and removed when it completes successfully.
///
/// If this file is present on entry it means a previous import was interrupted
/// mid-session-import. Re-running the import will automatically use force mode
/// for session import so already-persisted sessions are overwritten cleanly
/// instead of returning "session already exists" errors.
const IMPORT_RESUME_MARKER: &str = ".nous-import-in-progress";

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

    // WHY(#5782): reject only FUTURE versions outright; older files upgrade
    // transparently because every additive field carries serde(default).
    if agent_file.version > mneme::portability::AGENT_FILE_VERSION {
        return Err(import_error(&ImportError::VersionMismatch {
            version: agent_file.version,
            expected: mneme::portability::AGENT_FILE_VERSION,
        }));
    }
    if agent_file.version < mneme::portability::AGENT_FILE_VERSION {
        tracing::warn!(
            from_version = agent_file.version,
            to_version = mneme::portability::AGENT_FILE_VERSION,
            "upgrading agent file from an older format version; absent fields default"
        );
    }

    // WARNING(#4241): if --target-id is absent, the imported nous.id is
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
    // WHY: preflight before any I/O so a corrupt export is rejected without
    // leaving partial state on disk (no workspace files partially written,
    // no config half-updated, no orphaned session records).
    preflight_agent_file(&agent_file)?;

    // WHY(#5102/#4965): validate/report partial sections before any mutation.
    report_partial_export(&agent_file);

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
        let facts = agent_file.knowledge.as_ref().map_or(0, |k| k.facts.len());
        let entities = agent_file
            .knowledge
            .as_ref()
            .map_or(0, |k| k.entities.len());
        let relationships = agent_file
            .knowledge
            .as_ref()
            .map_or(0, |k| k.relationships.len());
        println!("Facts: {facts}");
        println!("Entities: {entities}");
        println!("Relationships: {relationships}");
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
    let mut summary = ImportSummary::default();

    let nous_dir = oikos.nous_dir(&nous_id);
    if nous_dir.exists() && !args.force {
        whatever!(
            "nous directory already exists: {}\nUse --force to overwrite workspace files.",
            nous_dir.display()
        );
    }

    // Scaffold workspace from agent file.
    if args.skip_workspace {
        summary.skipped_categories.push("workspace");
    } else {
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
            summary.workspace_files += 1;
        }

        // WHY(#5102): binary workspace files are recorded by path but their
        // contents are not carried in the portability format. Report the omission
        // clearly instead of silently dropping them.
        if !agent_file.workspace.binary_files.is_empty() {
            eprintln!(
                "  WARN: skipping {} binary workspace file(s) (contents not included in export): {:?}",
                agent_file.workspace.binary_files.len(),
                agent_file.workspace.binary_files
            );
            summary.skipped_categories.push("workspace_binary_files");
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
    if args.skip_sessions {
        summary.skipped_categories.push("sessions");
    } else {
        let resume_marker = nous_dir.join(IMPORT_RESUME_MARKER);
        let resuming = resume_marker.exists();
        // WHY: if a previous import was interrupted mid-session-import, the
        // marker file exists. Auto-using force mode lets re-runs overwrite
        // already-partially-imported sessions without requiring the caller to
        // know to pass --force. Silent loss is still prevented because
        // import_session with force=true overwrites rather than deletes.
        let import_force = args.force || resuming;
        if resuming {
            eprintln!("  INFO: resuming interrupted import for '{nous_id}'");
        }
        std::fs::create_dir_all(&nous_dir)
            .with_whatever_context(|_| format!("failed to create {}", nous_dir.display()))?;
        koina::fs::write_restricted(&resume_marker, b"")
            .with_whatever_context(|_| "failed to write import resume marker")?;

        let sessions_db = oikos.sessions_db();
        let store = mneme::store::SessionStore::open(&sessions_db).with_whatever_context(|_| {
            format!("failed to open session store at {}", sessions_db.display())
        })?;

        for session in &agent_file.sessions {
            parse_session_or_agent_id(&session.id).with_whatever_context(|_| {
                format!(
                    "imported session {} uses a reserved internal prefix",
                    session.id
                )
            })?;
            parse_session_or_agent_id(&session.session_key).with_whatever_context(|_| {
                format!("imported session {} has a reserved session_key", session.id)
            })?;

            let status =
                parse_session_status(&session.status, &session.id, args.allow_unknown_values)
                    .map_err(|e| import_error(&e))?;
            let session_type = parse_session_type(
                &session.session_type,
                &session.id,
                args.allow_unknown_values,
            )
            .map_err(|e| import_error(&e))?;

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
                            // WHY(#5783): restore identity metrics from the export;
                            // absent in v1 files, where serde(default) yields None/0.
                            last_input_tokens: session.last_input_tokens.unwrap_or(0),
                            bootstrap_hash: session.bootstrap_hash.clone(),
                            distillation_count: session.distillation_count,
                            last_distilled_at: session.last_distilled_at.clone(),
                            computed_context_tokens: session.computed_context_tokens.unwrap_or(0),
                        },
                        origin: SessionOrigin {
                            // WHY(#5783): faithful round-trip — preserve origin
                            // metadata exactly as exported (including None), so a
                            // re-export is byte-identical (the #4163 fidelity contract).
                            parent_session_id: session.parent_session_id.clone(),
                            thread_id: session.thread_id.clone(),
                            transport: session.transport.clone(),
                            display_name: session.display_name.clone(),
                        },
                        artefact_meta: None,
                    },
                    import_force,
                )
                .with_whatever_context(|_| format!("failed to import session {}", session.id))?;

            for msg in &session.messages {
                let role = parse_message_role(&msg.role, &session.id, args.allow_unknown_values)
                    .map_err(|e| import_error(&e))?;
                store
                    .insert_message_raw(&mneme::types::Message {
                        id: msg.seq,
                        session_id: imported.id.clone(),
                        seq: msg.seq,
                        role,
                        content: msg.content.clone(),
                        tool_call_id: msg.tool_call_id.clone(),
                        tool_name: msg.tool_name.clone(),
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

            if let Some(usage_records) = &session.usage_records {
                for record in usage_records {
                    store
                        .record_usage(&mneme::types::UsageRecord {
                            session_id: imported.id.clone(),
                            turn_seq: record.turn_seq,
                            input_tokens: record.input_tokens,
                            output_tokens: record.output_tokens,
                            cache_read_tokens: record.cache_read_tokens,
                            cache_write_tokens: record.cache_write_tokens,
                            model: record.model.clone(),
                        })
                        .with_whatever_context(|_| {
                            format!(
                                "failed to import usage record turn {} into session {}",
                                record.turn_seq, session.id
                            )
                        })?;
                }
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

            // WHY: import reports success only after the full per-session batch
            // is durable. Otherwise a crash immediately after import can lose
            // rows written after `import_session`.
            store.ensure_durable().with_whatever_context(|_| {
                format!("failed to ensure durability for session {}", session.id)
            })?;

            summary.sessions += 1;
        }

        // Best-effort: remove marker on success. On failure the marker remains
        // so a re-run auto-enables force mode and resumes safely.
        let _ = std::fs::remove_file(&resume_marker); // kanon:ignore RUST/no-silent-result-swallow — best-effort; if removal fails the marker persists so a re-run auto-enables force mode and resumes safely
    }

    // WHY: knowledge import is independent of session import. Previously this
    // was nested inside `if !args.skip_sessions`, which silently dropped all
    // facts when the caller only wanted to skip session history. Knowledge
    // (facts, entities, relationships) is typed data that belongs to the agent
    // regardless of whether session history is restored.
    if args.skip_knowledge {
        summary.skipped_categories.push("knowledge");
    } else if let Some(knowledge) = &agent_file.knowledge {
        let counts = import_knowledge(&oikos, &nous_id, knowledge)?;
        summary.facts = counts.facts;
        summary.entities = counts.entities;
        summary.relationships = counts.relationships;
        #[cfg(not(feature = "recall"))]
        summary.skipped_categories.push("knowledge");
    }

    print_import_summary(&nous_id, &args.file, &summary);

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
        let config = knowledge_config_for_oikos(&oikos);
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

        let config = knowledge_config_for_oikos(&oikos);
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

        let config = knowledge_config_for_oikos(&oikos);
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
                            print_pending_skill_for_review(fact, &ps);
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
                let review = skill_review_input(args)?;
                let new_id = store
                    .approve_pending_skill(&fact_id, nous_id, review)
                    .whatever_context("failed to approve skill")?;
                println!("Approved: {fid} → new skill fact: {new_id}");
            }
            "reject" => {
                let fid = args.fact_id.as_deref().ok_or_else(|| {
                    crate::error::Error::msg("--fact-id required for reject action")
                })?;
                let fact_id = mneme::id::FactId::new(fid).whatever_context("invalid fact id")?;
                let review = skill_review_input(args)?;
                store
                    .reject_pending_skill(&fact_id, nous_id, review)
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

#[cfg(feature = "recall")]
const REVIEW_INPUT_PREVIEW_CHARS: usize = 160;

#[cfg(feature = "recall")]
fn print_pending_skill_for_review(fact: &mneme::knowledge::Fact, ps: &mneme::skills::PendingSkill) {
    // WHY: the review surface is built as a pure String so the provenance it
    // exposes (source session, evidence sessions, sequence hashes, extraction
    // refs, redacted tool input) can be asserted in tests without capturing
    // stdout. Behaviour is identical to printing each line.
    print!("{}", format_pending_skill_for_review(fact, ps));
}

#[cfg(feature = "recall")]
fn format_pending_skill_for_review(
    fact: &mneme::knowledge::Fact,
    ps: &mneme::skills::PendingSkill,
) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(out, "  ID: {}", fact.id).ok();
    writeln!(out, "  Name: {}", ps.skill.name).ok();
    writeln!(
        out,
        "  Description: {}",
        ps.skill.description.lines().next().unwrap_or("")
    )
    .ok();
    writeln!(out, "  Tools: {}", ps.skill.tools_used.join(", ")).ok();
    writeln!(out, "  Tags: {}", ps.skill.domain_tags.join(", ")).ok();
    writeln!(out, "  Steps: {}", ps.skill.steps.len()).ok();
    writeln!(out, "  Status: {}", ps.status).ok();
    writeln!(out, "  Candidate: {}", ps.candidate_id).ok();
    let source_session = ps
        .source_session_id
        .as_deref()
        .or(fact.provenance.source_session_id.as_deref())
        .or_else(|| ps.source_evidence.session_refs.first().map(String::as_str))
        .unwrap_or("unknown");
    writeln!(out, "  Source session: {source_session}").ok();
    if !ps.source_evidence.session_refs.is_empty() {
        writeln!(
            out,
            "  Evidence sessions: {}",
            ps.source_evidence.session_refs.join(", ")
        )
        .ok();
    }
    if !ps.source_evidence.sequence_hashes.is_empty() {
        writeln!(
            out,
            "  Sequence hashes: {}",
            ps.source_evidence.sequence_hashes.join(", ")
        )
        .ok();
    }
    if let Some(ref audit) = ps.extraction_audit {
        writeln!(
            out,
            "  Extraction: prompt {}:{}, response {}:{}",
            audit.user_prompt_ref.algorithm,
            audit.user_prompt_ref.digest,
            audit.response_ref.algorithm,
            audit.response_ref.digest
        )
        .ok();
    }
    if let Some(observation) = ps.source_evidence.observations.first() {
        writeln!(out, "  Evidence tools:").ok();
        for tool in &observation.tool_calls {
            writeln!(out, "{}", format_tool_evidence_for_review(tool)).ok();
        }
    }
    writeln!(out, "  Extracted: {}", ps.extracted_at).ok();
    writeln!(out).ok();
    out
}

#[cfg(feature = "recall")]
fn format_tool_evidence_for_review(tool: &mneme::skills::ToolCallRecord) -> String {
    let input = tool
        .redacted_input
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
        .map_or_else(
            || "{}".to_owned(),
            |value| truncate_for_review(&value, REVIEW_INPUT_PREVIEW_CHARS),
        );
    let result = tool.result_ref.as_ref().map_or_else(
        || "none".to_owned(),
        |ref_| format!("{}:{}", ref_.algorithm, ref_.digest),
    );
    let status = if tool.is_error { "error" } else { "ok" };
    format!(
        "    - {} [{}] input={} result_ref={}",
        tool.tool_name, status, input, result
    )
}

#[cfg(feature = "recall")]
fn skill_review_input(args: &ReviewSkillsArgs) -> Result<mneme::skills::SkillReviewInput> {
    let reviewer = args
        .reviewer
        .as_deref()
        .and_then(non_empty_trimmed)
        .map(str::to_owned)
        .or_else(derive_skill_reviewer)
        .ok_or_else(|| {
            crate::error::Error::msg(
                "reviewer required: pass --reviewer or set ALETHEIA_REVIEWER, GIT_AUTHOR_NAME, USER, or USERNAME",
            )
        })?;
    let reason = args
        .reason
        .as_deref()
        .and_then(non_empty_trimmed)
        .map(str::to_owned);
    Ok(mneme::skills::SkillReviewInput::new(reviewer, reason))
}

#[cfg(feature = "recall")]
fn derive_skill_reviewer() -> Option<String> {
    ["ALETHEIA_REVIEWER", "GIT_AUTHOR_NAME", "USER", "USERNAME"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|value| non_empty_trimmed(&value).map(str::to_owned))
}

#[cfg(feature = "recall")]
fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(feature = "recall")]
fn truncate_for_review(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

// async required when migrate-qdrant feature is enabled; with the feature
// disabled this function has no await points.
#[cfg_attr(
    not(feature = "migrate-qdrant"),
    expect(
        clippy::unused_async,
        reason = "async required when migrate-qdrant feature is enabled"
    )
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
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashMap;
    use std::fmt::Write as _;

    use mneme::portability::{
        AgentFile, ExportMetadata, ExportedMessage, ExportedNote, ExportedSession,
        ExportedUsageRecord, NousInfo, OmittedSection, WorkspaceData,
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
                        tool_call_id: None,
                        tool_name: None,
                    },
                    ExportedMessage {
                        role: "tool_result".to_owned(),
                        content: "tool output".to_owned(),
                        seq: 2,
                        token_estimate: 25,
                        is_distilled: false,
                        created_at: "2026-03-05T10:00:01Z".to_owned(),
                        tool_call_id: Some("call-1".to_owned()),
                        tool_name: Some("read_file".to_owned()),
                    },
                ],
                usage_records: Some(vec![ExportedUsageRecord {
                    turn_seq: 1,
                    input_tokens: 50,
                    output_tokens: 100,
                    cache_read_tokens: 3,
                    cache_write_tokens: 4,
                    model: Some("claude-sonnet-4-6".to_owned()),
                }]),
                parent_session_id: None,
                thread_id: None,
                transport: None,
                display_name: None,
                last_input_tokens: None,
                bootstrap_hash: None,
                last_distilled_at: None,
                computed_context_tokens: None,
            }],
            memory: None,
            knowledge: None,
            export_metadata: None,
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
            .append_message(
                &session.id,
                Role::ToolResult,
                "tool output",
                Some("call-export"),
                Some("read_file"),
                7,
            )
            .unwrap();
        store
            .record_usage(&mneme::types::UsageRecord {
                session_id: session.id.clone(),
                turn_seq: 1,
                input_tokens: 5,
                output_tokens: 7,
                cache_read_tokens: 2,
                cache_write_tokens: 3,
                model: Some("mock-model".to_owned()),
            })
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
            allow_partial: false,
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
        assert_eq!(
            exported.sessions[0].messages[1].tool_call_id.as_deref(),
            Some("call-export")
        );
        assert_eq!(
            exported.sessions[0].messages[1].tool_name.as_deref(),
            Some("read_file")
        );
        let usage_records = exported.sessions[0]
            .usage_records
            .as_ref()
            .expect("usage records exported");
        assert_eq!(usage_records.len(), 1);
        assert_eq!(usage_records[0].input_tokens, 5);
        assert_eq!(usage_records[0].cache_write_tokens, 3);
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
                allow_partial: false,
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
                skip_knowledge: false,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
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

    #[test]
    fn import_agent_rejects_reserved_cross_session_key_without_persisting() {
        let dest = tempfile::tempdir().unwrap();
        let dest_oikos = Oikos::from_root(dest.path());
        std::fs::create_dir_all(dest_oikos.config()).unwrap();
        std::fs::create_dir_all(dest_oikos.data()).unwrap();

        let mut agent_file = sample_agent_file();
        agent_file.sessions[0].session_key = "cross:victim".to_owned();
        let import_path = dest.path().join("reserved.agent.json");
        std::fs::write(
            &import_path,
            serde_json::to_string_pretty(&agent_file).unwrap(),
        )
        .unwrap();

        let result = import_agent(
            Some(&dest.path().to_path_buf()),
            &ImportArgs {
                file: import_path,
                target_id: None,
                skip_sessions: false,
                skip_workspace: false,
                skip_knowledge: false,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
            },
        );

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("reserved session_key"), "got: {msg}");

        let store = mneme::store::SessionStore::open(&dest_oikos.sessions_db()).unwrap();
        let persisted = store
            .find_session("imported-agent", "cross:victim")
            .unwrap();
        assert!(
            persisted.is_none(),
            "import must not persist sessions with reserved session keys"
        );
    }

    /// WHY(#4163): import preserves session status, timestamps, and metrics
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
                allow_partial: false,
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
                skip_knowledge: false,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
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

    /// WHY(#4163): import preserves per-message `seq`, `is_distilled`, and
    /// `created_at` via [`insert_message_raw`].
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "regression test exercises full export/import metadata fidelity"
    )]
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
            .append_message(
                &session.id,
                Role::ToolResult,
                "msg two",
                Some("call-meta"),
                Some("read_file"),
                20,
            )
            .unwrap();
        source_store
            .append_message(&session.id, Role::User, "msg three", None, None, 30)
            .unwrap();
        source_store
            .record_usage(&mneme::types::UsageRecord {
                session_id: session.id.clone(),
                turn_seq: 1,
                input_tokens: 40,
                output_tokens: 20,
                cache_read_tokens: 1,
                cache_write_tokens: 2,
                model: Some("mock-model".to_owned()),
            })
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
                allow_partial: false,
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
                skip_knowledge: false,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
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
            assert_eq!(
                dst.tool_call_id, src.tool_call_id,
                "#4799: tool_call_id must be preserved for seq {}",
                src.seq
            );
            assert_eq!(
                dst.tool_name, src.tool_name,
                "#4799: tool_name must be preserved for seq {}",
                src.seq
            );
        }
        let usage_records = dest_store.get_usage_for_session(&session.id).unwrap();
        assert_eq!(usage_records.len(), 1, "#4799: usage record imported");
        assert_eq!(usage_records[0].input_tokens, 40);
        assert_eq!(usage_records[0].cache_write_tokens, 2);
    }

    /// WHY(#4163): regression — `export_agent` must read session history via
    /// `get_history_raw` so distilled messages survive the export.
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
                allow_partial: false,
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

    /// WHY(#4163): regression — `export_agent` reads the per-session working
    /// state from the blackboard at `ws:{nous_id}:{session_id}` and serializes
    /// it into the `workingState` slot; a hardcoded `None` here silently drops
    /// task stack / focus state on round-trip.
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
                allow_partial: false,
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
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
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

        // WHY: reopening the store exercises import durability before any
        // unrelated durable write can mask missing imported history.
        let store = mneme::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let sessions = store.list_sessions(Some("imported-agent")).unwrap();
        assert_eq!(sessions.len(), 1, "one session should be imported");

        let history = store.get_history_raw(&sessions[0].id, None).unwrap();
        assert_eq!(
            history.len(),
            2,
            "two messages should be recoverable after reopen"
        );
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[1].content, "tool output");
    }

    #[test]
    fn import_agent_skips_sessions_when_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let mut agent_file = sample_agent_file();
        #[cfg(feature = "recall")]
        {
            agent_file.knowledge = Some(sample_knowledge_export("imported-agent"));
        }
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("test.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: true,
            skip_workspace: false,
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
        };

        import_agent(Some(&dir.path().to_path_buf()), &args).unwrap();

        let store = mneme::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let sessions = store.list_sessions(Some("imported-agent")).unwrap();
        assert!(sessions.is_empty(), "sessions should be skipped");

        #[cfg(feature = "recall")]
        {
            let facts = imported_facts(&oikos, "imported-agent");
            assert_eq!(facts.len(), 1, "knowledge facts should still import");
            assert_eq!(facts[0].content, "Imported agent prefers Rust");
        }
    }

    #[cfg(feature = "recall")]
    #[test]
    fn import_agent_retargets_fact_nous_id_to_target_id() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let mut agent_file = sample_agent_file();
        agent_file.knowledge = Some(mneme::portability::KnowledgeExport {
            facts: vec![
                sample_fact("fact-retarget-001", "imported-agent", "first imported fact"),
                sample_fact(
                    "fact-retarget-002",
                    "imported-agent",
                    "second imported fact",
                ),
            ],
            entities: vec![],
            relationships: vec![],
            fact_entity_edges: vec![],
        });
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("retarget.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: Some("dest-nous".to_owned()),
            skip_sessions: true,
            skip_workspace: true,
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
        };

        import_agent(Some(&dir.path().to_path_buf()), &args).unwrap();

        let facts = imported_facts(&oikos, "dest-nous");
        assert_eq!(facts.len(), 2, "all facts should import for target nous");
        assert!(
            facts.iter().all(|fact| fact.nous_id == "dest-nous"),
            "every fact must be rewritten to the target nous"
        );
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
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
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
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
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
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
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
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
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
    fn sample_fact(id: &str, nous_id: &str, content: &str) -> mneme::knowledge::Fact {
        use mneme::knowledge::{
            EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
            FactTemporal, Visibility, far_future, parse_timestamp,
        };

        Fact {
            id: mneme::id::FactId::new(id).unwrap(),
            nous_id: nous_id.to_owned(),
            content: content.to_owned(),
            fact_type: "test".to_owned(),
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
        }
    }

    #[cfg(feature = "recall")]
    fn sample_knowledge_export(nous_id: &str) -> mneme::portability::KnowledgeExport {
        mneme::portability::KnowledgeExport {
            facts: vec![sample_fact(
                "fact-import-001",
                nous_id,
                "Imported agent prefers Rust",
            )],
            entities: vec![],
            relationships: vec![],
            fact_entity_edges: vec![],
        }
    }

    #[cfg(feature = "recall")]
    fn imported_facts(oikos: &Oikos, nous_id: &str) -> Vec<mneme::knowledge::Fact> {
        let knowledge_path = knowledge_path_for_nous(oikos, nous_id);
        let store = mneme::knowledge_store::KnowledgeStore::open_fjall(
            &knowledge_path,
            mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .unwrap();
        store
            .list_all_facts(i64::MAX)
            .unwrap()
            .into_iter()
            .filter(|fact| fact.nous_id == nous_id)
            .collect()
    }

    #[cfg(feature = "recall")]
    fn sample_entity(id: &str, name: &str) -> mneme::knowledge::Entity {
        mneme::knowledge::Entity {
            id: mneme::id::EntityId::new(id).unwrap(),
            name: name.to_owned(),
            entity_type: "topic".to_owned(),
            aliases: vec![],
            created_at: mneme::knowledge::parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
            updated_at: mneme::knowledge::parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
        }
    }

    #[cfg(feature = "recall")]
    fn link_fact_entity(
        store: &mneme::knowledge_store::KnowledgeStore,
        fact_id: &str,
        entity_id: &str,
    ) {
        let script = r"
            ?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]
            :put fact_entities {fact_id, entity_id => created_at}
        ";
        let mut params = std::collections::BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            mneme::engine::DataValue::Str(fact_id.into()),
        );
        params.insert(
            "entity_id".to_owned(),
            mneme::engine::DataValue::Str(entity_id.into()),
        );
        params.insert(
            "created_at".to_owned(),
            mneme::engine::DataValue::Str("2026-03-01T00:00:00Z".into()),
        );
        store
            .run_mut_query(script, params)
            .expect("link fact to entity");
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
        link_fact_entity(&store, fact.id.as_str(), entity1.id.as_str());
        link_fact_entity(&store, fact.id.as_str(), entity2.id.as_str());
    }

    /// WHY(#4163): typed knowledge (Fact, Entity, Relationship) round-trips
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
                allow_partial: false,
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
                skip_knowledge: false,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
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

        let dest_edges = store
            .list_fact_entity_edges_for_facts(std::slice::from_ref(&dest_fact.id))
            .unwrap();
        let mut dest_edge_pairs: Vec<(&str, &str)> = dest_edges
            .iter()
            .map(|(fact_id, entity_id)| (fact_id.as_str(), entity_id.as_str()))
            .collect();
        dest_edge_pairs.sort_unstable();
        assert_eq!(
            dest_edge_pairs,
            vec![
                ("fact-rt-001", "entity-rt-001"),
                ("fact-rt-001", "entity-rt-002")
            ]
        );

        // NOTE: HNSW vectors are covered by the #4399 regression below.
    }

    #[cfg(feature = "recall")]
    #[test]
    fn export_agent_scopes_knowledge_graph_to_exported_facts() {
        let source = tempfile::tempdir().unwrap();
        let source_oikos = Oikos::from_root(source.path());
        write_agent_config(source.path(), "alice", "Alice");
        std::fs::write(source_oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let knowledge_path = knowledge_path_for_nous(&source_oikos, "alice");
        let parent = knowledge_path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let store = mneme::knowledge_store::KnowledgeStore::open_fjall(
            &knowledge_path,
            mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .unwrap();

        let alice_fact = {
            let mut fact = sample_fact("agent-export-alice-fact", "alice", "alice fact");
            fact.visibility = mneme::knowledge::Visibility::Private;
            fact
        };
        let bob_fact = {
            let mut fact = sample_fact("agent-export-bob-fact", "bob", "bob fact");
            fact.visibility = mneme::knowledge::Visibility::Private;
            fact
        };
        let alice_entity = sample_entity("agent-export-alice-entity", "Alice Entity");
        let bob_entity = sample_entity("agent-export-bob-entity", "Bob Entity");

        store.insert_fact(&alice_fact).unwrap();
        store.insert_fact(&bob_fact).unwrap();
        store.insert_entity(&alice_entity).unwrap();
        store.insert_entity(&bob_entity).unwrap();
        link_fact_entity(&store, alice_fact.id.as_str(), alice_entity.id.as_str());
        link_fact_entity(&store, bob_fact.id.as_str(), bob_entity.id.as_str());
        store
            .insert_relationship(&mneme::knowledge::Relationship {
                src: alice_entity.id.clone(),
                dst: bob_entity.id.clone(),
                relation: "crosses".to_owned(),
                weight: 0.9,
                created_at: mneme::knowledge::parse_timestamp("2026-03-01T00:00:00Z").unwrap(),
            })
            .unwrap();
        drop(store);

        let export_path = source.path().join("alice-scoped.agent.json");
        export_agent(
            Some(&source.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(export_path.clone()),
                archived: false,
                max_messages: 0,
                compact: true,
                force: false,
                allow_partial: false,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(export_path).unwrap()).unwrap();
        let knowledge = exported.knowledge.expect("knowledge export present");
        let entity_ids: Vec<&str> = knowledge
            .entities
            .iter()
            .map(|entity| entity.id.as_str())
            .collect();

        assert_eq!(knowledge.facts.len(), 1);
        assert_eq!(knowledge.facts[0].id.as_str(), "agent-export-alice-fact");
        assert_eq!(knowledge.fact_entity_edges.len(), 1);
        assert_eq!(
            knowledge.fact_entity_edges[0].fact_id.as_str(),
            "agent-export-alice-fact"
        );
        assert_eq!(
            knowledge.fact_entity_edges[0].entity_id.as_str(),
            "agent-export-alice-entity"
        );
        assert!(entity_ids.contains(&"agent-export-alice-entity"));
        assert!(
            !entity_ids.contains(&"agent-export-bob-entity"),
            "foreign entity must not be exported"
        );
        assert!(
            knowledge.relationships.is_empty(),
            "relationship touching a foreign entity must not be exported"
        );
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
                allow_partial: false,
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
                skip_knowledge: false,
                // WHY: write_agent_config above pre-creates the dest nous dir, so
                // import must overwrite it; the [embedding] config still applies.
                force: true,
                dry_run: false,
                allow_unknown_values: false,
            },
        )
        .unwrap();

        let knowledge_path = knowledge_path_for_nous(&dest_oikos, "alice");
        let store = mneme::knowledge_store::KnowledgeStore::open_fjall(
            &knowledge_path,
            mneme::knowledge_store::KnowledgeConfig {
                dim: 4,
                embedding_model: "mock-embedding".to_owned(),
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

    /// WHY(#4163): a full export → import → re-export cycle produces identical
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
                allow_partial: false,
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
                skip_knowledge: false,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
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
                allow_partial: false,
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

    fn minimal_agent_file() -> AgentFile {
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
                files: HashMap::new(),
                binary_files: vec![],
            },
            sessions: vec![],
            memory: None,
            knowledge: None,
            export_metadata: None,
        }
    }

    fn single_session_agent_file(status: &str, session_type: &str, role: &str) -> AgentFile {
        let mut file = minimal_agent_file();
        file.sessions = vec![ExportedSession {
            id: "ses-001".to_owned(),
            session_key: "main".to_owned(),
            status: status.to_owned(),
            session_type: session_type.to_owned(),
            message_count: 1,
            token_count_estimate: 10,
            distillation_count: 0,
            created_at: "2026-03-05T10:00:00Z".to_owned(),
            updated_at: "2026-03-05T11:00:00Z".to_owned(),
            working_state: None,
            distillation_priming: None,
            notes: vec![],
            messages: vec![ExportedMessage {
                role: role.to_owned(),
                content: "hello".to_owned(),
                seq: 1,
                token_estimate: 10,
                is_distilled: false,
                created_at: "2026-03-05T10:00:00Z".to_owned(),
                tool_call_id: None,
                tool_name: None,
            }],
            usage_records: None,
            parent_session_id: None,
            thread_id: None,
            transport: None,
            display_name: None,
            last_input_tokens: None,
            bootstrap_hash: None,
            last_distilled_at: None,
            computed_context_tokens: None,
        }];
        file
    }

    #[test]
    fn import_rejects_future_version() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let mut agent_file = minimal_agent_file();
        agent_file.version = mneme::portability::AGENT_FILE_VERSION + 1;
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("future.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: true,
            skip_workspace: true,
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
        };
        let err = import_agent(Some(&dir.path().to_path_buf()), &args).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("agent file is version"), "got: {msg}");
        assert!(msg.contains("requires v"), "got: {msg}");
    }

    #[test]
    fn import_accepts_v1_file() {
        // WHY(#5782): an older-version file must upgrade transparently rather than
        // hard-reject — additive fields carry serde(default).
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let mut agent_file = minimal_agent_file();
        agent_file.version = mneme::portability::AGENT_FILE_VERSION - 1;
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("v1.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: true,
            skip_workspace: true,
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
        };
        import_agent(Some(&dir.path().to_path_buf()), &args)
            .expect("older-version agent file should import after transparent upgrade");
    }

    #[test]
    fn import_rejects_unknown_session_status() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let agent_file = single_session_agent_file("bad-status", "primary", "user");
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("bad-status.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: false,
            skip_workspace: true,
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
        };
        let err = import_agent(Some(&dir.path().to_path_buf()), &args).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown session_status"), "got: {msg}");
    }

    #[test]
    fn import_rejects_unknown_session_type() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let agent_file = single_session_agent_file("active", "bad-type", "user");
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("bad-type.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: false,
            skip_workspace: true,
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
        };
        let err = import_agent(Some(&dir.path().to_path_buf()), &args).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown session_type"), "got: {msg}");
    }

    #[test]
    fn import_rejects_unknown_message_role() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let agent_file = single_session_agent_file("active", "primary", "bad-role");
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("bad-role.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: false,
            skip_workspace: true,
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: false,
        };
        let err = import_agent(Some(&dir.path().to_path_buf()), &args).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown message_role"), "got: {msg}");
    }

    #[test]
    fn import_accepts_unknown_values_with_flag() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let agent_file = single_session_agent_file("bad-status", "bad-type", "bad-role");
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("lossy.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        let args = ImportArgs {
            file: agent_path,
            target_id: None,
            skip_sessions: false,
            skip_workspace: true,
            skip_knowledge: false,
            force: false,
            dry_run: false,
            allow_unknown_values: true,
        };
        import_agent(Some(&dir.path().to_path_buf()), &args).unwrap();

        let store = mneme::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let sessions = store.list_sessions(Some("imported-agent")).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, SessionStatus::Active);
        assert_eq!(sessions[0].session_type, SessionType::Primary);
        let history = store.get_history(&sessions[0].id, None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, Role::User);
    }

    /// WHY(#5102/#4965): an existing knowledge store that cannot be opened must
    /// fail the export by default; the operator can opt into a partial export
    /// with `--allow-partial`.
    #[cfg(feature = "recall")]
    #[test]
    fn export_fails_on_unopenable_knowledge_store_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");
        std::fs::write(oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        // Create a file where the knowledge store directory should be.
        let knowledge_path = knowledge_path_for_nous(&oikos, "alice");
        std::fs::create_dir_all(knowledge_path.parent().unwrap()).unwrap();
        std::fs::write(&knowledge_path, b"not a fjall database").unwrap();

        let output = dir.path().join("alice.agent.json");
        let result = export_agent(
            Some(&dir.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(output.clone()),
                archived: false,
                max_messages: 0,
                compact: false,
                force: false,
                allow_partial: false,
            },
        );

        assert!(
            result.is_err(),
            "default export must fail when knowledge store cannot be opened"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("knowledge store") || msg.contains("failed to open"),
            "got: {msg}"
        );
        assert!(
            !output.exists(),
            "partial file must not be written when --allow-partial is not set"
        );
    }

    /// WHY(#5102/#4965): `--allow-partial` lets the export succeed and records
    /// the knowledge omission in machine-readable metadata.
    #[cfg(feature = "recall")]
    #[test]
    fn export_allows_partial_for_unopenable_knowledge_store() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");
        std::fs::write(oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let knowledge_path = knowledge_path_for_nous(&oikos, "alice");
        std::fs::create_dir_all(knowledge_path.parent().unwrap()).unwrap();
        std::fs::write(&knowledge_path, b"not a fjall database").unwrap();

        let output = dir.path().join("alice.agent.json");
        export_agent(
            Some(&dir.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(output.clone()),
                archived: false,
                max_messages: 0,
                compact: false,
                force: false,
                allow_partial: true,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
        let meta = exported
            .export_metadata
            .expect("partial export must include metadata");
        assert!(
            !meta.lossless,
            "partial export must not claim to be lossless"
        );
        let knowledge_omitted = meta
            .omitted_sections
            .iter()
            .any(|o| o.section == "knowledge" && o.reason == "store_unavailable");
        assert!(knowledge_omitted, "metadata must record knowledge omission");
    }

    /// WHY(#5102/#4965): a missing knowledge store is different from an
    /// unopenable store. It produces no knowledge slot but is still lossless.
    #[cfg(feature = "recall")]
    #[test]
    fn export_succeeds_when_knowledge_store_missing() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");
        std::fs::write(oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let output = dir.path().join("alice.agent.json");
        export_agent(
            Some(&dir.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(output.clone()),
                archived: false,
                max_messages: 0,
                compact: false,
                force: false,
                allow_partial: false,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
        assert!(
            exported.knowledge.is_none(),
            "missing store => no knowledge slot"
        );
        let meta = exported.export_metadata.expect("export metadata present");
        assert!(
            meta.lossless,
            "missing knowledge store is not a partial export"
        );
        assert!(meta.omitted_sections.is_empty());
    }

    /// WHY(#5102/#4965): an empty knowledge store is explicitly represented as
    /// an empty object so consumers can distinguish "present but empty" from
    /// "never configured".
    #[cfg(feature = "recall")]
    #[test]
    fn export_succeeds_when_knowledge_store_empty() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");
        std::fs::write(oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();

        let knowledge_path = knowledge_path_for_nous(&oikos, "alice");
        std::fs::create_dir_all(knowledge_path.parent().unwrap()).unwrap();
        mneme::knowledge_store::KnowledgeStore::open_fjall(
            &knowledge_path,
            mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .unwrap();

        let output = dir.path().join("alice.agent.json");
        export_agent(
            Some(&dir.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(output.clone()),
                archived: false,
                max_messages: 0,
                compact: false,
                force: false,
                allow_partial: false,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
        let knowledge = exported
            .knowledge
            .expect("empty store => empty knowledge object");
        assert!(knowledge.facts.is_empty());
        assert!(knowledge.entities.is_empty());
        assert!(knowledge.relationships.is_empty());
        let meta = exported.export_metadata.expect("export metadata present");
        assert!(meta.lossless, "empty knowledge store is still lossless");
    }

    /// WHY(#5102/#4965): a populated knowledge store is exported with counts
    /// and marked lossless.
    #[cfg(feature = "recall")]
    #[test]
    fn export_records_typed_knowledge_counts() {
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
                allow_partial: false,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
        let knowledge = exported.knowledge.expect("knowledge present");
        assert_eq!(knowledge.facts.len(), 1);
        assert_eq!(knowledge.entities.len(), 2);
        assert_eq!(knowledge.relationships.len(), 1);
        let meta = exported.export_metadata.expect("metadata present");
        assert!(meta.lossless);
    }

    /// WHY(#5102/#4965): `--max-messages` truncation is recorded in metadata.
    #[test]
    fn export_records_message_truncation_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");

        let store = mneme::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let session = store
            .create_session("ses-trunc", "alice", "main", None, Some("mock-model"))
            .unwrap();
        for i in 0..5 {
            store
                .append_message(&session.id, Role::User, &format!("msg {i}"), None, None, 1)
                .unwrap();
        }
        drop(store);

        let output = dir.path().join("alice.agent.json");
        export_agent(
            Some(&dir.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(output.clone()),
                archived: false,
                max_messages: 2,
                compact: false,
                force: false,
                allow_partial: false,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
        assert_eq!(exported.sessions[0].messages.len(), 2);
        let meta = exported.export_metadata.expect("metadata present");
        assert!(!meta.lossless);
        let trunc = meta
            .truncations
            .iter()
            .find(|t| t.section == "session_messages" && t.item_id.as_deref() == Some(&session.id))
            .expect("truncation recorded");
        assert_eq!(trunc.limit, 2);
        assert_eq!(trunc.original, Some(5));
    }

    /// WHY(#5102/#4965): excluding archived sessions is no longer silent; the
    /// export metadata records the omission.
    #[test]
    fn export_records_archived_session_omission() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");

        let store = mneme::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let active = store
            .create_session("ses-active", "alice", "main", None, Some("mock-model"))
            .unwrap();
        store
            .append_message(&active.id, Role::User, "active", None, None, 1)
            .unwrap();
        let archived = store
            .create_session(
                "ses-archived",
                "alice",
                "archived",
                None,
                Some("mock-model"),
            )
            .unwrap();
        store
            .append_message(&archived.id, Role::User, "archived", None, None, 1)
            .unwrap();
        store
            .update_session_status(&archived.id, SessionStatus::Archived)
            .unwrap();
        drop(store);

        let output = dir.path().join("alice.agent.json");
        export_agent(
            Some(&dir.path().to_path_buf()),
            &ExportArgs {
                nous_id: "alice".to_owned(),
                output: Some(output.clone()),
                archived: false,
                max_messages: 0,
                compact: false,
                force: false,
                allow_partial: false,
            },
        )
        .unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
        assert_eq!(exported.sessions.len(), 1);
        let meta = exported.export_metadata.expect("metadata present");
        assert!(!meta.lossless);
        let omitted = meta
            .omitted_sections
            .iter()
            .find(|o| o.section == "sessions" && o.reason == "archived_excluded")
            .expect("archived omission recorded");
        assert_eq!(omitted.count, Some(1));
    }

    /// WHY(#5102/#4965): import validates and reports partial exports before
    /// mutating the destination instance, while preserving the existing import
    /// behavior for the data that is present.
    #[test]
    fn import_reports_partial_export_metadata_before_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        std::fs::create_dir_all(oikos.config()).unwrap();
        std::fs::create_dir_all(oikos.data()).unwrap();

        let mut agent_file = minimal_agent_file();
        agent_file.export_metadata = Some(ExportMetadata {
            lossless: false,
            omitted_sections: vec![OmittedSection {
                section: "knowledge".to_owned(),
                reason: "store_unavailable".to_owned(),
                count: None,
            }],
            truncations: vec![],
        });
        let json = serde_json::to_string(&agent_file).unwrap();
        let agent_path = dir.path().join("partial.agent.json");
        std::fs::write(&agent_path, json).unwrap();

        import_agent(
            Some(&dir.path().to_path_buf()),
            &ImportArgs {
                file: agent_path,
                target_id: None,
                skip_sessions: true,
                skip_workspace: true,
                skip_knowledge: false,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
            },
        )
        .unwrap();

        // Existing behavior: config entry is written even for a partial export.
        let config = std::fs::read_to_string(oikos.config().join("aletheia.toml")).unwrap();
        assert!(config.contains(r#"id = "imported-agent""#));
    }

    /// Criterion 5: the `review-skills list` surface must expose enough
    /// provenance for a human to decide — source session, evidence sessions,
    /// sequence hashes, extraction prompt/response refs, and per-tool redacted
    /// input + result reference — without leaking redacted secret values.
    #[cfg(feature = "recall")]
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive provenance fixture plus full review-surface assertions"
    )]
    fn review_skills_list_renders_full_provenance_without_leaking_secrets() {
        use episteme::skills::{
            CandidateTracker, ContentEvidenceRef, ExtractedSkill, PendingSkill,
            SkillExtractionAudit, ToolCallRecord,
        };
        use mneme::knowledge::{
            EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
            FactTemporal, Visibility,
        };

        // Build a candidate on the live tracker path so its evidence carries a
        // real sequence hash and redacted tool input rather than hand-built
        // structs.
        let secret = "super-secret-token-value";
        let tool_calls = vec![
            ToolCallRecord::new("Grep", 10).with_evidence(
                "t0",
                &serde_json::json!({ "pattern": "needle" }),
                Some("hits"),
                Some("receipt-0"),
            ),
            ToolCallRecord::new("Read", 10),
            ToolCallRecord::new("Read", 10),
            ToolCallRecord::new("Edit", 10).with_evidence(
                "t3",
                &serde_json::json!({ "api_key": secret }),
                Some("patched"),
                Some("receipt-3"),
            ),
            ToolCallRecord::new("Bash", 10),
            ToolCallRecord::new("Bash", 10),
        ];
        let tracker = CandidateTracker::new();
        tracker.track_sequence(&tool_calls, "session-alpha", "review-nous");
        let candidate = tracker
            .candidates_for("review-nous")
            .pop()
            .expect("candidate tracked");
        let seq_hash = candidate
            .evidence
            .first()
            .expect("observation evidence present")
            .sequence_hash
            .clone();
        assert!(!seq_hash.is_empty(), "observation carries a sequence hash");

        let extracted = ExtractedSkill {
            name: "diagnose-and-patch".to_owned(),
            description: "Diagnose a failure then patch it".to_owned(),
            steps: vec!["grep".to_owned(), "read".to_owned(), "edit".to_owned()],
            tools_used: vec!["Grep".to_owned(), "Read".to_owned(), "Edit".to_owned()],
            domain_tags: vec!["debugging".to_owned()],
            when_to_use: "when fixing bugs".to_owned(),
        };
        let audit = SkillExtractionAudit {
            model: Some("haiku-test".to_owned()),
            system_prompt_ref: ContentEvidenceRef::sha256("extraction_system_prompt", "system"),
            user_prompt_ref: ContentEvidenceRef::sha256("extraction_user_prompt", "user prompt"),
            response_ref: ContentEvidenceRef::sha256("extraction_response", "response body"),
            extracted_at: jiff::Timestamp::now(),
        };
        let pending = PendingSkill::new_with_provenance(&extracted, &candidate, audit);

        let now = jiff::Timestamp::now();
        let fact = Fact {
            id: mneme::id::FactId::new("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("valid fact id"),
            nous_id: "review-nous".to_owned(),
            content: pending.to_json().expect("pending serializes"),
            fact_type: "skill_pending".to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: now,
                valid_to: now,
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence: 0.6,
                tier: EpistemicTier::Inferred,
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

        // Exercise the exact `review-skills list` rendering path: parse the
        // fact content back, then format it for review.
        let parsed = PendingSkill::from_json(&fact.content).expect("pending deserializes");
        let rendered = format_pending_skill_for_review(&fact, &parsed);

        assert!(
            rendered.contains("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            "fact id surfaced: {rendered}"
        );
        assert!(
            rendered.contains("Source session: session-alpha"),
            "source session surfaced: {rendered}"
        );
        assert!(
            rendered.contains("Evidence sessions: session-alpha"),
            "evidence session surfaced: {rendered}"
        );
        assert!(
            rendered.contains(&seq_hash),
            "sequence hash surfaced: {rendered}"
        );
        assert!(
            rendered.contains("Extraction: prompt sha256:"),
            "extraction prompt/response refs surfaced: {rendered}"
        );
        assert!(
            rendered.contains("[REDACTED]"),
            "redacted tool input surfaced: {rendered}"
        );
        assert!(
            rendered.contains("result_ref=sha256:"),
            "tool result reference surfaced: {rendered}"
        );
        assert!(
            !rendered.contains(secret),
            "secret value must not leak into the review surface: {rendered}"
        );
    }

    /// WHY(#5102): binary workspace files are enumerated but not serialized, so
    /// the export metadata must record the omission instead of claiming losslessness.
    #[test]
    fn export_records_binary_workspace_files_and_memory_omissions_5102() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        write_agent_config(dir.path(), "alice", "Alice");
        std::fs::write(oikos.nous_dir("alice").join("SOUL.md"), "# Alice\n").unwrap();
        // WHY: PNG magic bytes are not valid UTF-8, so the collector treats this
        // as a binary file and records only its path.
        std::fs::write(
            oikos.nous_dir("alice").join("avatar.png"),
            [0x89, 0x50, 0x4e, 0x47],
        )
        .unwrap();

        let output = dir.path().join("alice.agent.json");
        let args = ExportArgs {
            nous_id: "alice".to_owned(),
            output: Some(output.clone()),
            archived: false,
            max_messages: 0,
            compact: false,
            force: false,
            allow_partial: false,
        };
        export_agent(Some(&dir.path().to_path_buf()), &args).unwrap();

        let exported: AgentFile =
            serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
        assert_eq!(exported.workspace.binary_files, vec!["avatar.png"]);
        let meta = exported
            .export_metadata
            .expect("metadata must be present for a partial export");
        assert!(
            !meta.lossless,
            "export with known gaps must not claim lossless"
        );
        // WHY: no knowledge store exists in this test, so there are no embeddings
        // to omit; only the binary workspace file makes this export non-lossless.
        let binary = meta
            .omitted_sections
            .iter()
            .find(|s| s.section == "workspace_binary_files")
            .expect("binary file omission must be recorded");
        assert_eq!(binary.count, Some(1));
    }

    /// WHY(#5102): importers must not silently drop binary workspace files.
    /// They are reported as skipped and are not written to disk.
    #[test]
    fn import_skips_binary_workspace_files_5102() {
        let dest = tempfile::tempdir().unwrap();
        let dest_oikos = Oikos::from_root(dest.path());
        std::fs::create_dir_all(dest_oikos.config()).unwrap();
        std::fs::create_dir_all(dest_oikos.data()).unwrap();

        let mut agent_file = sample_agent_file();
        agent_file.workspace.binary_files = vec!["avatar.png".to_owned()];
        agent_file.export_metadata = Some(mneme::portability::ExportMetadata {
            lossless: false,
            omitted_sections: vec![mneme::portability::OmittedSection {
                section: "workspace_binary_files".to_owned(),
                reason: "binary_content_not_exported".to_owned(),
                count: Some(1),
            }],
            truncations: vec![],
        });
        let import_path = dest.path().join("agent.agent.json");
        std::fs::write(
            &import_path,
            serde_json::to_string_pretty(&agent_file).unwrap(),
        )
        .unwrap();

        import_agent(
            Some(&dest.path().to_path_buf()),
            &ImportArgs {
                file: import_path,
                target_id: None,
                skip_sessions: false,
                skip_workspace: false,
                skip_knowledge: false,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
            },
        )
        .unwrap();

        let nous_dir = dest_oikos.nous_dir("imported-agent");
        assert!(nous_dir.join("SOUL.md").exists());
        assert!(
            !nous_dir.join("avatar.png").exists(),
            "binary file must not be created"
        );
    }

    /// WHY(#5102): session and knowledge skipping must be independent flags.
    #[cfg(feature = "recall")]
    #[test]
    fn import_skip_knowledge_flag_omits_typed_knowledge_5102() {
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
                allow_partial: false,
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
                skip_knowledge: true,
                force: false,
                dry_run: false,
                allow_unknown_values: false,
            },
        )
        .unwrap();

        assert!(
            imported_facts(&dest_oikos, "alice").is_empty(),
            "knowledge must be skipped when --skip-knowledge is set"
        );
    }
}
