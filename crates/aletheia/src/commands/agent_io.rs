//! Agent import/export and skill management commands.

use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use crate::error::Result;

#[derive(Debug, Clone, Args)]
pub(crate) struct ExportArgs {
    /// Agent (nous) ID to export
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
    pub nous_id: String,
    /// Output directory (default: .claude/skills)
    #[arg(short, long, default_value = ".claude/skills")]
    pub output: PathBuf,
    /// Filter by domain tags (comma-separated)
    #[arg(short, long)]
    pub domain: Option<String>,
    /// Server URL for lock detection
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    pub url: String,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ReviewSkillsArgs {
    /// Agent (nous) ID whose pending skills to review
    #[arg(short, long)]
    pub nous_id: String,
    /// Action: list, approve, reject
    #[arg(short, long, default_value = "list")]
    pub action: String,
    /// Fact ID of the pending skill (required for approve/reject)
    #[arg(short, long)]
    pub fact_id: Option<String>,
    /// Server URL for lock detection
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    pub url: String,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct MigrateMemoryArgs {
    /// Qdrant server URL
    #[arg(long, default_value = "http://localhost:6333", env = "QDRANT_URL")]
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

use graphe::types::Role;

#[cfg(feature = "recall")]
fn knowledge_path_for_nous(oikos: &Oikos, nous_id: &str) -> PathBuf {
    let cohort = taxis::loader::load_config(oikos).ok().map_or_else(
        || std::sync::Arc::from("shared"),
        |config| taxis::config::resolve_nous(&config, nous_id).episteme_cohort,
    );
    oikos.knowledge_cohort_db(cohort.as_ref())
}

pub(crate) fn export_agent(_instance_root: Option<&PathBuf>, _args: &ExportArgs) -> Result<()> {
    // WHY: the SQLite export pipeline was removed alongside rusqlite (#3446).
    // A fjall-backed agent export/import round trip needs to be re-implemented
    // against the new session store before this subcommand returns.
    whatever!(
        "export is temporarily unavailable: the agent-file export pipeline is being reimplemented on the fjall backend (#3446)"
    );
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
    let json = std::fs::read_to_string(&args.file)
        .with_whatever_context(|_| format!("failed to read {}", args.file.display()))?;
    let agent_file: mneme::portability::AgentFile =
        serde_json::from_str(&json).whatever_context("failed to parse agent file")?;

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
        let store =
            graphe::store::SessionStore::open(&sessions_db).with_whatever_context(|_| {
                format!("failed to open session store at {}", sessions_db.display())
            })?;

        for session in &agent_file.sessions {
            let imported = store
                .create_session(
                    &session.id,
                    &nous_id,
                    &session.session_key,
                    None,
                    agent_file.nous.model.as_deref(),
                )
                .with_whatever_context(|_| format!("failed to create session {}", session.id))?;

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
                    .append_message(
                        &imported.id,
                        role,
                        &msg.content,
                        None,
                        None,
                        msg.token_estimate,
                    )
                    .with_whatever_context(|_| {
                        format!("failed to append message to session {}", session.id)
                    })?;
            }

            for note in &session.notes {
                let category = if graphe::store::SessionStore::VALID_CATEGORIES
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
pub(crate) fn seed_skills(args: &SeedSkillsArgs) -> Result<()> {
    use mneme::skill::{SkillContent, parse_skill_md, scan_skill_dir};

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

        let store =
            KnowledgeStore::open_mem().whatever_context("failed to open knowledge store")?;

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
        let _ = (args, nous_id, parsed, parse_errors);
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
/// preventing a confusing `FjallError::Locked` crash.
pub(crate) async fn guard_knowledge_lock(url: &str) -> Result<()> {
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

    use graphe::portability::{
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

    fn sample_agent_file() -> AgentFile {
        AgentFile {
            version: 1,
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
        let store = graphe::store::SessionStore::open(&oikos.sessions_db()).unwrap();
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

        let store = graphe::store::SessionStore::open(&oikos.sessions_db()).unwrap();
        let sessions = store.list_sessions(Some("imported-agent")).unwrap();
        assert!(sessions.is_empty(), "sessions should be skipped");
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
}
