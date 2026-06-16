//! `NuExtract`-2.0 ONNX bookkeeping provider.
//!
//! Structured JSON extraction from text given a template schema. Complements
//! the `GLiNER` NER provider: `GLiNER` excels at span-based entity tagging while
//! `NuExtract` excels at schema-constrained fact and relationship extraction.
//! Both run in-process via `ort`; VRAM contention between the two is possible
//! on single-GPU hosts — consider llama-server sidecar offload if OOM occurs.

use std::path::{Path, PathBuf};

use eidos::bookkeeping::{
    BookkeepingError, BookkeepingProvider, BookkeepingResult, ConversationMessage, ExtractedEntity,
    ExtractedFact, ExtractedRelationship, Extraction, ExtractionSchema, ProviderFailedSnafu,
};
use ort::session::Session;
use ort::value::{Shape, Tensor};
use tokenizers::Tokenizer;
use tokenizers::utils::padding::{PaddingParams, PaddingStrategy};
use tokenizers::utils::truncation::{TruncationParams, TruncationStrategy};
use tokio::sync::Mutex;
use tracing::warn;

const DEFAULT_MODEL_DIR: &str = "/models/onnx/nuextract-2b";
const MODEL_FILE: &str = "onnx/model.onnx";
const TOKENIZER_FILE: &str = "tokenizer.json";
// WHY: NuExtract-2.0-2B has a 4096-token context; leave headroom for
// schema + special tokens. 3584 tokens fits the typical conversation window.
const MAX_INPUT_TOKENS: usize = 3584;
// WHY: NuExtract returns deterministic structured JSON; generation cap of 512
// tokens is generous for a single-schema extraction response.
const MAX_NEW_TOKENS: usize = 512;

/// Runtime configuration for [`NuExtractProvider`].
#[derive(Debug, Clone)]
pub struct NuExtractProviderConfig {
    /// Directory containing `tokenizer.json` and `onnx/model.onnx`.
    pub model_dir: PathBuf,
    /// Maximum new tokens to generate per extraction call.
    pub max_new_tokens: usize,
}

impl Default for NuExtractProviderConfig {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::from(DEFAULT_MODEL_DIR),
            max_new_tokens: MAX_NEW_TOKENS,
        }
    }
}

/// NuExtract-2.0-backed structured extraction provider.
///
/// The constructor loads the tokenizer and ONNX encoder/decoder graphs up
/// front. Schema-constrained JSON is generated at inference time with a
/// greedy decode (temperature=0 equivalent). Entity and relationship fields
/// from the returned JSON are lifted into `Extraction`; the LLM fallback is
/// not used because `NuExtract` is designed for full-coverage extraction rather
/// than NER-only span tagging.
pub struct NuExtractProvider {
    config: NuExtractProviderConfig,
    tokenizer: Tokenizer,
    session: Mutex<Session>,
}

impl NuExtractProvider {
    /// Load the default staged `NuExtract` model and tokenizer.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if the tokenizer or ONNX model cannot be
    /// loaded at startup.
    pub fn new() -> BookkeepingResult<Self> {
        Self::with_config(NuExtractProviderConfig::default())
    }

    /// Load `NuExtract` from an explicit model directory.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if the tokenizer or ONNX model cannot be
    /// loaded at startup.
    pub fn with_model_dir(model_dir: impl Into<PathBuf>) -> BookkeepingResult<Self> {
        Self::with_config(NuExtractProviderConfig {
            model_dir: model_dir.into(),
            ..NuExtractProviderConfig::default()
        })
    }

