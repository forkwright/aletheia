//! Agent import: restore an agent from a portable `AgentFile`.

use std::path::Path;

use snafu::{ResultExt, ensure};
use tracing::{info, instrument, warn};

use crate::error::{self, Result};
use crate::portability::{AGENT_FILE_VERSION, AgentFile};
use crate::store::SessionStore;

/// Options controlling import behavior.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Skip importing session history.
    pub skip_sessions: bool,
    /// Skip restoring workspace files.
    pub skip_workspace: bool,
    /// Override the target agent ID.
    pub target_nous_id: Option<String>,
    /// Overwrite existing workspace files.
    pub force: bool,
}

/// Summary of what was imported.
#[derive(Debug, Clone)]
pub struct ImportResult {
    /// Agent ID the data was imported under.
    pub nous_id: String,
    /// Number of workspace files restored.
    pub files_restored: u32,
    /// Number of sessions created.
    pub sessions_imported: u32,
    /// Total messages inserted.
    pub messages_imported: u32,
    /// Total notes inserted.
    pub notes_imported: u32,
}

/// Import an agent from a portable `AgentFile`.
///
/// `id_generator` produces new session IDs: the caller provides this because
/// mneme doesn't depend on `ulid`.
///
/// # Errors
///
/// Returns errors for unsupported versions, path traversal attempts, or store/IO failures.
#[instrument(skip(agent_file, store, id_generator))]
pub fn import_agent(
    agent_file: &AgentFile,
    store: &SessionStore,
    workspace_path: &Path,
    id_generator: &dyn Fn() -> String,
    opts: &ImportOptions,
) -> Result<ImportResult> {
    ensure!(
        agent_file.version == AGENT_FILE_VERSION,
        error::UnsupportedVersionSnafu {
            version: agent_file.version,
        }
    );

    let nous_id = opts
        .target_nous_id
        .as_deref()
        .unwrap_or(&agent_file.nous.id);

    let mut result = ImportResult {
        nous_id: nous_id.to_owned(),
        files_restored: 0,
        sessions_imported: 0,
        messages_imported: 0,
        notes_imported: 0,
    };

    if !opts.skip_workspace {
        result.files_restored =
            restore_workspace(&agent_file.workspace.files, workspace_path, opts.force)?;
    }

    if !opts.skip_sessions {
        import_sessions(agent_file, store, nous_id, id_generator, &mut result)?;
    }

    if let Some(ref memory) = agent_file.memory {
        let vectors = memory.vectors.as_ref().map_or(0, Vec::len);
        let graph = memory.graph.is_some();
        if vectors > 0 || graph {
            info!(
                vectors,
                graph, "memory data present but requires sidecar — skipped"
            );
        }
    }

    if let Some(ref knowledge) = agent_file.knowledge {
        info!(
            facts = knowledge.facts.len(),
            entities = knowledge.entities.len(),
            relationships = knowledge.relationships.len(),
            "knowledge data present — import requires knowledge store (skipped in session import)"
        );
    }

    info!(
        nous_id,
        files = result.files_restored,
        sessions = result.sessions_imported,
        messages = result.messages_imported,
        notes = result.notes_imported,
        "agent imported"
    );

    Ok(result)
}

/// Validate a relative file path for safety.
fn validate_relative_path(rel_path: &str) -> bool {
    if rel_path.is_empty() {
        return false;
    }

    // Reject absolute paths
    if rel_path.starts_with('/') || rel_path.starts_with('\\') {
        return false;
    }

    // Reject Windows drive letters
    if rel_path.len() >= 2 && rel_path.as_bytes()[1] == b':' {
        return false;
    }

    // Reject protocol prefixes
    if rel_path.contains("://") {
        return false;
    }

    // Reject path traversal via .. components
    for component in rel_path.split(['/', '\\']) {
        if component == ".." {
            return false;
        }
    }

    true
}

/// Restore workspace files to disk.
fn restore_workspace(
    files: &std::collections::HashMap<String, String>,
    workspace_path: &Path,
    force: bool,
) -> Result<u32> {
    let mut count = 0;

    for (rel_path, content) in files {
        ensure!(
            validate_relative_path(rel_path),
            error::UnsafePathSnafu {
                path: std::path::PathBuf::from(rel_path),
            }
        );

        let full_path = workspace_path.join(rel_path);

        if full_path.exists() && !force {
            warn!(path = %rel_path, "skipping existing file (use --force to overwrite)");
            continue;
        }

        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).context(error::IoSnafu {
                path: parent.to_path_buf(),
            })?;
        }

        std::fs::write(&full_path, content).context(error::IoSnafu {
            path: full_path.clone(),
        })?;

        count += 1;
    }

    Ok(count)
}

