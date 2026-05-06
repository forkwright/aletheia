//! `GLiNER` ONNX bookkeeping provider.
//!
//! Uses `ort` because the staged `GLiNER` graph hits a `tract-onnx`
//! `Unsqueeze13` symbolic-shape failure during inference. ONNX Runtime still
//! runs in-process here. The staged `GLiNER` model is span-based NER, so this
//! provider extracts entity spans locally and delegates relationships and fact
//! triples to the LLM compatibility provider.

use std::path::{Path, PathBuf};

use eidos::bookkeeping::{
    BookkeepingError, BookkeepingProvider, BookkeepingResult, ConversationMessage, ExtractedEntity,
    ExtractedFact, Extraction, ExtractionSchema, ProviderFailedSnafu,
};
use ort::session::Session;
use ort::value::{Shape, Tensor};
use tokenizers::Tokenizer;
use tokenizers::utils::padding::{PaddingParams, PaddingStrategy};
use tokenizers::utils::truncation::{TruncationParams, TruncationStrategy};
use tokio::sync::Mutex;
use tracing::warn;

use super::LlmBookkeepingProvider;
use crate::extract::ExtractionProvider;
use crate::extract::engine::ExtractionEngine;

const DEFAULT_MODEL_DIR: &str = "/models/onnx/gliner_multi-v2.1";
const MODEL_FILE: &str = "onnx/model_int8.onnx";
const TOKENIZER_FILE: &str = "tokenizer.json";
const MAX_INPUT_TOKENS: usize = 384;
const PROMPT_WORDS: usize = ENTITY_LABELS.len() * 2 + 1;
const MAX_TEXT_WORDS: usize = MAX_INPUT_TOKENS - PROMPT_WORDS;
const MAX_SPAN_WIDTH: usize = 12;
const DEFAULT_THRESHOLD: f32 = 0.5;
const ENTITY_LABELS: [&str; 6] = [
    "person",
    "project",
    "concept",
    "tool",
    "location",
    "organization",
];

/// Runtime configuration for [`GlinerExtractionProvider`].
#[derive(Debug, Clone)]
pub struct GlinerProviderConfig {
    /// Directory containing `tokenizer.json` and `onnx/model_int8.onnx`.
    pub model_dir: PathBuf,
    /// Minimum sigmoid score for an entity span.
    pub threshold: f32,
}

impl Default for GlinerProviderConfig {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::from(DEFAULT_MODEL_DIR),
            threshold: DEFAULT_THRESHOLD,
        }
    }
}

/// GLiNER-backed extraction provider with LLM fallback.
///
/// The constructor loads the tokenizer and ONNX graph up front. Entity spans
/// are decoded from `GLiNER` logits; relationships and subject-predicate-object
/// facts remain on the LLM fallback because this model artifact is NER-only.
pub struct GlinerExtractionProvider<'a> {
    config: GlinerProviderConfig,
    tokenizer: Tokenizer,
    session: Mutex<Session>,
    fallback: LlmBookkeepingProvider<'a>,
}

#[derive(Debug)]
struct GlinerEntity {
    name: String,
    entity_type: String,
    score: f32,
}

impl<'a> GlinerExtractionProvider<'a> {
    /// Load the default staged `GLiNER` model and tokenizer.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if the tokenizer or ONNX model cannot be
    /// loaded at startup.
    pub fn new(
        engine: &'a ExtractionEngine,
        provider: &'a dyn ExtractionProvider,
    ) -> BookkeepingResult<Self> {
        Self::with_config(engine, provider, GlinerProviderConfig::default())
    }

    /// Load `GLiNER` from an explicit model directory.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if the tokenizer or ONNX model cannot be
    /// loaded at startup.
    pub fn with_model_dir(
        engine: &'a ExtractionEngine,
        provider: &'a dyn ExtractionProvider,
        model_dir: impl Into<PathBuf>,
    ) -> BookkeepingResult<Self> {
        Self::with_config(
            engine,
            provider,
            GlinerProviderConfig {
                model_dir: model_dir.into(),
                ..GlinerProviderConfig::default()
            },
        )
    }

