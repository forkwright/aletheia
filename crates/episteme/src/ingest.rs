//! Data source ingestion pipeline: file → chunk → fact extraction.

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::id::FactId;
use crate::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    far_future,
};

/// Supported ingestion formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum IngestFormat {
    /// Markdown with optional YAML frontmatter.
    Markdown,
    /// Plain text.
    PlainText,
    /// JSON array of facts or single fact object.
    Json,
    /// JSON Lines — one fact per line.
    Jsonl,
}

/// Parse a format string into [`IngestFormat`].
///
/// Accepted values (case-insensitive): `markdown`, `md`, `text`, `plain_text`,
/// `plaintext`, `json`, `jsonl`.
#[must_use]
pub fn parse_format(s: &str) -> Option<IngestFormat> {
    match s.to_ascii_lowercase().as_str() {
        "markdown" | "md" => Some(IngestFormat::Markdown),
        "text" | "plain_text" | "plaintext" | "plain text" => Some(IngestFormat::PlainText),
        "json" => Some(IngestFormat::Json),
        "jsonl" => Some(IngestFormat::Jsonl),
        _ => None,
    }
}

/// A content chunk ready for fact creation.
#[derive(Debug, Clone)]
pub struct IngestChunk {
    /// The chunk text.
    pub content: String,
    /// Optional source identifier (file name, URL, etc.).
    pub source_hint: Option<String>,
}

/// Ingestion pipeline configuration.
#[derive(Debug, Clone)]
pub struct IngestConfig {
    /// Maximum characters per chunk before splitting.
    pub max_chunk_size: usize,
    /// Overlap between consecutive chunks.
    pub chunk_overlap: usize,
    /// Default confidence for heuristic-extracted facts.
    pub default_confidence: f64,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            max_chunk_size: 2_000,
            chunk_overlap: 200,
            default_confidence: 0.7,
        }
    }
}

/// Ingest raw content and produce facts.
///
/// For [`IngestFormat::Json`] and [`IngestFormat::Jsonl`], facts are parsed
/// directly from the input. For [`IngestFormat::Markdown`] and
/// [`IngestFormat::PlainText`], content is chunked and each chunk becomes a
/// heuristic fact.
///
/// # Errors
///
/// Returns an error if JSON parsing fails or if a generated fact ID is
/// invalid.
pub fn ingest_content(
    content: &str,
    format: IngestFormat,
    config: &IngestConfig,
    nous_id: &str,
) -> crate::error::Result<Vec<Fact>> {
    match format {
        IngestFormat::Markdown => {
            let chunks = chunk_markdown(content, config);
            chunks
                .into_iter()
                .map(|c| chunk_to_fact(c, nous_id, config))
                .collect()
        }
        IngestFormat::PlainText => {
            let chunks = chunk_plaintext(content, config);
            chunks
                .into_iter()
                .map(|c| chunk_to_fact(c, nous_id, config))
                .collect()
        }
        IngestFormat::Json => parse_json_facts(content),
        IngestFormat::Jsonl => parse_jsonl_facts(content),
    }
}

fn chunk_markdown(content: &str, config: &IngestConfig) -> Vec<IngestChunk> {
    let body = strip_frontmatter(content);
    let sections = split_by_headers(body);
    let mut chunks = Vec::new();
    for section in sections {
        let trimmed = section.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.len() <= config.max_chunk_size {
            chunks.push(IngestChunk {
                content: trimmed.to_owned(),
                source_hint: None,
            });
        } else {
            chunks.extend(split_into_chunks(
                trimmed,
                config.max_chunk_size,
                config.chunk_overlap,
            ));
        }
    }
    chunks
}

fn chunk_plaintext(content: &str, config: &IngestConfig) -> Vec<IngestChunk> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.len() <= config.max_chunk_size {
        vec![IngestChunk {
            content: trimmed.to_owned(),
            source_hint: None,
        }]
    } else {
        split_into_chunks(trimmed, config.max_chunk_size, config.chunk_overlap)
    }
}