/// Import all sessions, messages, and notes from the agent file.
fn import_sessions(
    agent_file: &AgentFile,
    store: &SessionStore,
    nous_id: &str,
    id_generator: &dyn Fn() -> String,
    result: &mut ImportResult,
) -> Result<()> {
    let conn = store.conn();
    let timestamp = jiff::Timestamp::now().strftime("%Y%m%dT%H%M%S").to_string();

    for exported in &agent_file.sessions {
        let new_id = id_generator();
        let session_key = format!("{}-import-{timestamp}", exported.session_key);

        conn.execute(
            "INSERT INTO sessions (id, nous_id, session_key, status, session_type, \
             token_count_estimate, message_count, distillation_count, \
             working_state, distillation_priming, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                new_id,
                nous_id,
                session_key,
                exported.status,
                exported.session_type,
                exported.token_count_estimate,
                exported.message_count,
                exported.distillation_count,
                exported
                    .working_state
                    .as_ref()
                    .map(serde_json::Value::to_string),
                exported
                    .distillation_priming
                    .as_ref()
                    .map(serde_json::Value::to_string),
                exported.created_at,
                exported.updated_at,
            ],
        )
        .context(error::DatabaseSnafu)?;

        result.sessions_imported += 1;

        // Import messages in sequence order
        let mut sorted_messages = exported.messages.clone();
        sorted_messages.sort_by_key(|m| m.seq);

        for msg in &sorted_messages {
            conn.execute(
                "INSERT INTO messages (session_id, seq, role, content, token_estimate, \
                 is_distilled, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    new_id,
                    msg.seq,
                    msg.role,
                    msg.content,
                    msg.token_estimate,
                    i64::from(msg.is_distilled),
                    msg.created_at,
                ],
            )
            .context(error::DatabaseSnafu)?;

            result.messages_imported += 1;
        }

        // Import notes
        let valid_categories = crate::schema::VALID_CATEGORIES;
        for note in &exported.notes {
            let category = if valid_categories.contains(&note.category.as_str()) {
                &note.category
            } else {
                "context"
            };
            store.add_note(&new_id, nous_id, category, &note.content)?;
            result.notes_imported += 1;
        }
    }

    Ok(())
}

/// Import knowledge graph data from an `AgentFile` into a knowledge store.
///
/// # Errors
///
/// Returns errors if fact/entity/relationship insertion fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(knowledge, store))]
pub fn import_knowledge(
    knowledge: &crate::portability::KnowledgeExport,
    store: &crate::knowledge_store::KnowledgeStore,
) -> Result<KnowledgeImportResult> {
    let mut result = KnowledgeImportResult::default();

    for entity in &knowledge.entities {
        if let Err(e) = store.insert_entity(entity) {
            warn!(entity_id = %entity.id, error = %e, "failed to import entity");
            continue;
        }
        result.entities_imported += 1;
    }

    for rel in &knowledge.relationships {
        if let Err(e) = store.insert_relationship(rel) {
            warn!(src = %rel.src, dst = %rel.dst, error = %e, "failed to import relationship");
            continue;
        }
        result.relationships_imported += 1;
    }

    for fact in &knowledge.facts {
        if let Err(e) = store.insert_fact(fact) {
            warn!(fact_id = %fact.id, error = %e, "failed to import fact");
            continue;
        }
        result.facts_imported += 1;
    }

    info!(
        facts = result.facts_imported,
        entities = result.entities_imported,
        relationships = result.relationships_imported,
        "knowledge imported"
    );

    Ok(result)
}

/// Summary of knowledge graph import results.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone, Default)]
pub struct KnowledgeImportResult {
    /// Number of facts successfully imported.
    pub facts_imported: usize,
    /// Number of entities successfully imported.
    pub entities_imported: usize,
    /// Number of relationships successfully imported.
    pub relationships_imported: usize,
}

#[cfg(test)]
#[path = "import_tests/mod.rs"]
mod tests;
