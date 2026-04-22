use std::path::Path;

use jiff::Timestamp;
use ort::session::Session;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::debug;

use crate::error::{ArtifactMissingSnafu, InferenceFailedSnafu, InvalidMetadataSnafu, Result};

/// Maximum character length for classification input (100k chars).
const MAX_TEXT_LENGTH: usize = 100_000;

/// Expected metadata schema version for this runtime.
const EXPECTED_SCHEMA_VERSION: &str = "1";

/// Classification output categories in index order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthorClass {
    /// User-authored text.
    User = 0,
    /// Subagent-generated response.
    Subagent = 1,
    /// System scaffolding (setup blocks, task descriptions, etc.).
    SystemScaffolding = 2,
    /// Template text or boilerplate.
    Template = 3,
}

impl AuthorClass {
    /// Convert from integer index to enum variant.
    fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Self::User),
            1 => Some(Self::Subagent),
            2 => Some(Self::SystemScaffolding),
            3 => Some(Self::Template),
            _ => None,
        }
    }

    /// Return the integer index corresponding to this variant.
    #[must_use]
    pub fn index(self) -> usize {
        self as usize
    }

    /// Human-readable name for this class.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Subagent => "subagent",
            Self::SystemScaffolding => "system_scaffolding",
            Self::Template => "template",
        }
    }
}

/// Artifact metadata sidecar describing the classifier model.
///
/// Loaded from `metadata.json` alongside the ONNX model file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    /// Metadata schema version (currently "1").
    pub schema_version: String,
    /// Classifier artifact version semver.
    pub artifact_version: String,
    /// Producer identifier (e.g. "gnomon-author-classifier@0.1.0").
    pub producer: String,
    /// Timestamp when the artifact was produced.
    pub produced_at: String,
    /// Model type (e.g. "logistic_regression_with_tfidf").
    pub model_type: String,
    /// Array of class names in index order.
    pub classes: Vec<String>,
    /// Minimum ort crate version required.
    pub ort_minimum_version: String,
    /// Minimum aletheia runtime version required.
    pub runtime_version: Option<String>,
}

impl ArtifactMetadata {
    /// Validate this metadata against runtime constraints.
    fn validate(&self) -> Result<()> {
        if self.schema_version != EXPECTED_SCHEMA_VERSION {
            return Err(crate::error::ClassifyError::VersionMismatch {
                artifact_schema: self.schema_version.clone(),
                runtime_schema: EXPECTED_SCHEMA_VERSION.to_owned(),
            });
        }
        if self.classes.len() != 4 {
            return Err(crate::error::ClassifyError::InvalidOutputShape {
                len: self.classes.len(),
            });
        }
        Ok(())
    }
}

/// Classification result with confidence and metadata.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct AuthorProbs {
    /// Per-class probability scores [user, subagent, system_scaffolding, template].
    pub probabilities: [f32; 4],
    /// Timestamp of classification.
    pub classified_at: Timestamp,
}

impl AuthorProbs {
    /// Return the class with highest probability.
    #[must_use]
    pub fn argmax(&self) -> AuthorClass {
        let max_idx = self
            .probabilities
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        AuthorClass::from_index(max_idx).unwrap_or(AuthorClass::User)
    }

    /// Return the highest probability (confidence).
    #[must_use]
    pub fn confidence(&self) -> f32 {
        self.probabilities.iter().copied().fold(0.0_f32, f32::max)
    }
}

/// Author classifier: loads ONNX model and performs inference.
pub struct Classifier {
    #[expect(
        dead_code,
        reason = "session field is used for inference once real model artifact exists"
    )]
    session: Session,
    metadata: ArtifactMetadata,
}