    /// Load `GLiNER` with explicit runtime configuration.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if the tokenizer or ONNX model cannot be
    /// loaded at startup.
    pub fn with_config(
        engine: &'a ExtractionEngine,
        provider: &'a dyn ExtractionProvider,
        config: GlinerProviderConfig,
    ) -> BookkeepingResult<Self> {
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
            fallback: LlmBookkeepingProvider::new(engine, provider),
        })
    }

    /// Run `GLiNER` over a plain text span and return entity candidates.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if tokenization or ONNX inference fails.
    pub async fn extract_entities(&self, text: &str) -> BookkeepingResult<Vec<ExtractedEntity>> {
        let words = split_words(text);
        if words.is_empty() {
            return Ok(Vec::new());
        }

        let input = build_input(&self.tokenizer, &words)?;
        let mut session = self.session.lock().await;
        let outputs = session
            .run(ort::inputs![
                "input_ids" => Tensor::from_array((Shape::new([1, usize_to_i64(input.input_ids.len())?]), input.input_ids))
                    .map_err(|err| provider_failed("tensor_input_ids", err))?,
                "attention_mask" => Tensor::from_array((Shape::new([1, usize_to_i64(input.attention_mask.len())?]), input.attention_mask))
                    .map_err(|err| provider_failed("tensor_attention_mask", err))?,
                "words_mask" => Tensor::from_array((Shape::new([1, usize_to_i64(input.words_mask.len())?]), input.words_mask))
                    .map_err(|err| provider_failed("tensor_words_mask", err))?,
                "text_lengths" => Tensor::from_array((Shape::new([1, 1]), input.text_lengths))
                    .map_err(|err| provider_failed("tensor_text_lengths", err))?,
                "span_idx" => Tensor::from_array((Shape::new([1, usize_to_i64(input.span_idx.len() / 2)?, 2]), input.span_idx))
                    .map_err(|err| provider_failed("tensor_span_idx", err))?,
                "span_mask" => Tensor::from_array((Shape::new([1, usize_to_i64(input.span_mask.len())?]), input.span_mask))
                    .map_err(|err| provider_failed("tensor_span_mask", err))?,
            ])
            .map_err(|err| provider_failed("run_inference", err))?;
        let logits = outputs
            .get("logits")
            .ok_or_else(|| provider_failed("decode_logits", "missing logits output"))?;
        let (shape, data) = logits
            .try_extract_tensor::<f32>()
            .map_err(|err| provider_failed("view_logits", err))?;
        decode_entities(
            shape,
            data,
            &input.text_words,
            &input.span_pairs,
            self.config.threshold,
        )
    }

    /// Run a small synthetic inference through the loaded ONNX graph.
    ///
    /// # Errors
    ///
    /// Returns a bookkeeping error if the model cannot execute.
    pub async fn smoke_infer(&self) -> BookkeepingResult<()> {
        let _entities = self
            .extract_entities("Alice uses Aletheia in Paris.")
            .await?;
        Ok(())
    }

    pub(crate) async fn extract_messages(
        &self,
        messages: &[ConversationMessage],
        schema: &ExtractionSchema,
    ) -> BookkeepingResult<Extraction> {
        let text = join_messages(messages);
        let gliner_entities = match self.extract_entities(&text).await {
            Ok(entities) => entities,
            Err(err) => {
                warn!(error = %err, "GLiNER extraction failed; falling back to LLM");
                return self.fallback.extract_knowledge(messages, schema).await;
            }
        };

        let mut extraction = self.fallback.extract_knowledge(messages, schema).await?;
        merge_entities(&mut extraction.entities, gliner_entities);
        Ok(extraction)
    }

    pub(crate) async fn extract_messages_with_turn_type(
        &self,
        messages: &[ConversationMessage],
        turn_type: crate::extract::refinement::TurnType,
        _schema: &ExtractionSchema,
    ) -> BookkeepingResult<Extraction> {
        let text = join_messages(messages);
        let gliner_entities = match self.extract_entities(&text).await {
            Ok(entities) => entities,
            Err(err) => {
                warn!(error = %err, "GLiNER extraction failed; falling back to LLM");
                return self
                    .fallback
                    .extract_messages_with_turn_type(messages, turn_type)
                    .await
                    .map_err(|err| provider_failed("fallback_extract_knowledge", err));
            }
        };

        let mut extraction = self
            .fallback
            .extract_messages_with_turn_type(messages, turn_type)
            .await
            .map_err(|err| provider_failed("fallback_extract_knowledge", err))?;
        merge_entities(&mut extraction.entities, gliner_entities);
        Ok(extraction)
    }
}

