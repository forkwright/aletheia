//! Fine-tune pipeline scaffold for domain-tuned embedding models.
//!
//! Provides data structures and export functionality for generating training
//! pairs from the knowledge graph. Actual training happens externally via
//! sentence-transformers or similar frameworks.
//!
//! # Training Pair Extraction
//!
//! The `export_training_pairs` function extracts (anchor, positive) pairs from
//! the knowledge graph:
//! - Fact content as anchor, related fact content as positive
//! - Entity-linked facts as positive pairs
//! - Facts from same session as weak positives

use std::path::Path;
use snafu::{ResultExt, Snafu};

#[cfg(feature = "mneme-engine")]
use crate::knowledge_store::KnowledgeStore;

/// Errors from fine-tuning operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (message, location, source) are self-documenting via display format"
)]
pub enum FinetuneError {
    /// Failed to write training pairs to output file.
    #[snafu(display("write failed: {message}"))]
    WriteFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Knowledge store query failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("store query failed: {source}"))]
    StoreQuery {
        source: crate::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No training pairs could be extracted from the store.
    #[snafu(display("no training pairs found in knowledge store"))]
    NoTrainingPairs {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization failed.
    #[snafu(display("JSON serialization failed: {source}"))]
    Serialization {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result type for fine-tuning operations.
pub type FinetuneResult<T> = std::result::Result<T, FinetuneError>;

/// Configuration for fine-tuning a domain-tuned embedding model.
///
/// This struct defines the parameters for fine-tuning. The actual training
/// is performed externally (e.g., via sentence-transformers).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FinetuneConfig {
    /// Base model name or path (e.g., "BAAI/bge-small-en-v1.5").
    pub base_model: String,
    /// Path to the training pairs file (JSONL format).
    pub training_pairs_path: std::path::PathBuf,
    /// Output path for the fine-tuned model.
    pub output_model_path: std::path::PathBuf,
    /// Number of training epochs.
    pub num_epochs: u32,
    /// Batch size for training.
    pub batch_size: u32,
    /// Learning rate (e.g., 2e-5).
    pub learning_rate: f64,
    /// Warmup ratio (fraction of total steps for warmup).
    pub warmup_ratio: f64,
    /// Maximum sequence length for tokenization.
    pub max_seq_length: usize,
    /// Evaluation steps (0 = no evaluation during training).
    pub eval_steps: usize,
    /// Save steps (0 = save only at end).
    pub save_steps: usize,
}

impl FinetuneConfig {
    /// Create a new finetune config with required fields.
    #[must_use]
    pub fn new(
        base_model: impl Into<String>,
        training_pairs_path: impl Into<std::path::PathBuf>,
        output_model_path: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            base_model: base_model.into(),
            training_pairs_path: training_pairs_path.into(),
            output_model_path: output_model_path.into(),
            num_epochs: 3,
            batch_size: 16,
            learning_rate: 2e-5,
            warmup_ratio: 0.1,
            max_seq_length: 512,
            eval_steps: 0,
            save_steps: 0,
        }
    }

    /// Set number of epochs.
    #[must_use]
    pub fn with_epochs(mut self, epochs: u32) -> Self {
        self.num_epochs = epochs;
        self
    }

    /// Set batch size.
    #[must_use]
    pub fn with_batch_size(mut self, batch_size: u32) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Set learning rate.
    #[must_use]
    pub fn with_learning_rate(mut self, lr: f64) -> Self {
        self.learning_rate = lr;
        self
    }
}

impl Default for FinetuneConfig {
    fn default() -> Self {
        Self {
            base_model: "BAAI/bge-small-en-v1.5".to_owned(),
            training_pairs_path: std::path::PathBuf::from("training_pairs.jsonl"),
            output_model_path: std::path::PathBuf::from("fine_tuned_model"),
            num_epochs: 3,
            batch_size: 16,
            learning_rate: 2e-5,
            warmup_ratio: 0.1,
            max_seq_length: 512,
            eval_steps: 0,
            save_steps: 0,
        }
    }
}

/// A single training pair for contrastive learning.
///
/// Used for MultipleNegativesRankingLoss or similar contrastive objectives.
/// The anchor is the query/fact, the positive is a similar/related text,
/// and the optional negative is a dissimilar text (if None, other batch
/// items serve as negatives).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TrainingPair {
    /// Anchor text (e.g., fact content, query).
    pub anchor: String,
    /// Positive text (semantically similar to anchor).
    pub positive: String,
    /// Optional hard negative (semantically different).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative: Option<String>,
    /// Optional metadata about the pair source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<PairSource>,
}