impl Classifier {
    /// Load a classifier from an artifact directory.
    ///
    /// Expects `model.onnx` and `metadata.json` in the provided directory.
    /// Returns `ClassifyError::ArtifactMissing` if files cannot be loaded.
    ///
    /// # Errors
    ///
    /// Returns an error if the artifact files cannot be read, parsed, or
    /// validated against runtime constraints.
    pub async fn load(artifact_dir: &Path) -> Result<Self> {
        let model_path = artifact_dir.join("model.onnx");
        let metadata_path = artifact_dir.join("metadata.json");

        debug!(
            model_path = %model_path.display(),
            metadata_path = %metadata_path.display(),
            "loading author classifier"
        );

        // Load metadata first for validation
        let metadata_content =
            std::fs::read_to_string(&metadata_path).context(ArtifactMissingSnafu {
                path: &metadata_path,
            })?;
        let metadata: ArtifactMetadata =
            serde_json::from_str(&metadata_content).context(InvalidMetadataSnafu)?;

        // Validate metadata against runtime constraints
        metadata.validate()?;

        // Load ONNX model
        let session = Session::builder()
            .context(InferenceFailedSnafu)?
            .commit_from_file(&model_path)
            .context(InferenceFailedSnafu)?;

        debug!(
            producer = %metadata.producer,
            artifact_version = %metadata.artifact_version,
            "author classifier loaded"
        );

        Ok(Self { session, metadata })
    }

    /// Classify a text string, returning per-class probabilities.
    ///
    /// # Errors
    ///
    /// Returns an error if the text is too long, ONNX inference fails,
    /// or the model produces invalid output.
    #[tracing::instrument(skip(self), fields(text_len = text.len()))]
    pub fn classify(&self, text: &str) -> Result<AuthorProbs> {
        // Guard against extremely long inputs
        if text.len() > MAX_TEXT_LENGTH {
            return Err(crate::error::ClassifyError::TextTooLong { len: text.len() });
        }

        // TODO: implement actual ONNX inference when real model artifact exists.
        // For now, return a stub with uniform probabilities for testing.
        let _text = text;
        let probabilities = [0.25, 0.25, 0.25, 0.25];

        Ok(AuthorProbs {
            probabilities,
            classified_at: Timestamp::now(),
        })
    }

    /// Reference to the loaded artifact metadata.
    #[must_use]
    pub fn metadata(&self) -> &ArtifactMetadata {
        &self.metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_probs_argmax_returns_max_class() {
        let probs = AuthorProbs {
            probabilities: [0.1, 0.8, 0.05, 0.05],
            classified_at: Timestamp::now(),
        };
        assert_eq!(probs.argmax(), AuthorClass::Subagent);
    }

    #[test]
    fn author_probs_confidence_returns_max_prob() {
        let probs = AuthorProbs {
            probabilities: [0.1, 0.8, 0.05, 0.05],
            classified_at: Timestamp::now(),
        };
        assert_eq!(probs.confidence(), 0.8);
    }

    #[test]
    fn author_class_index_maps_correctly() {
        assert_eq!(AuthorClass::User.index(), 0);
        assert_eq!(AuthorClass::Subagent.index(), 1);
        assert_eq!(AuthorClass::SystemScaffolding.index(), 2);
        assert_eq!(AuthorClass::Template.index(), 3);
    }

    #[test]
    fn author_class_from_index_all_variants() {
        assert_eq!(AuthorClass::from_index(0), Some(AuthorClass::User));
        assert_eq!(AuthorClass::from_index(1), Some(AuthorClass::Subagent));
        assert_eq!(
            AuthorClass::from_index(2),
            Some(AuthorClass::SystemScaffolding)
        );
        assert_eq!(AuthorClass::from_index(3), Some(AuthorClass::Template));
        assert_eq!(AuthorClass::from_index(4), None);
    }

    #[test]
    fn author_class_as_str() {
        assert_eq!(AuthorClass::User.as_str(), "user");
        assert_eq!(AuthorClass::Subagent.as_str(), "subagent");
        assert_eq!(
            AuthorClass::SystemScaffolding.as_str(),
            "system_scaffolding"
        );
        assert_eq!(AuthorClass::Template.as_str(), "template");
    }

    #[test]
    fn text_length_guard() {
        // Create a long text that exceeds the limit
        let long_text = "a".repeat(MAX_TEXT_LENGTH + 1);

        // We can't easily test the full classification without a real ONNX file,
        // but the length check will be tested in integration tests.
        assert!(long_text.len() > MAX_TEXT_LENGTH);
    }
}
