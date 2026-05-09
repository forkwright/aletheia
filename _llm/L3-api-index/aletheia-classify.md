# L3 API Index: aletheia-classify

Crate path: `crates/aletheia-classify`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/classifier.rs`

```rust
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
```

```rust
impl AuthorClass {
    pub fn index (self) -> usize;
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub struct ArtifactMetadata {
    /// Metadata schema version (currently "1").
    pub schema_version: String,
    /// Classifier artifact version semver.
    pub artifact_version: String,
    /// Producer identifier (e.g. "gnomon-author-classifier@0.1.0").
    pub producer: String,
    /// Timestamp when the artifact was produced.
    pub produced_at: String,
    /// Model type (e.g. "heuristic_rule_bank").
    pub model_type: String,
    /// Array of class names in index order.
    pub classes: Vec<String>,
    /// Minimum aletheia runtime version required.
    pub runtime_version: Option<String>,
}
```

```rust
pub struct AuthorProbs {
    /// Per-class probability scores [user, subagent, system_scaffolding, template].
    pub probabilities: [f32; 4],
    /// Timestamp of classification.
    pub classified_at: Timestamp,
}
```

```rust
impl AuthorProbs {
    pub fn argmax (&self) -> AuthorClass;
    pub fn confidence (&self) -> f32;
}
```

> Author classifier: heuristic rule bank for distinguishing human-authored
> text from AI-generated continuations, echoes, and scaffolding.
>
> WHY heuristic: no ONNX artifact or embedding model is required. The rule
> bank uses surface features (length, markdown density, self-reference
> patterns, informal markers) that are cheap to compute and sufficient for
> the decontamination gate. See #3786 for evaluation results.
```rust
pub struct Classifier {
    metadata: ArtifactMetadata,
}
```

```rust
impl Classifier {
    pub fn new () -> Self;
    pub async fn load (artifact_dir: &Path) -> Result<Self>;
    pub fn classify (&self, text: &str) -> Result<AuthorProbs>;
    pub fn metadata (&self) -> &ArtifactMetadata;
}
```

## `src/error.rs`

```rust
pub enum ClassifyError {
    /// Failed to load classifier artifact from the filesystem.
    #[snafu(display("failed to load classifier artifact from {}: {source}", path.display()))]
    ArtifactMissing {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Classifier artifact version is incompatible with this runtime.
    #[snafu(display(
        "classifier artifact version incompatible: artifact schema {artifact_schema}, runtime expects {runtime_schema}"
    ))]
    VersionMismatch {
        artifact_schema: String,
        runtime_schema: String,
    },

    /// Failed to parse metadata JSON.
    #[snafu(display("failed to parse classifier metadata: {source}"))]
    InvalidMetadata { source: serde_json::Error },

    /// Input text is too long for classification.
    #[snafu(display("text too long for classification (max 100000 chars): {len} chars"))]
    TextTooLong { len: usize },

    /// Model produced invalid output shape.
    #[snafu(display(
        "classification produced invalid output shape (expected 4-element array, got {len} elements)"
    ))]
    InvalidOutputShape { len: usize },
}
```

> Result type alias for author-classifier operations.
```rust
pub type Result<T> = std::result::Result<T, ClassifyError>;
```