#[expect(
    clippy::string_slice,
    reason = "byte-based chunking on mostly-ASCII text; boundary safety handled by word-boundary search"
)]
fn split_into_chunks(text: &str, max_size: usize, overlap: usize) -> Vec<IngestChunk> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let text_len = text.len();

    while start < text_len {
        let end = (start + max_size).min(text_len);
        let chunk_end = if end < text_len {
            let slice = &text[start..end];
            if let Some(pos) = slice.rfind(|c: char| c.is_whitespace()) {
                start + pos
            } else {
                end
            }
        } else {
            end
        };

        let content = text[start..chunk_end].trim().to_owned();
        if !content.is_empty() {
            chunks.push(IngestChunk {
                content,
                source_hint: None,
            });
        }

        let next_start = if chunk_end > start + overlap {
            chunk_end - overlap
        } else {
            chunk_end
        };

        if next_start <= start {
            start = chunk_end + 1;
        } else {
            start = next_start;
        }
    }

    chunks
}

#[expect(
    clippy::string_slice,
    reason = "ASCII boundary arithmetic on prefix-stripped content"
)]
fn strip_frontmatter(content: &str) -> &str {
    if let Some(after_open) = content.strip_prefix("---")
        && let Some(end) = after_open.find("\n---")
    {
        return after_open[end + 4..].trim_start();
    }
    content
}

fn split_by_headers(content: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        if line.trim_start().starts_with("##") {
            if !current.trim().is_empty() {
                sections.push(current.trim().to_owned());
            }
            current = {
                let mut s = line.to_owned();
                s.push('\n');
                s
            };
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }

    if !current.trim().is_empty() {
        sections.push(current.trim().to_owned());
    }

    if sections.is_empty() && !content.trim().is_empty() {
        sections.push(content.trim().to_owned());
    }

    sections
}

fn parse_json_facts(content: &str) -> crate::error::Result<Vec<Fact>> {
    let trimmed = content.trim();
    if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).context(crate::error::StoredJsonSnafu)
    } else {
        let fact: Fact = serde_json::from_str(trimmed).context(crate::error::StoredJsonSnafu)?;
        Ok(vec![fact])
    }
}

fn parse_jsonl_facts(content: &str) -> crate::error::Result<Vec<Fact>> {
    let mut facts = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fact: Fact = serde_json::from_str(line).context(crate::error::StoredJsonSnafu)?;
        facts.push(fact);
    }
    Ok(facts)
}

fn chunk_to_fact(
    chunk: IngestChunk,
    nous_id: &str,
    config: &IngestConfig,
) -> crate::error::Result<Fact> {
    let now = jiff::Timestamp::now();
    let classified_type = crate::knowledge::FactType::classify(&chunk.content);
    let id = FactId::new(format!("fact-{}", koina::ulid::Ulid::new()))
        .context(crate::error::InvalidIdSnafu)?;

    Ok(Fact {
        id,
        nous_id: nous_id.to_owned(),
        content: chunk.content,
        fact_type: classified_type.as_str().to_owned(),
        scope: None,
        project_id: None,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: config.default_confidence,
            tier: EpistemicTier::Inferred,
            source_session_id: chunk.source_hint,
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
        sensitivity: FactSensitivity::Public,
        visibility: crate::knowledge::Visibility::Private,
    })
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions — panics are acceptable in test context"
)]
mod tests {
    use super::*;

    #[test]
    fn parse_format_recognizes_all_variants() {
        assert_eq!(parse_format("markdown"), Some(IngestFormat::Markdown));
        assert_eq!(parse_format("md"), Some(IngestFormat::Markdown));
        assert_eq!(parse_format("text"), Some(IngestFormat::PlainText));
        assert_eq!(parse_format("plain_text"), Some(IngestFormat::PlainText));
        assert_eq!(parse_format("json"), Some(IngestFormat::Json));
        assert_eq!(parse_format("jsonl"), Some(IngestFormat::Jsonl));
    }

    #[test]
    fn parse_format_is_case_insensitive() {
        assert_eq!(parse_format("Markdown"), Some(IngestFormat::Markdown));
        assert_eq!(parse_format("JSONL"), Some(IngestFormat::Jsonl));
    }

    #[test]
    fn parse_format_returns_none_for_unknown() {
        assert_eq!(parse_format("pdf"), None);
    }

    #[test]
    fn strip_frontmatter_removes_yaml_block() {
        let text = "---\ntitle: Test\n---\n\n# Heading\nbody";
        assert_eq!(strip_frontmatter(text), "# Heading\nbody");
    }