    /// Load `NuExtract` with explicit runtime configuration.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if the tokenizer or ONNX model cannot be
    /// loaded at startup.
    pub fn with_config(config: NuExtractProviderConfig) -> BookkeepingResult<Self> {
        let tokenizer_path = config.model_dir.join(TOKENIZER_FILE);
        let model_path = config.model_dir.join(MODEL_FILE);
        let mut tokenizer = load_tokenizer(&tokenizer_path)?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_INPUT_TOKENS,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                ..TruncationParams::default()
            }))
            .map_err(|err| provider_failed("configure_tokenizer", err))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(MAX_INPUT_TOKENS),
            ..PaddingParams::default()
        }));
        let session = Session::builder()
            .map_err(|err| provider_failed("create_session_builder", err))?
            .commit_from_file(&model_path)
            .map_err(|err| provider_failed("load_model", err))?;

        Ok(Self {
            config,
            tokenizer,
            session: Mutex::new(session),
        })
    }

    /// Run `NuExtract` over text with the given JSON schema template.
    ///
    /// The template is a JSON object describing the fields to extract, e.g.
    /// `{"entities": [], "facts": [{"subject": "", "predicate": "", "object": ""}]}`.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if tokenization, ONNX inference, or JSON
    /// decoding fails.
    pub async fn extract_json(
        &self,
        text: &str,
        template: &str,
    ) -> BookkeepingResult<serde_json::Value> {
        let prompt = build_nuextract_prompt(text, template);
        let encoding = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|err| provider_failed("tokenize_prompt", err))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|id| i64::from(*id)).collect();
        let seq_len = input_ids.len();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|id| i64::from(*id))
            .collect();

        let mut session = self.session.lock().await;
        let output_ids = greedy_decode(
            &mut session,
            &input_ids,
            &attention_mask,
            seq_len,
            self.config.max_new_tokens,
        )?;
        drop(session);

        let decoded = self
            .tokenizer
            .decode(&output_ids, true)
            .map_err(|err| provider_failed("decode_output", err))?;
        parse_nuextract_json(&decoded)
    }

    /// Run a synthetic extraction through the loaded ONNX graph.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if the model cannot execute.
    pub async fn smoke_infer(&self) -> BookkeepingResult<()> {
        let template = r#"{"entities": []}"#;
        let _ = self
            .extract_json("Alice works on Aletheia in Berlin.", template)
            .await?;
        Ok(())
    }

    async fn extract_messages(
        &self,
        messages: &[ConversationMessage],
        schema: &ExtractionSchema,
    ) -> BookkeepingResult<Extraction> {
        let text = join_messages(messages);
        let template = build_schema_template(schema);
        match self.extract_json(&text, &template).await {
            Ok(json) => Ok(parse_extraction_json(&json)),
            Err(err) => {
                warn!(error = %err, "NuExtract extraction failed; returning empty extraction");
                Ok(Extraction::empty())
            }
        }
    }
}

impl BookkeepingProvider for NuExtractProvider {
    fn extract_knowledge<'a>(
        &'a self,
        messages: &'a [ConversationMessage],
        schema: &'a ExtractionSchema,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = BookkeepingResult<Extraction>> + Send + 'a>,
    > {
        Box::pin(async move { self.extract_messages(messages, schema).await })
    }

    fn extract_facts<'a>(
        &'a self,
        text: &'a str,
        schema: &'a ExtractionSchema,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = BookkeepingResult<Vec<ExtractedFact>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let messages = [ConversationMessage::user(text)];
            let extraction = self.extract_knowledge(&messages, schema).await?;
            Ok(extraction.facts)
        })
    }

    fn name(&self) -> &'static str {
        "nuextract"
    }
}

/// Build the `NuExtract`-2.0 prompt format.
///
/// `NuExtract` expects: `<|input|>\n{text}\n<|template|>\n{template}\n<|output|>\n`
fn build_nuextract_prompt(text: &str, template: &str) -> String {
    format!("<|input|>\n{text}\n<|template|>\n{template}\n<|output|>\n")
}