/// Source of a training pair for debugging/provenance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PairSource {
    /// Facts linked by the same entity.
    EntityLinked,
    /// Facts from the same session.
    SameSession,
    /// Facts with causal relationship.
    CausalEdge,
    /// Facts related by graph proximity.
    GraphProximity,
    /// Manually curated pair.
    Manual,
    /// Other source.
    Other(String),
}

/// Statistics from training pair export.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ExportStats {
    /// Total number of pairs exported.
    pub total_pairs: usize,
    /// Pairs from entity links.
    pub entity_linked_pairs: usize,
    /// Pairs from same session.
    pub session_pairs: usize,
    /// Pairs from causal relationships.
    pub causal_pairs: usize,
    /// Pairs from graph proximity.
    pub proximity_pairs: usize,
    /// Duration of export in milliseconds.
    pub duration_ms: u64,
}

/// Export training pairs from the knowledge store.
///
/// Extracts (anchor, positive) pairs from the knowledge graph:
/// - Fact content as anchor, related fact content as positive
/// - Entity-linked facts as positive pairs
/// - Facts from same session as weak positives
///
/// Writes the pairs to the output path as JSONL (one TrainingPair per line).
///
/// # Arguments
///
/// * `store` - The knowledge store to extract pairs from
/// * `output` - Path to write the JSONL file
///
/// # Returns
///
/// Number of training pairs exported, or an error if the operation fails.
///
/// # Errors
///
/// Returns `FinetuneError` if the store query fails, no pairs are found,
/// or writing to the output path fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(store))]
pub fn export_training_pairs(store: &KnowledgeStore, output: &Path) -> FinetuneResult<usize> {
    let start = Instant::now();

    // Extract pairs from various sources
    let mut all_pairs = Vec::new();

    // 1. Entity-linked facts
    let entity_pairs = extract_entity_linked_pairs(store)?;
    let entity_count = entity_pairs.len();
    all_pairs.extend(entity_pairs);

    // 2. Same-session facts
    let session_pairs = extract_same_session_pairs(store)?;
    let session_count = session_pairs.len();
    all_pairs.extend(session_pairs);

    // 3. Causally related facts
    let causal_pairs = extract_causal_pairs(store)?;
    let causal_count = causal_pairs.len();
    all_pairs.extend(causal_pairs);

    // Check if we have any pairs
    if all_pairs.is_empty() {
        return NoTrainingPairsSnafu.fail();
    }

    // Write to JSONL file
    write_training_pairs(output, &all_pairs)?;

    let duration_ms = start.elapsed().as_millis() as u64;

    tracing::info!(
        total_pairs = all_pairs.len(),
        entity_pairs = entity_count,
        session_pairs = session_count,
        causal_pairs = causal_count,
        duration_ms = duration_ms,
        "training pairs exported"
    );

    Ok(all_pairs.len())
}

