//! Post-merge lesson extraction pipeline.
//!
//! Accepts a PR diff in unified format, parses it into structured change
//! records, extracts knowledge facts (what changed, why, what was broken,
//! what was fixed), and produces typed nodes and edges for the knowledge graph.

use serde::{Deserialize, Serialize};

use super::diff::{DiffFile, ParsedDiff, parse_unified_diff};

/// A structured change record derived from a parsed diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRecord {
    /// File path that was changed.
    pub file_path: String,
    /// Type of change.
    pub change_type: ChangeType,
    /// Summary of what changed (human-readable).
    pub summary: String,
    /// Lines added across all hunks.
    pub lines_added: u32,
    /// Lines removed across all hunks.
    pub lines_removed: u32,
    /// Function/context names from hunk headers, if available.
    pub contexts: Vec<String>,
}

/// Classification of a file-level change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChangeType {
    /// New file added.
    Added,
    /// Existing file modified.
    Modified,
    /// File deleted.
    Deleted,
    /// File renamed (detected by `old_path` != `new_path` and both non-null).
    Renamed,
}

impl std::fmt::Display for ChangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Added => f.write_str("added"),
            Self::Modified => f.write_str("modified"),
            Self::Deleted => f.write_str("deleted"),
            Self::Renamed => f.write_str("renamed"),
        }
    }
}

/// A lesson extracted from a PR diff, ready to be stored as knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedLesson {
    /// Entities discovered in the diff (files, modules, functions).
    pub entities: Vec<super::types::ExtractedEntity>,
    /// Relationships between entities.
    pub relationships: Vec<super::types::ExtractedRelationship>,
    /// Facts about what changed and why.
    pub facts: Vec<super::types::ExtractedFact>,
    /// Causal edges between facts (e.g., "bug fix caused by code change").
    pub causal_edges: Vec<CausalFactPair>,
}

/// A pair of fact indices representing a causal relationship within an extraction.
///
/// The indices refer to positions in [`ExtractedLesson::facts`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalFactPair {
    /// Index of the cause fact in the lesson's fact list.
    pub cause_index: usize,
    /// Index of the effect fact in the lesson's fact list.
    pub effect_index: usize,
    /// Confidence in this causal link.
    pub confidence: f64,
}

/// Configuration for the lesson extraction pipeline.
#[derive(Debug, Clone)]
pub struct LessonConfig {
    /// PR identifier or title for provenance.
    pub pr_title: String,
    /// PR number for linking.
    pub pr_number: Option<u32>,
    /// The nous agent this lesson belongs to.
    pub nous_id: String,
    /// Source identifier (e.g., "pr-merge:123").
    pub source: String,
}

/// Extract lessons from a unified diff string.
///
/// Pipeline:
/// 1. Parse unified diff into structured `DiffFile` records
/// 2. Classify each file change (added/modified/deleted)
/// 3. Extract knowledge facts from the changes
/// 4. Produce entities, relationships, and causal edges
#[must_use]
pub fn extract_lessons(diff: &str, config: &LessonConfig) -> ExtractedLesson {
    let parsed = parse_unified_diff(diff);
    let changes = classify_changes(&parsed);
    build_lesson(&changes, config)
}

/// Classify parsed diff files into structured change records.
#[must_use]
fn classify_changes(parsed: &ParsedDiff) -> Vec<ChangeRecord> {
    parsed.files.iter().map(classify_file).collect()
}