impl BookkeepingProvider for GlinerExtractionProvider<'_> {
    fn extract_knowledge<'b>(
        &'b self,
        messages: &'b [ConversationMessage],
        schema: &'b ExtractionSchema,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = BookkeepingResult<Extraction>> + Send + 'b>,
    > {
        Box::pin(async move { self.extract_messages(messages, schema).await })
    }

    fn extract_facts<'b>(
        &'b self,
        text: &'b str,
        schema: &'b ExtractionSchema,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = BookkeepingResult<Vec<ExtractedFact>>> + Send + 'b>,
    > {
        Box::pin(async move {
            let messages = [ConversationMessage::user(text)];
            let extraction = self.extract_knowledge(&messages, schema).await?;
            Ok(extraction.facts)
        })
    }

    fn name(&self) -> &'static str {
        "gliner"
    }
}

struct ModelInput {
    input_ids: Vec<i64>,
    attention_mask: Vec<i64>,
    words_mask: Vec<i64>,
    text_lengths: Vec<i64>,
    span_idx: Vec<i64>,
    span_mask: Vec<bool>,
    span_pairs: Vec<(usize, usize)>,
    text_words: Vec<String>,
}

fn load_tokenizer(path: &Path) -> BookkeepingResult<Tokenizer> {
    Tokenizer::from_file(path).map_err(|err| provider_failed("load_tokenizer", err))
}

fn build_input(tokenizer: &Tokenizer, words: &[String]) -> BookkeepingResult<ModelInput> {
    let mut prompt_words = Vec::with_capacity(ENTITY_LABELS.len() * 2 + 1 + words.len());
    for label in ENTITY_LABELS {
        prompt_words.push("<<ENT>>".to_owned());
        prompt_words.push(label.to_owned());
    }
    prompt_words.push("<<SEP>>".to_owned());
    let prompt_len = prompt_words.len();
    prompt_words.extend(words.iter().cloned());

    let encoding = tokenizer
        .encode(prompt_words, true)
        .map_err(|err| provider_failed("tokenize", err))?;
    let input_ids = encoding
        .get_ids()
        .iter()
        .map(|id| i64::from(*id))
        .collect::<Vec<_>>();
    let attention_mask = encoding
        .get_attention_mask()
        .iter()
        .map(|id| i64::from(*id))
        .collect::<Vec<_>>();
    let words_mask = build_words_mask(encoding.get_word_ids(), prompt_len)?;
    let text_word_count = words_mask
        .iter()
        .filter_map(|word| usize::try_from(*word).ok())
        .max()
        .unwrap_or(0)
        .min(MAX_TEXT_WORDS);
    let text_words = words
        .iter()
        .take(text_word_count)
        .cloned()
        .collect::<Vec<_>>();
    let span_pairs = build_span_pairs(text_words.len());
    let mut span_idx = Vec::with_capacity(span_pairs.len() * 2);
    for (start, end) in &span_pairs {
        span_idx.push(usize_to_i64(*start)?);
        span_idx.push(usize_to_i64(*end)?);
    }

    Ok(ModelInput {
        input_ids,
        attention_mask,
        words_mask,
        text_lengths: vec![usize_to_i64(text_words.len())?],
        span_mask: span_pairs
            .iter()
            .map(|(_, end)| *end < text_word_count)
            .collect(),
        span_pairs,
        text_words,
        span_idx,
    })
}

fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            current.push(ch);
        } else {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            if !ch.is_whitespace() {
                words.push(ch.to_string());
            }
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn build_words_mask(word_ids: &[Option<u32>], prompt_len: usize) -> BookkeepingResult<Vec<i64>> {
    let mut mask = Vec::with_capacity(word_ids.len());
    let mut previous = None;
    let mut seen_words = 0usize;
    for word_id in word_ids {
        match word_id {
            Some(id) if Some(*id) != previous => {
                seen_words += 1;
                if seen_words <= prompt_len {
                    mask.push(0);
                } else {
                    mask.push(usize_to_i64(seen_words - prompt_len)?);
                }
            }
            None | Some(_) => mask.push(0),
        }
        previous = *word_id;
    }
    Ok(mask)
}

fn build_span_pairs(word_count: usize) -> Vec<(usize, usize)> {
    let mut pairs = Vec::with_capacity(word_count.saturating_mul(MAX_SPAN_WIDTH));
    for start in 0..word_count {
        for width in 0..MAX_SPAN_WIDTH {
            pairs.push((start, start + width));
        }
    }
    pairs
}

fn decode_entities(
    shape: &Shape,
    data: &[f32],
    words: &[String],
    span_pairs: &[(usize, usize)],
    threshold: f32,
) -> BookkeepingResult<Vec<ExtractedEntity>> {
    match shape.len() {
        4 => decode_dense_logits(shape, data, words, threshold),
        3 => decode_span_logits(shape, data, words, span_pairs, threshold),
        ndim => Err(provider_failed(
            "decode_logits",
            format!("unsupported logits rank {ndim}"),
        )),
    }
}

fn decode_dense_logits(
    shape: &Shape,
    data: &[f32],
    words: &[String],
    threshold: f32,
) -> BookkeepingResult<Vec<ExtractedEntity>> {
    let starts = shape_usize(
        *shape
            .get(1)
            .ok_or_else(|| provider_failed("decode_logits", "missing start dimension"))?,
    )?;
    let widths = shape_usize(
        *shape
            .get(2)
            .ok_or_else(|| provider_failed("decode_logits", "missing width dimension"))?,
    )?;
    let classes = shape_usize(
        *shape
            .get(3)
            .ok_or_else(|| provider_failed("decode_logits", "missing class dimension"))?,
    )?;
    let stride_start = widths
        .checked_mul(classes)
        .ok_or_else(|| provider_failed("decode_logits", "logit stride overflow"))?;
    let batch_stride = starts
        .checked_mul(stride_start)
        .ok_or_else(|| provider_failed("decode_logits", "logit batch stride overflow"))?;
    if data.len() < batch_stride {
        return Err(provider_failed(
            "decode_logits",
            "logits data shorter than shape",
        ));
    }
    let mut entities = Vec::new();

    for start in 0..starts.min(words.len()) {
        for width in 0..widths {
            let end = start + width;
            if end >= words.len() {
                continue;
            }
            for (class_idx, label) in ENTITY_LABELS
                .iter()
                .enumerate()
                .take(classes.min(ENTITY_LABELS.len()))
            {
                let Some(score) = dense_logit(data, start, width, class_idx, widths, classes)
                else {
                    continue;
                };
                let probability = sigmoid(score);
                if probability >= threshold {
                    let Some(span_words) = words.get(start..=end) else {
                        continue;
                    };
                    entities.push(GlinerEntity {
                        name: span_words.join(" "),
                        entity_type: (*label).to_owned(),
                        score: probability,
                    });
                }
            }
        }
    }

    Ok(to_extracted_entities(entities))
}

fn decode_span_logits(
    shape: &Shape,
    data: &[f32],
    words: &[String],
    span_pairs: &[(usize, usize)],
    threshold: f32,
) -> BookkeepingResult<Vec<ExtractedEntity>> {
    let spans = shape_usize(
        *shape
            .get(1)
            .ok_or_else(|| provider_failed("decode_logits", "missing span dimension"))?,
    )?;
    let classes = shape_usize(
        *shape
            .get(2)
            .ok_or_else(|| provider_failed("decode_logits", "missing class dimension"))?,
    )?;
    let batch_stride = spans
        .checked_mul(classes)
        .ok_or_else(|| provider_failed("decode_logits", "span logit stride overflow"))?;
    if data.len() < batch_stride {
        return Err(provider_failed(
            "decode_logits",
            "logits data shorter than shape",
        ));
    }
    let mut entities = Vec::new();

    for span_idx in 0..spans.min(span_pairs.len()) {
        let Some((start, end)) = span_pairs.get(span_idx).copied() else {
            continue;
        };
        if end >= words.len() {
            continue;
        }
        for (class_idx, label) in ENTITY_LABELS
            .iter()
            .enumerate()
            .take(classes.min(ENTITY_LABELS.len()))
        {
            let Some(score) = span_logit(data, span_idx, class_idx, classes) else {
                continue;
            };
            let probability = sigmoid(score);
            if probability >= threshold {
                let Some(span_words) = words.get(start..=end) else {
                    continue;
                };
                entities.push(GlinerEntity {
                    name: span_words.join(" "),
                    entity_type: (*label).to_owned(),
                    score: probability,
                });
            }
        }
    }

    Ok(to_extracted_entities(entities))
}

fn dense_logit(
    data: &[f32],
    start: usize,
    width: usize,
    class_idx: usize,
    widths: usize,
    classes: usize,
) -> Option<f32> {
    let offset = start.checked_mul(widths.checked_mul(classes)?)?;
    let offset = offset.checked_add(width.checked_mul(classes)?)?;
    let offset = offset.checked_add(class_idx)?;
    data.get(offset).copied()
}

fn span_logit(data: &[f32], span_idx: usize, class_idx: usize, classes: usize) -> Option<f32> {
    let offset = span_idx.checked_mul(classes)?.checked_add(class_idx)?;
    data.get(offset).copied()
}

fn to_extracted_entities(mut candidates: Vec<GlinerEntity>) -> Vec<ExtractedEntity> {
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut entities = Vec::new();
    for candidate in candidates {
        if entities.iter().any(|entity: &ExtractedEntity| {
            entity.name.eq_ignore_ascii_case(&candidate.name)
                && entity.entity_type == candidate.entity_type
        }) {
            continue;
        }
        entities.push(ExtractedEntity {
            description: format!("GLiNER entity span with score {:.3}", candidate.score),
            name: candidate.name,
            entity_type: candidate.entity_type,
        });
    }
    entities
}

fn merge_entities(existing: &mut Vec<ExtractedEntity>, incoming: Vec<ExtractedEntity>) {
    for entity in incoming {
        if existing.iter().any(|known| {
            known.name.eq_ignore_ascii_case(&entity.name) && known.entity_type == entity.entity_type
        }) {
            continue;
        }
        existing.push(entity);
    }
}

fn join_messages(messages: &[ConversationMessage]) -> String {
    messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn usize_to_i64(value: usize) -> BookkeepingResult<i64> {
    i64::try_from(value).map_err(|err| provider_failed("integer_conversion", err))
}

fn shape_usize(value: i64) -> BookkeepingResult<usize> {
    usize::try_from(value).map_err(|err| provider_failed("shape_conversion", err))
}

fn provider_failed(operation: &'static str, message: impl std::fmt::Display) -> BookkeepingError {
    ProviderFailedSnafu {
        provider: "gliner",
        operation,
        message: message.to_string(),
    }
    .build()
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]

    use super::*;

    struct NeverFallback;

    impl ExtractionProvider for NeverFallback {
        fn complete<'a>(
            &'a self,
            _system: &'a str,
            _user_message: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, crate::extract::ExtractionError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async move { unreachable!("smoke test should not call LLM fallback") })
        }
    }

    #[test]
    fn split_words_keeps_punctuation_tokens() {
        let words = split_words("Alice uses Aletheia.");
        assert_eq!(words, ["Alice", "uses", "Aletheia", "."]);
    }

    #[tokio::test]
    async fn gliner_model_smoke_inference_when_staged_model_exists() {
        let model_dir = Path::new(DEFAULT_MODEL_DIR);
        if !model_dir.exists() {
            return;
        }

        let engine = ExtractionEngine::new(crate::extract::ExtractionConfig::default());
        let provider = GlinerExtractionProvider::new(&engine, &NeverFallback)
            .expect("staged GLiNER model should load");
        provider
            .smoke_infer()
            .await
            .expect("staged GLiNER model should run synthetic inference");
    }
}