/// Export training pairs with detailed statistics.
///
/// Same as `export_training_pairs` but returns detailed statistics about
/// the exported pairs.
///
/// # Errors
///
/// Returns `FinetuneError` if the store query fails, no pairs are found,
/// or writing to the output path fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(store))]
pub fn export_training_pairs_with_stats(
    store: &KnowledgeStore,
    output: &Path,
) -> FinetuneResult<ExportStats> {
    let start = Instant::now();

    // Extract pairs from various sources
    let entity_pairs = extract_entity_linked_pairs(store)?;
    let session_pairs = extract_same_session_pairs(store)?;
    let causal_pairs = extract_causal_pairs(store)?;
    let proximity_pairs = extract_proximity_pairs(store)?;

    let mut all_pairs = Vec::new();
    all_pairs.extend(entity_pairs.clone());
    all_pairs.extend(session_pairs.clone());
    all_pairs.extend(causal_pairs.clone());
    all_pairs.extend(proximity_pairs.clone());

    // Check if we have any pairs
    if all_pairs.is_empty() {
        return NoTrainingPairsSnafu.fail();
    }

    // Write to JSONL file
    write_training_pairs(output, &all_pairs)?;

    let duration_ms = start.elapsed().as_millis() as u64;

    let stats = ExportStats {
        total_pairs: all_pairs.len(),
        entity_linked_pairs: entity_pairs.len(),
        session_pairs: session_pairs.len(),
        causal_pairs: causal_pairs.len(),
        proximity_pairs: proximity_pairs.len(),
        duration_ms,
    };

    tracing::info!(
        total_pairs = stats.total_pairs,
        entity_linked_pairs = stats.entity_linked_pairs,
        session_pairs = stats.session_pairs,
        causal_pairs = stats.causal_pairs,
        proximity_pairs = stats.proximity_pairs,
        duration_ms = stats.duration_ms,
        "training pairs exported with stats"
    );

    Ok(stats)
}

/// Extract pairs from facts linked by the same entity.
#[cfg(feature = "mneme-engine")]
fn extract_entity_linked_pairs(store: &KnowledgeStore) -> FinetuneResult<Vec<TrainingPair>> {
    use std::collections::BTreeMap;

    let script = r#"
        ?[fact_id, entity_id, content] := 
            *fact_entities{fact_id, entity_id},
            *facts{id: fact_id, valid_from: vf, content: content},
            *entities{id: entity_id}
    "#;

    let result = store
        .run_query(script, BTreeMap::new())
        .map_err(|e| FinetuneError::StoreQuery {
            source: e,
        })?;

    let mut entity_facts: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();

    for row in &result.rows {
        if row.len() >= 3 {
            let fact_id = extract_string(&row[0]);
            let entity_id = extract_string(&row[1]);
            let content = extract_string(&row[2]);
            entity_facts
                .entry(entity_id)
                .or_default()
                .push((fact_id, content));
        }
    }

    let mut pairs = Vec::new();
    for (_, facts) in entity_facts {
        // Generate pairs from all combinations of facts for this entity
        for i in 0..facts.len() {
            for j in (i + 1)..facts.len() {
                pairs.push(TrainingPair {
                    anchor: facts[i].1.clone(),
                    positive: facts[j].1.clone(),
                    negative: None,
                    source: Some(PairSource::EntityLinked),
                });
            }
        }
    }

    Ok(pairs)
}

/// Extract pairs from facts in the same session.
#[cfg(feature = "mneme-engine")]
fn extract_same_session_pairs(store: &KnowledgeStore) -> FinetuneResult<Vec<TrainingPair>> {
    use std::collections::BTreeMap;

    let script = r#"
        ?[id, content, source_session_id] := 
            *facts{id, valid_from, content, source_session_id},
            source_session_id != null
    "#;

    let result = store
        .run_query(script, BTreeMap::new())
        .map_err(|e| FinetuneError::StoreQuery {
            source: e,
        })?;

    let mut session_facts: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for row in &result.rows {
        if row.len() >= 3 {
            let content = extract_string(&row[1]);
            let session_id = extract_string(&row[2]);
            session_facts.entry(session_id).or_default().push(content);
        }
    }

    let mut pairs = Vec::new();
    for (_, facts) in session_facts {
        // Generate adjacent pairs within the same session
        for i in 0..facts.len().saturating_sub(1) {
            pairs.push(TrainingPair {
                anchor: facts[i].clone(),
                positive: facts[i + 1].clone(),
                negative: None,
                source: Some(PairSource::SameSession),
            });
        }
    }

    Ok(pairs)
}