/// Classify a single file diff into a change record.
fn classify_file(file: &DiffFile) -> ChangeRecord {
    let change_type = if file.is_new {
        ChangeType::Added
    } else if file.is_deleted {
        ChangeType::Deleted
    } else if file.old_path != file.new_path
        && !file.old_path.is_empty()
        && !file.new_path.is_empty()
    {
        ChangeType::Renamed
    } else {
        ChangeType::Modified
    };

    let lines_added: u32 = file
        .hunks
        .iter()
        .map(|h| u32::try_from(h.additions.len()).unwrap_or(u32::MAX))
        .fold(0u32, u32::saturating_add);

    let lines_removed: u32 = file
        .hunks
        .iter()
        .map(|h| u32::try_from(h.deletions.len()).unwrap_or(u32::MAX))
        .fold(0u32, u32::saturating_add);

    let contexts: Vec<String> = file
        .hunks
        .iter()
        .filter(|h| !h.context.is_empty())
        .map(|h| h.context.clone())
        .collect();

    let path = file.effective_path();
    let summary = match change_type {
        ChangeType::Added => format!("new file: {path}"),
        ChangeType::Deleted => format!("deleted file: {path}"),
        ChangeType::Renamed => format!("renamed: {} -> {}", file.old_path, file.new_path),
        ChangeType::Modified => {
            format!("modified {path} (+{lines_added}/-{lines_removed})")
        }
    };

    ChangeRecord {
        file_path: path.to_owned(),
        change_type,
        summary,
        lines_added,
        lines_removed,
        contexts,
    }
}

/// Build a lesson from classified change records.
fn build_lesson(changes: &[ChangeRecord], config: &LessonConfig) -> ExtractedLesson {
    let mut entities = Vec::new();
    let mut relationships = Vec::new();
    let mut facts = Vec::new();
    let mut causal_edges = Vec::new();

    // Create a PR entity.
    let pr_name = if let Some(num) = config.pr_number {
        format!("PR #{num}")
    } else {
        config.pr_title.clone()
    };

    entities.push(super::types::ExtractedEntity {
        name: pr_name.clone(),
        entity_type: "pull_request".to_owned(),
        description: config.pr_title.clone(),
    });

    for change in changes {
        // Extract file path components for entities.
        let file_entity_name = extract_module_name(&change.file_path);

        entities.push(super::types::ExtractedEntity {
            name: file_entity_name.clone(),
            entity_type: "file".to_owned(),
            description: change.file_path.clone(),
        });

        // PR modifies/adds/deletes file.
        relationships.push(super::types::ExtractedRelationship {
            source: pr_name.clone(),
            relation: format!("{}", change.change_type),
            target: file_entity_name.clone(),
            confidence: 1.0,
        });

        // Create a fact about the change.
        let change_fact_idx = facts.len();
        facts.push(super::types::ExtractedFact {
            subject: pr_name.clone(),
            predicate: format!("{}", change.change_type),
            object: change.file_path.clone(),
            confidence: 1.0,
            is_correction: false,
            fact_type: Some("event".to_owned()),
        });

        // Extract facts from hunk-level patterns.
        let hunk_facts = extract_hunk_facts(change, &file_entity_name);
        for hunk_fact in hunk_facts {
            let effect_idx = facts.len();
            facts.push(hunk_fact);
            // The file change caused the hunk-level observation.
            causal_edges.push(CausalFactPair {
                cause_index: change_fact_idx,
                effect_index: effect_idx,
                confidence: 0.8,
            });
        }

        // Extract context-level entities (functions/methods from hunk headers).
        for ctx in &change.contexts {
            let ctx_name = ctx.trim().to_owned();
            if !ctx_name.is_empty() {
                entities.push(super::types::ExtractedEntity {
                    name: ctx_name.clone(),
                    entity_type: "function".to_owned(),
                    description: format!("function in {}", change.file_path),
                });
                relationships.push(super::types::ExtractedRelationship {
                    source: file_entity_name.clone(),
                    relation: "contains".to_owned(),
                    target: ctx_name,
                    confidence: 0.9,
                });
            }
        }
    }

    ExtractedLesson {
        entities,
        relationships,
        facts,
        causal_edges,
    }
}