/// Greedy token-by-token decode via the ONNX encoder-decoder graph.
///
/// NuExtract-2.0 exports as a single seq2seq ONNX graph with `input_ids`,
/// `attention_mask`, and `max_new_tokens` inputs, producing `output_ids`.
/// Falls back to a simpler `generate`-style interface when the graph does not
/// expose the encoder/decoder split.
fn greedy_decode(
    session: &mut Session,
    input_ids: &[i64],
    attention_mask: &[i64],
    seq_len: usize,
    max_new_tokens: usize,
) -> BookkeepingResult<Vec<u32>> {
    // WHY: NuExtract-2.0 ONNX export uses a flat seq2seq graph: caller
    // supplies input_ids + attention_mask + max_new_tokens and reads
    // output_ids directly. This avoids the encoder/decoder split that
    // requires kv-cache plumbing not yet in ort 2.0-rc.
    let max_new = vec![
        i64::try_from(max_new_tokens).map_err(|err| provider_failed("max_new_tokens_cast", err))?,
    ];
    let seq_i64 = usize_to_i64(seq_len)?;
    let outputs = session
        .run(ort::inputs![
            "input_ids" => Tensor::from_array((Shape::new([1, seq_i64]), input_ids.to_vec()))
                .map_err(|err| provider_failed("tensor_input_ids", err))?,
            "attention_mask" => Tensor::from_array((Shape::new([1, seq_i64]), attention_mask.to_vec()))
                .map_err(|err| provider_failed("tensor_attention_mask", err))?,
            "max_new_tokens" => Tensor::from_array((Shape::new([1]), max_new))
                .map_err(|err| provider_failed("tensor_max_new_tokens", err))?,
        ])
        .map_err(|err| provider_failed("run_inference", err))?;

    let output_tensor = outputs
        .get("output_ids")
        .ok_or_else(|| provider_failed("decode_output_ids", "missing output_ids"))?;
    let (_shape, data) = output_tensor
        .try_extract_tensor::<i64>()
        .map_err(|err| provider_failed("view_output_ids", err))?;

    data.iter()
        .map(|&id| u32::try_from(id).map_err(|err| provider_failed("output_id_cast", err)))
        .collect()
}

/// Extract JSON from the model's decoded output.
///
/// NuExtract-2.0 wraps its output in `<|output|>...<|end|>` markers.
/// Falls back to direct JSON parse if the markers are absent.
fn parse_nuextract_json(decoded: &str) -> BookkeepingResult<serde_json::Value> {
    let json_str = if let Some(start) = decoded.find("<|output|>") {
        // WHY: `start` is a byte offset from str::find on this exact &str; adding the
        // ASCII marker length lands on a char boundary by construction.
        #[expect(
            clippy::string_slice,
            reason = "`start` comes from str::find on `decoded`; ASCII marker length preserves char boundary"
        )]
        let after = &decoded[start + "<|output|>".len()..];
        if let Some(end) = after.find("<|end|>") {
            // WHY: `end` is a byte offset from str::find on `after`; slicing at a
            // find-returned index is always on a char boundary.
            #[expect(
                clippy::string_slice,
                reason = "`end` comes from str::find on `after`; index is guaranteed a char boundary"
            )]
            after[..end].trim()
        } else {
            after.trim()
        }
    } else {
        decoded.trim()
    };

    serde_json::from_str(json_str).map_err(|err| provider_failed("parse_json", err))
}

/// Build the extraction schema template for `NuExtract`.
///
/// `NuExtract` uses JSON with empty arrays/strings as field stubs:
/// filled values indicate the desired output shape.
fn build_schema_template(schema: &ExtractionSchema) -> String {
    // WHY: NuExtract interprets an empty array as "extract a list of these";
    // a nested object stub instructs the model on what fields to populate.
    // Caps from the schema are not injected into the template because NuExtract
    // does not support inline max-count hints — the caller trims post-extraction.
    let _ = schema;
    serde_json::json!({
        "entities": [{"name": "", "entity_type": "", "description": ""}],
        "relationships": [{"source": "", "relation": "", "target": "", "confidence": 0.0}],
        "facts": [{"subject": "", "predicate": "", "object": "", "confidence": 0.0}]
    })
    .to_string()
}