/// Extract pairs from causally related facts.
#[cfg(feature = "mneme-engine")]
fn extract_causal_pairs(store: &KnowledgeStore) -> FinetuneResult<Vec<TrainingPair>> {
    use std::collections::BTreeMap;

    let script = r#"
        ?[cause_id, effect_id, cause_content, effect_content] := 
            *causal_edges{cause: cause_id, effect: effect_id},
            *facts{id: cause_id, valid_from: vf1, content: cause_content},
            *facts{id: effect_id, valid_from: vf2, content: effect_content}
    "#;

    let result = store
        .run_query(script, BTreeMap::new())
        .map_err(|e| FinetuneError::StoreQuery {
            source: e,
        })?;

    let mut pairs = Vec::new();
    for row in &result.rows {
        if row.len() >= 4 {
            let cause_content = extract_string(&row[2]);
            let effect_content = extract_string(&row[3]);
            pairs.push(TrainingPair {
                anchor: cause_content,
                positive: effect_content,
                negative: None,
                source: Some(PairSource::CausalEdge),
            });
        }
    }

    Ok(pairs)
}

/// Extract pairs from graph proximity (placeholder implementation).
#[cfg(feature = "mneme-engine")]
fn extract_proximity_pairs(_store: &KnowledgeStore) -> FinetuneResult<Vec<TrainingPair>> {
    // Placeholder: would extract pairs based on graph proximity metrics
    // For now, return empty vector
    Ok(Vec::new())
}

/// Extract string from DataValue.
#[cfg(feature = "mneme-engine")]
fn extract_string(value: &crate::engine::DataValue) -> String {
    match value {
        crate::engine::DataValue::Str(s) => s.to_string(),
        crate::engine::DataValue::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        _ => value.to_string(),
    }
}

/// Write training pairs to a JSONL file.
fn write_training_pairs(output: &Path, pairs: &[TrainingPair]) -> FinetuneResult<()> {
    use std::io::Write;

    let mut file = std::fs::File::create(output).map_err(|e| {
        WriteFailedSnafu {
            message: format!("failed to create file: {e}"),
        }
        .build()
    })?;

    for pair in pairs {
        let line = serde_json::to_string(pair).context(SerializationSnafu)?;
        writeln!(file, "{line}").map_err(|e| {
            WriteFailedSnafu {
                message: format!("failed to write line: {e}"),
            }
            .build()
        })?;
    }

    Ok(())
}

/// Generate a training script for sentence-transformers.
///
/// Returns a Python script that can be used to train the model with the
/// exported training pairs.
pub fn generate_training_script(config: &FinetuneConfig) -> String {
    format!(
        r#"#!/usr/bin/env python3
"""
Auto-generated training script for domain-tuned embedding model.
Run this after export_training_pairs() has generated the training data.
"""

from sentence_transformers import SentenceTransformer, InputExample, losses
from torch.utils.data import DataLoader
import json

# Configuration
BASE_MODEL = "{base_model}"
TRAINING_PAIRS_PATH = "{training_pairs}"
OUTPUT_PATH = "{output}"
NUM_EPOCHS = {epochs}
BATCH_SIZE = {batch_size}
LEARNING_RATE = {lr}
WARMUP_RATIO = {warmup}
MAX_SEQ_LENGTH = {max_seq}

# Load base model
model = SentenceTransformer(BASE_MODEL)
model.max_seq_length = MAX_SEQ_LENGTH

# Load training pairs
train_examples = []
with open(TRAINING_PAIRS_PATH, 'r') as f:
    for line in f:
        data = json.loads(line)
        anchor = data['anchor']
        positive = data['positive']
        negative = data.get('negative')
        if negative:
            train_examples.append(InputExample(texts=[anchor, positive, negative]))
        else:
            train_examples.append(InputExample(texts=[anchor, positive]))

print(f"Loaded {{len(train_examples)}} training pairs")

# Create dataloader
train_dataloader = DataLoader(train_examples, shuffle=True, batch_size=BATCH_SIZE)

# Use MultipleNegativesRankingLoss (contrastive learning)
train_loss = losses.MultipleNegativesRankingLoss(model)

# Train
model.fit(
    train_objectives=[(train_dataloader, train_loss)],
    epochs=NUM_EPOCHS,
    warmup_steps=int(len(train_dataloader) * WARMUP_RATIO),
    optimizer_params={{'lr': LEARNING_RATE}},
    output_path=OUTPUT_PATH,
    show_progress_bar=True,
)

print(f"Model saved to {{OUTPUT_PATH}}")
"#,
        base_model = config.base_model,
        training_pairs = config.training_pairs_path.display(),
        output = config.output_model_path.display(),
        epochs = config.num_epochs,
        batch_size = config.batch_size,
        lr = config.learning_rate,
        warmup = config.warmup_ratio,
        max_seq = config.max_seq_length,
    )
}

