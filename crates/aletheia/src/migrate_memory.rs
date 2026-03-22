//! One-time migration of Qdrant memories into embedded `KnowledgeStore`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing::info;

use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    ScrollPointsBuilder, value, with_payload_selector, with_vectors_selector,
};

use aletheia_mneme::embedding::{EmbeddingConfig, EmbeddingProvider, create_provider};
use aletheia_mneme::id::{EmbeddingId, FactId};
use aletheia_mneme::knowledge::{
    EmbeddedChunk, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
    far_future, parse_timestamp,
};
use aletheia_mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use aletheia_taxis::oikos::Oikos;

struct MemoryRecord {
    content: String,
    agent_id: String,
    mem_score: f64,
    created_at: String,
}

fn extract_string(v: &qdrant_client::qdrant::Value) -> Option<String> {
    match v.kind.as_ref()? {
        value::Kind::StringValue(s) => Some(s.clone()),
        _ => None,
    }
}

fn extract_double(v: &qdrant_client::qdrant::Value) -> Option<f64> {
    match v.kind.as_ref()? {
        value::Kind::DoubleValue(d) => Some(*d),
        _ => None,
    }
}

pub(crate) async fn run(
    instance_root: Option<&PathBuf>,
    qdrant_url: &str,
    collection: &str,
    knowledge_path: Option<&PathBuf>,
    review_file: Option<&PathBuf>,
    dry_run: bool,
) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    info!(qdrant_url, collection, dry_run, "starting memory migration");

    let client = Qdrant::from_url(qdrant_url)
        .build()
        .context("failed to connect to Qdrant")?;

    let embedder: Arc<dyn EmbeddingProvider> = Arc::from(
        create_provider(&EmbeddingConfig::default()).context("failed to create embedder")?,
    );

    let config = KnowledgeConfig::default();
    let knowledgedb = if dry_run {
        KnowledgeStore::open_mem_with_config(config)
            .context("failed to open in-memory knowledge store")?
    } else {
        let path = knowledge_path
            .cloned()
            .unwrap_or_else(|| oikos.knowledge_db());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("failed to create knowledge store directory")?;
        }
        info!(path = %path.display(), "opening persistent knowledge store");
        KnowledgeStore::open_fjall(&path, config)
            .context("failed to open persistent knowledge store")?
    };

    let all_records = fetch_from_qdrant(&client, collection).await?;
    info!(total = all_records.len(), "fetched memories from Qdrant");

    let mut by_agent: HashMap<String, Vec<MemoryRecord>> = HashMap::new();
    for record in all_records {
        by_agent
            .entry(record.agent_id.clone())
            .or_default()
            .push(record);
    }

    let mut total_imported = 0usize;
    let mut total_deduped = 0usize;
    let mut flagged: Vec<String> = Vec::new();

    for (agent_id, records) in &by_agent {
        let (imported, deduped) = process_agent(
            agent_id,
            records,
            &knowledgedb,
            &embedder,
            &mut flagged,
            dry_run,
        )?;
        total_imported += imported;
        total_deduped += deduped;
    }

    info!(
        imported = total_imported,
        deduplicated = total_deduped,
        flagged = flagged.len(),
        dry_run,
        "migration complete"
    );

    if let Some(path) = review_file {
        if !flagged.is_empty() {
            write_review_file(path, &flagged)?;
            info!(path = %path.display(), "review file written");
        }
    }

    Ok(())
}

async fn fetch_from_qdrant(client: &Qdrant, collection: &str) -> Result<Vec<MemoryRecord>> {
    let mut offset: Option<qdrant_client::qdrant::PointId> = None;
    let mut all_records: Vec<MemoryRecord> = Vec::new();

    loop {
        let mut builder = ScrollPointsBuilder::new(collection)
            .with_payload(with_payload_selector::SelectorOptions::Enable(true))
            .with_vectors(with_vectors_selector::SelectorOptions::Enable(false))
            .limit(100);
        if let Some(ref o) = offset {
            builder = builder.offset(o.clone());
        }

        let response = client
            .scroll(builder)
            .await
            .context("failed to scroll Qdrant points")?;

        for point in &response.result {
            let payload = &point.payload;
            let content = payload
                .get("memory")
                .or_else(|| payload.get("data"))
                .and_then(extract_string)
                .unwrap_or_default();
            if content.is_empty() {
                continue;
            }

            let agent_id = payload
                .get("agent_id")
                .and_then(extract_string)
                .unwrap_or_else(|| "unknown".to_owned());
            let mem_score = payload
                .get("score")
                .or_else(|| payload.get("confidence"))
                .and_then(extract_double)
                .unwrap_or(0.5);
            let created_at = payload
                .get("created_at")
                .and_then(extract_string)
                .unwrap_or_else(|| "2025-01-01T00:00:00Z".to_owned());

            all_records.push(MemoryRecord {
                content,
                agent_id,
                mem_score,
                created_at,
            });
        }

        offset = response.next_page_offset;
        if offset.is_none() {
            break;
        }
    }

    Ok(all_records)
}

