//! Agent import/export and skill management commands.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

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
    /// Instance root directory (default in interactive/-y mode: ./instance)
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
    /// Anthropic API key
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

use aletheia_mneme::store::SessionStore;
use aletheia_taxis::config::resolve_nous;
use aletheia_taxis::loader::load_config;
use aletheia_taxis::oikos::Oikos;

pub(crate) fn export_agent(instance_root: Option<&PathBuf>, args: &ExportArgs) -> Result<()> {
    let nous_id = &args.nous_id;
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let config = load_config(&oikos).context("failed to load config")?;
    let resolved = resolve_nous(&config, nous_id);

    let db_path = oikos.sessions_db();
    let store = SessionStore::open(&db_path)
        .with_context(|| format!("failed to open session store at {}", db_path.display()))?;

    let workspace_path = oikos.nous_dir(nous_id);

    let agent_config = config
        .agents
        .list
        .iter()
        .find(|a| a.id == nous_id.as_str())
        .map_or(serde_json::Value::Null, |a| {
            serde_json::to_value(a).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed to serialize agent config");
                serde_json::Value::Null
            })
        });

    let opts = aletheia_mneme::export::ExportOptions {
        max_messages_per_session: args.max_messages,
        include_archived: args.archived,
    };
    let agent_file = aletheia_mneme::export::export_agent(
        nous_id,
        resolved.name.as_deref(),
        Some(&resolved.model.primary),
        agent_config,
        &store,
        &workspace_path,
        &opts,
    )
    .context("export failed")?;

    let output_path = args.output.clone().unwrap_or_else(|| {
        let date = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
        PathBuf::from(format!("{nous_id}-{date}.agent.json"))
    });

    if output_path.exists() && !args.force {
        anyhow::bail!(
            "output file already exists: {}\nUse --force to overwrite.",
            output_path.display()
        );
    }

    let json = if args.compact {
        serde_json::to_string(&agent_file)?
    } else {
        serde_json::to_string_pretty(&agent_file)?
    };
    std::fs::write(&output_path, &json)
        .with_context(|| format!("failed to write {}", output_path.display()))?;

    println!("Exported to: {}", output_path.display());
    println!("Size: {} bytes", json.len());
    println!(
        "Sessions: {}, Workspace: {} text / {} binary",
        agent_file.sessions.len(),
        agent_file.workspace.files.len(),
        agent_file.workspace.binary_files.len()
    );

    Ok(())
}