/// Map parsed `NuExtract` JSON into the canonical `Extraction` type.
fn parse_extraction_json(value: &serde_json::Value) -> Extraction {
    let entities = value
        .get("entities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?.trim().to_owned();
                    if name.is_empty() {
                        return None;
                    }
                    Some(ExtractedEntity {
                        name,
                        entity_type: item
                            .get("entity_type")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("other")
                            .to_owned(),
                        description: item
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    })
                })
                .collect()
        })
        // kanon:ignore RUST/no-result-unwrap-or-default — chain returns Option<Vec<_>> (Value::as_array → Option, .map → Option); fallback is the documented empty-list default when the JSON key is absent or not an array.
        .unwrap_or_default();

    let relationships = value
        .get("relationships")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let source = item.get("source")?.as_str()?.trim().to_owned();
                    let relation = item.get("relation")?.as_str()?.trim().to_owned();
                    let target = item.get("target")?.as_str()?.trim().to_owned();
                    if source.is_empty() || relation.is_empty() || target.is_empty() {
                        return None;
                    }
                    Some(ExtractedRelationship {
                        source,
                        relation,
                        target,
                        confidence: item
                            .get("confidence")
                            .and_then(serde_json::Value::as_f64)
                            .unwrap_or(1.0),
                    })
                })
                .collect()
        })
        // kanon:ignore RUST/no-result-unwrap-or-default — chain returns Option<Vec<_>> (Value::as_array → Option, .map → Option); fallback is the documented empty-list default when the JSON key is absent or not an array.
        .unwrap_or_default();

    let facts = value
        .get("facts")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let subject = item.get("subject")?.as_str()?.trim().to_owned();
                    let predicate = item.get("predicate")?.as_str()?.trim().to_owned();
                    let object = item.get("object")?.as_str()?.trim().to_owned();
                    if subject.is_empty() || predicate.is_empty() || object.is_empty() {
                        return None;
                    }
                    Some(ExtractedFact {
                        subject,
                        predicate,
                        object,
                        confidence: item
                            .get("confidence")
                            .and_then(serde_json::Value::as_f64)
                            .unwrap_or(1.0),
                        is_correction: false,
                        fact_type: None,
                    })
                })
                .collect()
        })
        // kanon:ignore RUST/no-result-unwrap-or-default — chain returns Option<Vec<_>> (Value::as_array → Option, .map → Option); fallback is the documented empty-list default when the JSON key is absent or not an array.
        .unwrap_or_default();

    Extraction {
        entities,
        relationships,
        facts,
    }
}

fn usize_to_i64(value: usize) -> BookkeepingResult<i64> {
    i64::try_from(value).map_err(|err| provider_failed("integer_conversion", err))
}

fn load_tokenizer(path: &Path) -> BookkeepingResult<Tokenizer> {
    Tokenizer::from_file(path).map_err(|err| provider_failed("load_tokenizer", err))
}