/// Extract hunk-level facts by pattern-matching on added/removed lines.
fn extract_hunk_facts(
    change: &ChangeRecord,
    file_entity: &str,
) -> Vec<super::types::ExtractedFact> {
    let mut facts = Vec::new();

    // Detect fix patterns in additions.
    let all_additions: Vec<&str> = change.contexts.iter().map(String::as_str).collect();

    // Detect if this looks like a bug fix (common patterns).
    if is_bug_fix_pattern(change) {
        facts.push(super::types::ExtractedFact {
            subject: file_entity.to_owned(),
            predicate: "had bug fixed in".to_owned(),
            object: change.summary.clone(),
            confidence: 0.7,
            is_correction: true,
            fact_type: Some("event".to_owned()),
        });
    }

    // Detect dependency changes.
    if is_dependency_change(&change.file_path) {
        let dep_action = match change.lines_added.cmp(&change.lines_removed) {
            std::cmp::Ordering::Greater => "added dependencies",
            std::cmp::Ordering::Less => "removed dependencies",
            std::cmp::Ordering::Equal => "updated dependencies",
        };
        facts.push(super::types::ExtractedFact {
            subject: file_entity.to_owned(),
            predicate: dep_action.to_owned(),
            object: change.file_path.clone(),
            confidence: 0.9,
            is_correction: false,
            fact_type: Some("event".to_owned()),
        });
    }

    // Detect test changes.
    if is_test_file(&change.file_path) || has_test_context(&all_additions) {
        facts.push(super::types::ExtractedFact {
            subject: file_entity.to_owned(),
            predicate: "had tests modified".to_owned(),
            object: change.summary.clone(),
            confidence: 0.9,
            is_correction: false,
            fact_type: Some("event".to_owned()),
        });
    }

    // Detect refactoring (significant removals and additions, not a new/deleted file).
    if change.change_type == ChangeType::Modified
        && change.lines_added > 5
        && change.lines_removed > 5
    {
        facts.push(super::types::ExtractedFact {
            subject: file_entity.to_owned(),
            predicate: "was refactored".to_owned(),
            object: format!("+{}/−{} lines", change.lines_added, change.lines_removed),
            confidence: 0.6,
            is_correction: false,
            fact_type: Some("event".to_owned()),
        });
    }

    facts
}

/// Heuristic: does this change look like a bug fix?
fn is_bug_fix_pattern(change: &ChangeRecord) -> bool {
    let path_lower = change.file_path.to_lowercase();
    // File path hints.
    if path_lower.contains("fix") || path_lower.contains("patch") {
        return true;
    }
    // Context hints (function names containing "fix").
    change
        .contexts
        .iter()
        .any(|c| c.to_lowercase().contains("fix"))
}

/// Heuristic: is this a dependency manifest file?
fn is_dependency_change(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with("cargo.toml")
        || lower.ends_with("cargo.lock")
        || lower.ends_with("package.json")
        || lower.ends_with("go.mod")
        || lower.ends_with("requirements.txt")
        || lower.ends_with("pyproject.toml")
}

/// Heuristic: is this file likely a test file?
fn is_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("test")
        || lower.contains("spec")
        || lower.ends_with("_tests.rs")
        || lower.ends_with("_test.go")
}

/// Heuristic: do the contexts mention test functions?
fn has_test_context(contexts: &[&str]) -> bool {
    contexts
        .iter()
        .any(|c| c.contains("test") || c.contains("Test"))
}

/// Extract a short module name from a file path.
///
/// Returns the filename stem, or the last two path components for deeper paths.
fn extract_module_name(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.len() {
        0 => path.to_owned(),
        1 => parts.first().map_or(path, |p| p).to_owned(),
        _ => {
            // Use last two components for context: "knowledge_store/causal.rs"
            let start = parts.len().saturating_sub(2);
            parts
                .get(start..)
                .map_or_else(|| path.to_owned(), |slice| slice.join("/"))
        }
    }
}