/// Builder for constructing training pair exports with custom criteria.
pub struct TrainingPairExporter {
    /// Include entity-linked pairs.
    include_entity_linked: bool,
    /// Include same-session pairs.
    include_same_session: bool,
    /// Include causal relationship pairs.
    include_causal: bool,
    /// Include graph proximity pairs.
    include_proximity: bool,
    /// Maximum pairs to export (0 = unlimited).
    max_pairs: usize,
}

impl Default for TrainingPairExporter {
    fn default() -> Self {
        Self {
            include_entity_linked: true,
            include_same_session: true,
            include_causal: true,
            include_proximity: false,
            max_pairs: 0,
        }
    }
}

impl TrainingPairExporter {
    /// Create a new exporter with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether to include entity-linked pairs.
    #[must_use]
    pub fn with_entity_linked(mut self, include: bool) -> Self {
        self.include_entity_linked = include;
        self
    }

    /// Set whether to include same-session pairs.
    #[must_use]
    pub fn with_same_session(mut self, include: bool) -> Self {
        self.include_same_session = include;
        self
    }

    /// Set whether to include causal pairs.
    #[must_use]
    pub fn with_causal(mut self, include: bool) -> Self {
        self.include_causal = include;
        self
    }

    /// Set maximum pairs to export.
    #[must_use]
    pub fn with_max_pairs(mut self, max: usize) -> Self {
        self.max_pairs = max;
        self
    }

    /// Export training pairs according to configured criteria.
    ///
    /// # Errors
    ///
    /// Returns `FinetuneError` if the store query fails or writing to the output path fails.
    #[cfg(feature = "mneme-engine")]
    pub fn export(
        &self,
        store: &KnowledgeStore,
        output: &Path,
    ) -> FinetuneResult<ExportStats> {
        let start = Instant::now();
        let mut all_pairs = Vec::new();

        if self.include_entity_linked {
            let pairs = extract_entity_linked_pairs(store)?;
            all_pairs.extend(pairs);
        }

        if self.include_same_session {
            let pairs = extract_same_session_pairs(store)?;
            all_pairs.extend(pairs);
        }

        if self.include_causal {
            let pairs = extract_causal_pairs(store)?;
            all_pairs.extend(pairs);
        }

        if self.include_proximity {
            let pairs = extract_proximity_pairs(store)?;
            all_pairs.extend(pairs);
        }

        // Apply max pairs limit
        if self.max_pairs > 0 && all_pairs.len() > self.max_pairs {
            // Shuffle and truncate for random sampling
            use rand::seq::SliceRandom;
            let mut rng = rand::rng();
            all_pairs.shuffle(&mut rng);
            all_pairs.truncate(self.max_pairs);
        }

        if all_pairs.is_empty() {
            return NoTrainingPairsSnafu.fail();
        }

        write_training_pairs(output, &all_pairs)?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExportStats {
            total_pairs: all_pairs.len(),
            entity_linked_pairs: if self.include_entity_linked {
                all_pairs
                    .iter()
                    .filter(|p| p.source == Some(PairSource::EntityLinked))
                    .count()
            } else {
                0
            },
            session_pairs: if self.include_same_session {
                all_pairs
                    .iter()
                    .filter(|p| p.source == Some(PairSource::SameSession))
                    .count()
            } else {
                0
            },
            causal_pairs: if self.include_causal {
                all_pairs
                    .iter()
                    .filter(|p| p.source == Some(PairSource::CausalEdge))
                    .count()
            } else {
                0
            },
            proximity_pairs: 0,
            duration_ms,
        })
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic")]
mod tests {
    use super::*;