fn process_agent(
    agent_id: &str,
    records: &[MemoryRecord],
    knowledgedb: &KnowledgeStore,
    embedder: &Arc<dyn EmbeddingProvider>,
    flagged: &mut Vec<String>,
    dry_run: bool,
) -> Result<(usize, usize)> {
    let mut seen_hashes: HashMap<String, usize> = HashMap::new();
    let mut unique: Vec<&MemoryRecord> = Vec::new();
    let mut agent_deduped = 0usize;

    for record in records {
        let hash = content_hash(&record.content);
        if seen_hashes.contains_key(&hash) {
            agent_deduped += 1;
            continue;
        }
        seen_hashes.insert(hash, unique.len());
        unique.push(record);
    }

    for record in &unique {
        if record.content.len() < 10 {
            flagged.push(format!("[{agent_id}] too short: \"{}\"", record.content));
        }
        if record.mem_score < 0.3 {
            flagged.push(format!(
                "[{agent_id}] low score ({:.2}): \"{}\"",
                record.mem_score,
                record.content.chars().take(80).collect::<String>()
            ));
        }
    }

    if !dry_run {
        for record in &unique {
            import_fact(agent_id, record, knowledgedb, embedder)?;
        }
    }

    info!(
        agent_id,
        fetched = records.len(),
        duplicates_removed = agent_deduped,
        imported = unique.len(),
        flagged = flagged
            .iter()
            .filter(|f| f.starts_with(&format!("[{agent_id}]")))
            .count(),
        "agent migration stats"
    );

    Ok((unique.len(), agent_deduped))
}

fn import_fact(
    agent_id: &str,
    record: &MemoryRecord,
    knowledgedb: &KnowledgeStore,
    embedder: &Arc<dyn EmbeddingProvider>,
) -> Result<()> {
    let fact_id = format!("migrated-{}", content_hash(&record.content));
    let now = jiff::Timestamp::now();
    let valid_from = parse_timestamp(&record.created_at).unwrap_or(now);

    let fact = Fact {
        id: FactId::new(&fact_id).expect("ULID fact_id is always valid"),
        nous_id: agent_id.to_owned(),
        content: record.content.clone(),
        fact_type: String::new(),
        temporal: FactTemporal {
            valid_from,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.7,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: aletheia_mneme::knowledge::default_stability_hours(""),
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
    knowledgedb
        .insert_fact(&fact)
        .context("failed to insert fact")?;

    if let Ok(embedding) = embedder.embed(&record.content) {
        let chunk = EmbeddedChunk {
            id: EmbeddingId::new(&format!("emb-{fact_id}"))
                .expect("emb- prefix + ULID is always valid"),
            content: record.content.clone(),
            source_type: "fact".to_owned(),
            source_id: fact_id,
            nous_id: agent_id.to_owned(),
            embedding,
            created_at: now,
        };
        knowledgedb
            .insert_embedding(&chunk)
            .context("failed to insert embedding")?;
    }

    Ok(())
}

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    hex.get(..16).unwrap_or(&hex).to_owned()
}

fn write_review_file(path: &Path, flagged: &[String]) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path).context("failed to create review file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640));
    }
    writeln!(f, "# Memory Migration Review")?;
    writeln!(f)?;
    writeln!(f, "The following facts were flagged during migration:")?;
    writeln!(f)?;
    for item in flagged {
        writeln!(f, "- {item}")?;
    }
    Ok(())
}