    #[test]
    fn strip_frontmatter_leaves_plain_text() {
        let text = "# Heading\nbody";
        assert_eq!(strip_frontmatter(text), text);
    }

    #[test]
    fn split_by_headers_produces_sections() {
        let text = "intro\n## Section A\ncontent a\n## Section B\ncontent b";
        let sections = split_by_headers(text);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0], "intro");
        assert!(sections[1].starts_with("## Section A"));
        assert!(sections[2].starts_with("## Section B"));
    }

    #[test]
    fn split_into_chunks_respects_max_size() {
        let text = "one two three four five six seven eight nine ten";
        let chunks = split_into_chunks(text, 20, 5);
        for chunk in &chunks {
            assert!(chunk.content.len() <= 20, "chunk exceeded max size");
        }
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_plaintext_returns_single_chunk_when_small() {
        let config = IngestConfig::default();
        let chunks = chunk_plaintext("small text", &config);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "small text");
    }

    #[test]
    fn chunk_to_fact_populates_all_fields() {
        let chunk = IngestChunk {
            content: "Alice knows Rust".to_owned(),
            source_hint: Some("test.md".to_owned()),
        };
        let config = IngestConfig::default();
        let fact = chunk_to_fact(chunk, "syn", &config).unwrap();

        assert_eq!(fact.nous_id, "syn");
        assert_eq!(fact.content, "Alice knows Rust");
        #[expect(clippy::float_cmp, reason = "test assertion on exact config value")]
        {
            assert_eq!(fact.provenance.confidence, 0.7);
        }
        assert_eq!(fact.provenance.tier, EpistemicTier::Inferred);
        assert_eq!(
            fact.provenance.source_session_id,
            Some("test.md".to_owned())
        );
        assert!(!fact.lifecycle.is_forgotten);
    }

    #[test]
    fn parse_json_facts_array() {
        let json = r#"[{"id":"fact-01","nous_id":"syn","fact_type":"observation","content":"test","valid_from":"2024-01-01T00:00:00Z","valid_to":"9999-01-01T00:00:00Z","recorded_at":"2024-01-01T00:00:00Z","confidence":0.7,"tier":"inferred","stability_hours":72.0,"access_count":0,"is_forgotten":false}]"#;
        let facts = parse_json_facts(json).unwrap();
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn parse_json_facts_single() {
        let json = r#"{"id":"fact-01","nous_id":"syn","fact_type":"observation","content":"test","valid_from":"2024-01-01T00:00:00Z","valid_to":"9999-01-01T00:00:00Z","recorded_at":"2024-01-01T00:00:00Z","confidence":0.7,"tier":"inferred","stability_hours":72.0,"access_count":0,"is_forgotten":false}"#;
        let facts = parse_json_facts(json).unwrap();
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn parse_jsonl_facts_skips_empty_lines() {
        let jsonl = "\n{\"id\":\"fact-01\",\"nous_id\":\"syn\",\"fact_type\":\"observation\",\"content\":\"a\",\"valid_from\":\"2024-01-01T00:00:00Z\",\"valid_to\":\"9999-01-01T00:00:00Z\",\"recorded_at\":\"2024-01-01T00:00:00Z\",\"confidence\":0.7,\"tier\":\"inferred\",\"stability_hours\":72.0,\"access_count\":0,\"is_forgotten\":false}\n\n{\"id\":\"fact-02\",\"nous_id\":\"syn\",\"fact_type\":\"observation\",\"content\":\"b\",\"valid_from\":\"2024-01-01T00:00:00Z\",\"valid_to\":\"9999-01-01T00:00:00Z\",\"recorded_at\":\"2024-01-01T00:00:00Z\",\"confidence\":0.7,\"tier\":\"inferred\",\"stability_hours\":72.0,\"access_count\":0,\"is_forgotten\":false}\n";
        let facts = parse_jsonl_facts(jsonl).unwrap();
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn ingest_markdown_chunks_by_header() {
        let md = "## A\ncontent a\n## B\ncontent b";
        let config = IngestConfig::default();
        let facts = ingest_content(md, IngestFormat::Markdown, &config, "syn").unwrap();
        assert_eq!(facts.len(), 2);
    }
}