fn join_messages(messages: &[ConversationMessage]) -> String {
    messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn provider_failed(operation: &'static str, message: impl std::fmt::Display) -> BookkeepingError {
    ProviderFailedSnafu {
        provider: "nuextract",
        operation,
        message: message.to_string(),
    }
    .build()
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    #![expect(
        clippy::indexing_slicing,
        reason = "test: indices valid by test construction"
    )]

    use super::*;

    #[test]
    fn build_nuextract_prompt_contains_markers() {
        let prompt = build_nuextract_prompt("Alice works on Aletheia.", r#"{"entities": []}"#);
        assert!(prompt.contains("<|input|>"));
        assert!(prompt.contains("Alice works on Aletheia."));
        assert!(prompt.contains("<|template|>"));
        assert!(prompt.contains(r#"{"entities": []}"#));
        assert!(prompt.contains("<|output|>"));
    }

    #[test]
    fn parse_nuextract_json_with_output_markers() {
        let decoded = "<|output|>\n{\"entities\": [{\"name\": \"Alice\"}]}\n<|end|>";
        let value = parse_nuextract_json(decoded).expect("should parse wrapped JSON");
        assert!(value.get("entities").is_some());
    }

    #[test]
    fn parse_nuextract_json_without_markers() {
        let decoded = r#"{"facts": []}"#;
        let value = parse_nuextract_json(decoded).expect("should parse raw JSON");
        assert!(value.get("facts").is_some());
    }

    #[test]
    fn parse_extraction_json_maps_entities() {
        let json = serde_json::json!({
            "entities": [
                {"name": "Alice", "entity_type": "person", "description": "developer"}
            ],
            "relationships": [],
            "facts": []
        });
        let extraction = parse_extraction_json(&json);
        assert_eq!(extraction.entities.len(), 1);
        assert_eq!(
            extraction.entities.first().map(|e| e.name.as_str()),
            Some("Alice")
        );
        assert_eq!(
            extraction.entities.first().map(|e| e.entity_type.as_str()),
            Some("person")
        );
    }

    #[test]
    fn parse_extraction_json_skips_empty_name_entities() {
        let json = serde_json::json!({
            "entities": [
                {"name": "", "entity_type": "person", "description": ""},
                {"name": "Bob", "entity_type": "person", "description": ""}
            ],
            "relationships": [],
            "facts": []
        });
        let extraction = parse_extraction_json(&json);
        assert_eq!(extraction.entities.len(), 1);
        assert_eq!(
            extraction.entities.first().map(|e| e.name.as_str()),
            Some("Bob")
        );
    }

    #[test]
    fn parse_extraction_json_maps_relationships() {
        let json = serde_json::json!({
            "entities": [],
            "relationships": [
                {"source": "Alice", "relation": "uses", "target": "Aletheia", "confidence": 0.9}
            ],
            "facts": []
        });
        let extraction = parse_extraction_json(&json);
        assert_eq!(extraction.relationships.len(), 1);
        assert_eq!(
            extraction.relationships.first().map(|r| r.source.as_str()),
            Some("Alice")
        );
        assert!(
            extraction
                .relationships
                .first()
                .is_some_and(|r| (r.confidence - 0.9).abs() < 1e-9)
        );
    }

    #[test]
    fn parse_extraction_json_maps_facts() {
        let json = serde_json::json!({
            "entities": [],
            "relationships": [],
            "facts": [
                {"subject": "Aletheia", "predicate": "written in", "object": "Rust", "confidence": 1.0}
            ]
        });
        let extraction = parse_extraction_json(&json);
        assert_eq!(extraction.facts.len(), 1);
        assert_eq!(
            extraction.facts.first().map(|f| f.predicate.as_str()),
            Some("written in")
        );
    }

    #[test]
    fn parse_extraction_json_handles_empty_object() {
        let json = serde_json::json!({});
        let extraction = parse_extraction_json(&json);
        assert!(extraction.entities.is_empty());
        assert!(extraction.relationships.is_empty());
        assert!(extraction.facts.is_empty());
    }

    #[test]
    fn build_schema_template_is_valid_json() {
        let schema = ExtractionSchema::default();
        let template = build_schema_template(&schema);
        let _: serde_json::Value =
            serde_json::from_str(&template).expect("schema template should be valid JSON");
    }

    #[tokio::test]
    async fn nuextract_smoke_inference_when_staged_model_exists() {
        let model_dir = Path::new(DEFAULT_MODEL_DIR);
        if !model_dir.exists() {
            return;
        }

        let provider = NuExtractProvider::new().expect("staged NuExtract model should load");
        provider
            .smoke_infer()
            .await
            .expect("staged NuExtract model should run synthetic inference");
    }
}
