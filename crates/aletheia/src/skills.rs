//! Skill management subcommand handlers (seed, export, review).

use std::path::Path;

use anyhow::{Context, Result};

use aletheia_taxis::oikos::Oikos;

use crate::cli::Cli;

#[expect(
    clippy::too_many_lines,
    reason = "CLI dispatch is inherently verbose — splitting would hurt readability"
)]
pub(crate) fn seed_skills_cmd(
    dir: &Path,
    nous_id: &str,
    force: bool,
    dry_run: bool,
) -> Result<()> {
    use aletheia_mneme::skill::{SkillContent, parse_skill_md, scan_skill_dir};

    let entries = scan_skill_dir(dir)
        .with_context(|| format!("failed to scan skill directory: {}", dir.display()))?;

    if entries.is_empty() {
        println!("No SKILL.md files found in {}", dir.display());
        return Ok(());
    }

    println!("Found {} skill(s) in {}", entries.len(), dir.display());

    // Parse all skills first
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

    if dry_run {
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

    // Open knowledge store (in-memory for seeding — caller must configure persistent path)
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
            // Check for duplicates
            let existing = store
                .find_skill_by_name(nous_id, &skill.name)
                .map_err(|e| anyhow::anyhow!("failed to query existing skills: {e}"))?;

            if let Some(existing_id) = existing {
                if force {
                    // Supersede the old fact
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

            // Generate embedding for semantic search
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
        let _ = (force, nous_id, parsed, parse_errors);
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
pub(crate) fn export_skills_cmd(
    cli: &Cli,
    nous_id: &str,
    output: &Path,
    domain: Option<&str>,
) -> Result<()> {
    #[cfg(feature = "recall")]
    {
        use aletheia_mneme::knowledge_store::KnowledgeStore;
        use aletheia_mneme::skill::{SkillContent, export_skills_to_cc};

        let oikos = match &cli.instance_root {
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

        let facts = store
            .find_skills_for_nous(nous_id, 500)
            .map_err(|e| anyhow::anyhow!("failed to query skills: {e}"))?;

        if facts.is_empty() {
            println!("No skills found for nous '{nous_id}'");
            return Ok(());
        }

        // Parse facts into SkillContent
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

        // Apply domain filter
        let domain_tags: Vec<&str> = domain
            .map(|d| d.split(',').map(str::trim).collect())
            .unwrap_or_default();
        let filter = if domain_tags.is_empty() {
            None
        } else {
            Some(domain_tags.as_slice())
        };

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
        let _ = (cli, nous_id, output, domain);
        anyhow::bail!(
            "export-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }
}

pub(crate) fn review_skills_cmd(
    cli: &Cli,
    nous_id: &str,
    action: &str,
    fact_id: Option<&str>,
) -> Result<()> {
    #[cfg(feature = "recall")]
    {
        use aletheia_mneme::knowledge_store::KnowledgeStore;
        use aletheia_mneme::skills::extract::PendingSkill;

        let oikos = match &cli.instance_root {
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

        match action {
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
                let fid = fact_id
                    .ok_or_else(|| anyhow::anyhow!("--fact-id required for approve action"))?;
                let fact_id = aletheia_mneme::id::FactId::from(fid);
                let new_id = store
                    .approve_pending_skill(&fact_id, nous_id)
                    .map_err(|e| anyhow::anyhow!("failed to approve skill: {e}"))?;
                println!("Approved: {fid} → new skill fact: {new_id}");
            }
            "reject" => {
                let fid = fact_id
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
        let _ = (cli, nous_id, action, fact_id);
        anyhow::bail!(
            "review-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
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

    // Generate enough hash bytes to fill the embedding
    let mut seed = hasher.finalize().to_vec();
    while embedding.len() < dim {
        for byte in &seed {
            if embedding.len() >= dim {
                break;
            }
            // Map byte to [-1.0, 1.0] — value is in [-1.0, 1.0] so truncation is harmless
            #[expect(clippy::cast_possible_truncation, reason = "result fits in f32 range")]
            embedding.push((f64::from(*byte) / 127.5 - 1.0) as f32);
        }
        // Re-hash for more bytes
        let mut h = Sha256::new();
        h.update(&seed);
        seed = h.finalize().to_vec();
    }

    // L2-normalize
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut embedding {
            *v /= norm;
        }
    }

    embedding
}