/// Persist an extracted lesson into the knowledge store.
///
/// Creates facts, entities, relationships, and causal edges in the graph.
///
/// # Errors
///
/// Returns extraction errors if persistence fails.
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::too_many_lines,
    reason = "sequential persist pipeline: entities → relationships → facts → causal edges"
)]
pub fn persist_lesson(
    lesson: &ExtractedLesson,
    store: &crate::knowledge_store::KnowledgeStore,
    config: &LessonConfig,
) -> Result<LessonPersistResult, super::error::ExtractionError> {
    use super::error::PersistSnafu;
    use super::utils::slugify;
    use crate::knowledge::{
        CausalEdge, Entity, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance,
        FactTemporal, Relationship, TemporalOrdering, far_future,
    };

    let now = jiff::Timestamp::now();
    let mut result = LessonPersistResult::default();

    // Insert entities.
    for entity in &lesson.entities {
        let id = crate::id::EntityId::from(slugify(&entity.name));
        let aliases = if entity.description.is_empty() {
            vec![]
        } else {
            vec![entity.description.clone()]
        };
        let entity_type = if entity.entity_type.is_empty() {
            "concept".to_owned()
        } else {
            entity.entity_type.clone()
        };
        let e = Entity {
            id,
            name: entity.name.clone(),
            entity_type,
            aliases,
            created_at: now,
            updated_at: now,
        };
        store.insert_entity(&e).map_err(|e| {
            PersistSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        result.entities_inserted += 1;
    }

    // Insert relationships.
    for rel in &lesson.relationships {
        let r = Relationship {
            src: crate::id::EntityId::from(slugify(&rel.source)),
            dst: crate::id::EntityId::from(slugify(&rel.target)),
            relation: rel.relation.clone(),
            weight: rel.confidence,
            created_at: now,
        };
        store.insert_relationship(&r).map_err(|e| {
            PersistSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        result.relationships_inserted += 1;
    }

    // Insert facts and track their IDs for causal edge linking.
    let mut fact_ids: Vec<crate::id::FactId> = Vec::new();
    for (i, fact) in lesson.facts.iter().enumerate() {
        let content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
        let id = crate::id::FactId::from(format!("lesson-{}-{i}", slugify(&config.source)));
        fact_ids.push(id.clone());

        let classified_type = fact.fact_type.as_deref().map_or_else(
            || crate::knowledge::FactType::classify(&content),
            crate::knowledge::FactType::from_str_lossy,
        );

        let f = Fact {
            id,
            nous_id: config.nous_id.clone(),
            content,
            fact_type: classified_type.as_str().to_owned(),
            temporal: FactTemporal {
                valid_from: now,
                valid_to: far_future(),
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence: fact.confidence,
                tier: EpistemicTier::Inferred,
                source_session_id: Some(config.source.clone()),
                stability_hours: classified_type.base_stability_hours(),
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
        store.insert_fact(&f).map_err(|e| {
            PersistSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        result.facts_inserted += 1;
    }

    // Insert causal edges between facts.
    for causal in &lesson.causal_edges {
        if let (Some(cause_id), Some(effect_id)) = (
            fact_ids.get(causal.cause_index),
            fact_ids.get(causal.effect_index),
        ) {
            let edge = CausalEdge {
                cause: cause_id.clone(),
                effect: effect_id.clone(),
                ordering: TemporalOrdering::Before,
                confidence: causal.confidence,
                created_at: now,
            };
            store.insert_causal_edge(&edge).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            result.causal_edges_inserted += 1;
        }
    }

    Ok(result)
}

/// Result counts from persisting a lesson into the knowledge store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LessonPersistResult {
    /// Number of entities written.
    pub entities_inserted: usize,
    /// Number of relationships written.
    pub relationships_inserted: usize,
    /// Number of facts written.
    pub facts_inserted: usize,
    /// Number of causal edges written.
    pub causal_edges_inserted: usize,
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions")]
mod tests {
    use super::*;

    const PR_DIFF: &str = r#"diff --git a/crates/mneme/src/knowledge_store/causal.rs b/crates/mneme/src/knowledge_store/causal.rs
--- /dev/null
+++ b/crates/mneme/src/knowledge_store/causal.rs
@@ -0,0 +1,10 @@
+//! Causal edge operations.
+
+pub fn insert_causal_edge() {}
+pub fn query_effects() {}
+pub fn propagate_confidence() {}
diff --git a/crates/mneme/src/knowledge.rs b/crates/mneme/src/knowledge.rs
--- a/crates/mneme/src/knowledge.rs
+++ b/crates/mneme/src/knowledge.rs
@@ -50,6 +50,20 @@ pub struct Relationship {
+pub enum TemporalOrdering {
+    Before,
+    After,
+    Concurrent,
+}
+
+pub struct CausalEdge {
+    pub cause: FactId,
+    pub effect: FactId,
+    pub ordering: TemporalOrdering,
+    pub confidence: f64,
+}
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -15,6 +15,7 @@ serde = "1"
+jiff = "0.2"
"#;

    #[test]
    fn extracts_lessons_from_diff() {
        let config = LessonConfig {
            pr_title: "Add causal edges".to_owned(),
            pr_number: Some(1466),
            nous_id: "test-nous".to_owned(),
            source: "pr-merge:1466".to_owned(),
        };

        let lesson = extract_lessons(PR_DIFF, &config);

        assert!(
            !lesson.entities.is_empty(),
            "should extract at least one entity"
        );
        assert!(!lesson.facts.is_empty(), "should extract at least one fact");
        assert!(
            !lesson.relationships.is_empty(),
            "should extract at least one relationship"
        );

        // Should have a PR entity.
        assert!(
            lesson
                .entities
                .iter()
                .any(|e| e.entity_type == "pull_request"),
            "should have PR entity"
        );

        // Should have file entities.
        assert!(
            lesson.entities.iter().any(|e| e.entity_type == "file"),
            "should have file entities"
        );

        // Should detect the new file.
        assert!(
            lesson.facts.iter().any(|f| f.predicate == "added"),
            "should detect new file addition"
        );

        // Should detect dependency change.
        assert!(
            lesson
                .facts
                .iter()
                .any(|f| f.predicate.contains("dependencies")),
            "should detect dependency change in Cargo.toml"
        );
    }

    #[test]
    fn classify_file_types_correctly() {
        let parsed = parse_unified_diff(PR_DIFF);
        let changes = classify_changes(&parsed);

        assert_eq!(changes.len(), 3, "three files in the diff");

        // First file is new.
        assert_eq!(
            changes[0].change_type,
            ChangeType::Added,
            "causal.rs is new"
        );
        // Second file is modified.
        assert_eq!(
            changes[1].change_type,
            ChangeType::Modified,
            "knowledge.rs is modified"
        );
        // Third file is modified.
        assert_eq!(
            changes[2].change_type,
            ChangeType::Modified,
            "Cargo.toml is modified"
        );
    }

    #[test]
    fn causal_edges_link_file_changes_to_hunk_observations() {
        let config = LessonConfig {
            pr_title: "Test PR".to_owned(),
            pr_number: Some(42),
            nous_id: "test".to_owned(),
            source: "pr-merge:42".to_owned(),
        };

        let lesson = extract_lessons(PR_DIFF, &config);

        // Causal edges should link file-level changes to hunk-level observations.
        for edge in &lesson.causal_edges {
            assert!(
                edge.cause_index < lesson.facts.len(),
                "cause index should be valid"
            );
            assert!(
                edge.effect_index < lesson.facts.len(),
                "effect index should be valid"
            );
            assert!(
                (0.0..=1.0).contains(&edge.confidence),
                "confidence should be in [0.0, 1.0]"
            );
        }
    }

    #[test]
    fn extract_module_name_works() {
        assert_eq!(
            extract_module_name("crates/mneme/src/knowledge.rs"),
            "src/knowledge.rs"
        );
        assert_eq!(extract_module_name("Cargo.toml"), "Cargo.toml");
        assert_eq!(extract_module_name("a/b/c/d.rs"), "c/d.rs");
    }
}