pub(crate) fn import_agent(instance_root: Option<&PathBuf>, args: &ImportArgs) -> Result<()> {
    let json = std::fs::read_to_string(&args.file)
        .with_context(|| format!("failed to read {}", args.file.display()))?;
    let agent_file: aletheia_mneme::portability::AgentFile =
        serde_json::from_str(&json).context("failed to parse agent file")?;

    let nous_id = args.target_id.as_deref().unwrap_or(&agent_file.nous.id);

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

    let db_path = oikos.sessions_db();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create data dir {}", parent.display()))?;
    }
    let store = SessionStore::open(&db_path)
        .with_context(|| format!("failed to open session store at {}", db_path.display()))?;

    let workspace_path = oikos.nous_dir(nous_id);
    std::fs::create_dir_all(&workspace_path)
        .with_context(|| format!("failed to create workspace {}", workspace_path.display()))?;

    let opts = aletheia_mneme::import::ImportOptions {
        skip_sessions: args.skip_sessions,
        skip_workspace: args.skip_workspace,
        target_nous_id: args.target_id.clone(),
        force: args.force,
    };
    let id_gen = || ulid::Ulid::new().to_string();
    let result =
        aletheia_mneme::import::import_agent(&agent_file, &store, &workspace_path, &id_gen, &opts)
            .context("import failed")?;

    println!("Imported agent: {}", result.nous_id);
    println!("Files restored: {}", result.files_restored);
    println!("Sessions: {}", result.sessions_imported);
    println!("Messages: {}", result.messages_imported);
    println!("Notes: {}", result.notes_imported);

    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "CLI dispatch is inherently verbose — splitting would hurt readability"
)]
pub(crate) fn seed_skills(args: &SeedSkillsArgs) -> Result<()> {
    use aletheia_mneme::skill::{SkillContent, parse_skill_md, scan_skill_dir};

    let dir = &args.dir;
    let nous_id = &args.nous_id;
    let entries = scan_skill_dir(dir)
        .with_context(|| format!("failed to scan skill directory: {}", dir.display()))?;

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
        use aletheia_mneme::knowledge::{EpistemicTier, Fact, default_stability_hours};
        use aletheia_mneme::knowledge_store::KnowledgeStore;

        let store = KnowledgeStore::open_mem()
            .map_err(|e| anyhow::anyhow!("failed to open knowledge store: {e}"))?;

        let now = jiff::Timestamp::now();
        let mut seeded = 0u32;
        let mut skipped = 0u32;
        let mut overwritten = 0u32;

        for (slug, skill) in &parsed {
            let existing = store
                .find_skill_by_name(nous_id, &skill.name)
                .map_err(|e| anyhow::anyhow!("failed to query existing skills: {e}"))?;

            if let Some(existing_id) = existing {
                if args.force {
                    if let Err(e) = store.forget_fact(
                        &aletheia_mneme::id::FactId::from(existing_id),
                        aletheia_mneme::knowledge::ForgetReason::Outdated,
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
                .with_context(|| format!("failed to serialize skill: {slug}"))?;

            let fact_id = ulid::Ulid::new().to_string();
            let fact = Fact {
                id: aletheia_mneme::id::FactId::from(fact_id.clone()),
                nous_id: nous_id.to_owned(),
                content: content_json.clone(),
                confidence: 0.5,
                tier: EpistemicTier::Assumed,
                valid_from: now,
                valid_to: aletheia_mneme::knowledge::far_future(),
                superseded_by: None,
                source_session_id: None,
                recorded_at: now,
                access_count: 0,
                last_accessed_at: None,
                stability_hours: default_stability_hours("skill"),
                fact_type: "skill".to_owned(),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            };

            store
                .insert_fact(&fact)
                .map_err(|e| anyhow::anyhow!("failed to insert skill {slug}: {e}"))?;

            let embedding_text = format!("{}: {}", skill.name, skill.description);
            let emb_id = ulid::Ulid::new().to_string();
            let chunk = aletheia_mneme::knowledge::EmbeddedChunk {
                id: aletheia_mneme::id::EmbeddingId::from(emb_id),
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
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (args, nous_id, parsed, parse_errors);
        anyhow::bail!(
            "seed-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }

    Ok(())
}

/// Export skills from the knowledge store to Claude Code's native format.
///
/// Reads skill facts from an in-process `KnowledgeStore`, converts them to
/// `SkillContent`, and writes `.claude/skills/<slug>/SKILL.md` files.
pub(crate) fn export_skills(
    instance_root: Option<&PathBuf>,
    args: &ExportSkillsArgs,
) -> Result<()> {
    #[cfg(feature = "recall")]
    {
        use aletheia_mneme::knowledge_store::KnowledgeStore;
        use aletheia_mneme::skill::{SkillContent, export_skills_to_cc};

        let oikos = match instance_root {
            Some(root) => Oikos::from_root(root),
            None => Oikos::discover(),
        };
        let knowledge_path = oikos.knowledge_db();

        let config = aletheia_mneme::knowledge_store::KnowledgeConfig::default();
        let store = KnowledgeStore::open_fjall(&knowledge_path, config).map_err(|e| {
            anyhow::anyhow!(
                "failed to open knowledge store at {}: {e}",
                knowledge_path.display()
            )
        })?;

        let nous_id = &args.nous_id;
        let facts = store
            .find_skills_for_nous(nous_id, 500)
            .map_err(|e| anyhow::anyhow!("failed to query skills: {e}"))?;

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

        let domain_tags: Vec<&str> = args
            .domain
            .as_deref()
            .map(|d| d.split(',').map(str::trim).collect())
            .unwrap_or_default();
        let filter = if domain_tags.is_empty() {
            None
        } else {
            Some(domain_tags.as_slice())
        };

        let output = &args.output;
        let exported = export_skills_to_cc(&skills, output, filter)
            .with_context(|| format!("failed to export skills to {}", output.display()))?;

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
        anyhow::bail!(
            "export-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }
}

pub(crate) fn review_skills(
    instance_root: Option<&PathBuf>,
    args: &ReviewSkillsArgs,
) -> Result<()> {
    #[cfg(feature = "recall")]
    {
        use aletheia_mneme::knowledge_store::KnowledgeStore;
        use aletheia_mneme::skills::extract::PendingSkill;

        let oikos = match instance_root {
            Some(root) => Oikos::from_root(root),
            None => Oikos::discover(),
        };
        let knowledge_path = oikos.knowledge_db();

        let config = aletheia_mneme::knowledge_store::KnowledgeConfig::default();
        let store = KnowledgeStore::open_fjall(&knowledge_path, config).map_err(|e| {
            anyhow::anyhow!(
                "failed to open knowledge store at {}: {e}",
                knowledge_path.display()
            )
        })?;

        let nous_id = &args.nous_id;
        match args.action.as_str() {
            "list" => {
                let pending = store
                    .find_pending_skills(nous_id)
                    .map_err(|e| anyhow::anyhow!("failed to query pending skills: {e}"))?;

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
                let fid = args
                    .fact_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--fact-id required for approve action"))?;
                let fact_id = aletheia_mneme::id::FactId::from(fid);
                let new_id = store
                    .approve_pending_skill(&fact_id, nous_id)
                    .map_err(|e| anyhow::anyhow!("failed to approve skill: {e}"))?;
                println!("Approved: {fid} → new skill fact: {new_id}");
            }
            "reject" => {
                let fid = args
                    .fact_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--fact-id required for reject action"))?;
                let fact_id = aletheia_mneme::id::FactId::from(fid);
                store
                    .reject_pending_skill(&fact_id)
                    .map_err(|e| anyhow::anyhow!("failed to reject skill: {e}"))?;
                println!("Rejected: {fid}");
            }
            other => {
                anyhow::bail!("unknown action '{other}'. Use: list, approve, reject");
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (instance_root, args);
        anyhow::bail!(
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
        anyhow::bail!(
            "migrate-memory requires the `migrate-qdrant` feature.\n\
             Rebuild with: cargo build --features migrate-qdrant"
        );
    }
}

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
            #[expect(clippy::cast_possible_truncation, reason = "result fits in f32 range")]
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