    #[test]
    fn finetune_config_builder() {
        let config = FinetuneConfig::new(
            "BAAI/bge-small-en-v1.5",
            "/data/train.jsonl",
            "/models/output",
        )
        .with_epochs(5)
        .with_batch_size(32)
        .with_learning_rate(1e-5);

        assert_eq!(config.base_model, "BAAI/bge-small-en-v1.5");
        assert_eq!(config.num_epochs, 5);
        assert_eq!(config.batch_size, 32);
        assert!((config.learning_rate - 1e-5).abs() < f64::EPSILON);
    }

    #[test]
    fn training_pair_serialization() {
        let pair = TrainingPair {
            anchor: "What is Rust?".to_owned(),
            positive: "Rust is a systems programming language.".to_owned(),
            negative: Some("Python is a scripting language.".to_owned()),
            source: Some(PairSource::EntityLinked),
        };

        let json = serde_json::to_string(&pair).expect("serialize");
        assert!(json.contains("What is Rust?"));
        assert!(json.contains("entity_linked"));

        let deserialized: TrainingPair = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(pair, deserialized);
    }

    #[test]
    fn export_stats_serialization() {
        let stats = ExportStats {
            total_pairs: 100,
            entity_linked_pairs: 50,
            session_pairs: 30,
            causal_pairs: 20,
            proximity_pairs: 0,
            duration_ms: 500,
        };

        let json = serde_json::to_string(&stats).expect("serialize");
        assert!(json.contains("100"));
        assert!(json.contains("50"));
    }

    #[test]
    fn exporter_builder() {
        let exporter = TrainingPairExporter::new()
            .with_entity_linked(true)
            .with_same_session(false)
            .with_causal(true)
            .with_max_pairs(1000);

        assert!(exporter.include_entity_linked);
        assert!(!exporter.include_same_session);
        assert!(exporter.include_causal);
        assert_eq!(exporter.max_pairs, 1000);
    }

    #[test]
    fn generate_script_contains_config() {
        let config = FinetuneConfig::new("model", "train.jsonl", "output")
            .with_epochs(10)
            .with_batch_size(64);

        let script = generate_training_script(&config);
        assert!(script.contains("model"));
        assert!(script.contains("train.jsonl"));
        assert!(script.contains("output"));
        assert!(script.contains("10"));
        assert!(script.contains("64"));
    }

    #[test]
    fn write_and_read_training_pairs() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let output_path = temp_dir.path().join("pairs.jsonl");

        let pairs = vec![
            TrainingPair {
                anchor: "anchor1".to_owned(),
                positive: "positive1".to_owned(),
                negative: None,
                source: Some(PairSource::Manual),
            },
            TrainingPair {
                anchor: "anchor2".to_owned(),
                positive: "positive2".to_owned(),
                negative: Some("negative2".to_owned()),
                source: Some(PairSource::SameSession),
            },
        ];

        write_training_pairs(&output_path, &pairs).expect("write pairs");

        // Read back
        let contents = std::fs::read_to_string(&output_path).expect("read file");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        let parsed: TrainingPair = serde_json::from_str(lines[0]).expect("parse");
        assert_eq!(parsed.anchor, "anchor1");
    }

    #[test]
    fn pair_source_roundtrip() {
        let sources = vec![
            PairSource::EntityLinked,
            PairSource::SameSession,
            PairSource::CausalEdge,
            PairSource::GraphProximity,
            PairSource::Manual,
            PairSource::Other("custom".to_owned()),
        ];

        for source in sources {
            let json = serde_json::to_string(&source).expect("serialize");
            let deserialized: PairSource = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(source, deserialized);
        }
    }
}
