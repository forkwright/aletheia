//! Bounded PDF parsing and text extraction.
//!
//! `lopdf` is the sole PDF parser in this crate. In particular, this module
//! must not grow a second parser through `pdf-extract`: mixed parser versions
//! can disagree about malformed documents and make a successful page count
//! meaningless next to a failed extraction.

use std::cmp::min;
use std::panic::{AssertUnwindSafe, catch_unwind};

use lopdf::{DecompressionBudget, Document, LoadOptions};

use crate::error::Result;
use crate::{InspectError, PdfSummary};

/// Maximum input accepted by the backwards-compatible convenience functions.
///
/// Deployed callers pass their own authoritative boundary limit instead: the
/// CLI uses the workspace PDF-file limit and the tool uses its resolved
/// `ToolLimitsConfig::max_pdf_bytes` value.
const DEFAULT_MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_PAGES: usize = 256;
const DEFAULT_MAX_OBJECTS: usize = 16_384;
const DEFAULT_MAX_DECOMPRESSED_STREAM_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_DECOMPRESSED_PAGE_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_DECOMPRESSED_TOTAL_BYTES: usize = DEFAULT_MAX_INPUT_BYTES;
const DEFAULT_MAX_EXTRACTED_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// Resource policy for one PDF inspection operation.
///
/// This is the only owner of the PDF parser and extraction budgets. A caller
/// may lower any field for a tighter operation, but must never increase a
/// boundary-derived input budget after it has accepted bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfInspectLimits {
    /// Maximum encoded PDF input bytes.
    pub max_input_bytes: usize,
    /// Maximum pages that may be extracted.
    pub max_pages: usize,
    /// Maximum indirect objects retained by the parser.
    pub max_objects: usize,
    /// Maximum bytes one stream may decompress to while loading or extracting.
    pub max_decompressed_stream_bytes: usize,
    /// Maximum decoded content accepted for one page.
    pub max_decompressed_page_bytes: usize,
    /// Aggregate decoded stream/content budget for this operation.
    pub max_decompressed_total_bytes: usize,
    /// Aggregate UTF-8 text returned by extraction.
    pub max_extracted_text_bytes: usize,
}

impl PdfInspectLimits {
    /// Derive an inspection policy from a boundary that has already limited the
    /// encoded PDF bytes.
    #[must_use]
    pub fn for_input_bytes(max_input_bytes: usize) -> Self {
        Self {
            max_input_bytes,
            max_pages: DEFAULT_MAX_PAGES,
            max_objects: DEFAULT_MAX_OBJECTS,
            max_decompressed_stream_bytes: min(
                DEFAULT_MAX_DECOMPRESSED_STREAM_BYTES,
                max_input_bytes,
            ),
            max_decompressed_page_bytes: min(DEFAULT_MAX_DECOMPRESSED_PAGE_BYTES, max_input_bytes),
            max_decompressed_total_bytes: min(
                DEFAULT_MAX_DECOMPRESSED_TOTAL_BYTES,
                max_input_bytes,
            ),
            max_extracted_text_bytes: min(DEFAULT_MAX_EXTRACTED_TEXT_BYTES, max_input_bytes),
        }
    }
}

impl Default for PdfInspectLimits {
    fn default() -> Self {
        Self::for_input_bytes(DEFAULT_MAX_INPUT_BYTES)
    }
}

/// Every line of text in a PDF, with no summary cap applied.
pub(crate) fn extract_pdf_text_impl(bytes: &[u8], limits: &PdfInspectLimits) -> Result<String> {
    let (document, pages, budget) = load_document(bytes, limits)?;
    extract_text(&document, &pages, limits, &budget)
}

pub(crate) fn inspect_pdf_impl(bytes: &[u8], limits: &PdfInspectLimits) -> Result<PdfSummary> {
    let (document, pages, budget) = load_document(bytes, limits)?;
    let text = extract_text(&document, &pages, limits, &budget)?;

    let lines: Vec<&str> = text
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .collect();
    let total_lines = lines.len();
    let truncated = total_lines > 100;
    let text_snippets = lines.into_iter().take(100).map(ToOwned::to_owned).collect();

    Ok(PdfSummary::new(
        pages.len().max(1),
        true,
        text_snippets,
        truncated,
        total_lines,
    ))
}

fn load_document(
    bytes: &[u8],
    limits: &PdfInspectLimits,
) -> Result<(Document, Vec<u32>, DecompressionBudget)> {
    if bytes.len() > limits.max_input_bytes {
        return Err(InspectError::PdfInputTooLarge);
    }
    if limits.max_decompressed_stream_bytes == 0
        || limits.max_decompressed_page_bytes == 0
        || limits.max_decompressed_total_bytes == 0
    {
        return Err(InspectError::PdfLimitExceeded {
            limit: "decompression budget",
        });
    }

    // SAFETY: malformed PDFs must be normal errors. lopdf 0.44 has its own
    // parser nesting guard; this is defense in depth for non-aborting panics in
    // dependency code and does not attempt to recover a stack-overflow abort.
    let budget = DecompressionBudget::new(limits.max_decompressed_total_bytes);
    let document = catch_unwind(AssertUnwindSafe(|| {
        Document::load_mem_with_options(
            bytes,
            LoadOptions {
                strict: true,
                max_decompressed_size: Some(limits.max_decompressed_stream_bytes),
                decompression_budget: Some(budget.clone()),
                ..LoadOptions::default()
            },
        )
    }))
    .map_err(|_panic_payload| InspectError::PdfExtractionError {
        detail: "PDF parser rejected the document".to_owned(),
    })?
    .map_err(|error| map_lopdf_error(&error))?;

    // Never probe an empty password. An encrypted document is an explicit,
    // stable unsupported case rather than a surprising best-effort decode.
    if document.was_encrypted() || document.is_encrypted() {
        return Err(InspectError::EncryptedPdf);
    }
    if document.objects.len() > limits.max_objects {
        return Err(InspectError::PdfLimitExceeded {
            limit: "object count",
        });
    }

    let pages: Vec<u32> = document.get_pages().into_keys().collect();
    if pages.len() > limits.max_pages {
        return Err(InspectError::PdfLimitExceeded {
            limit: "page count",
        });
    }
    Ok((document, pages, budget))
}

fn extract_text(
    document: &Document,
    pages: &[u32],
    limits: &PdfInspectLimits,
    budget: &DecompressionBudget,
) -> Result<String> {
    let mut text = String::new();

    for page_number in pages {
        let per_decode = min(
            min(
                limits.max_decompressed_stream_bytes,
                limits.max_decompressed_page_bytes,
            ),
            budget.remaining(),
        );
        if per_decode == 0 {
            return Err(InspectError::PdfLimitExceeded {
                limit: "aggregate decompression",
            });
        }

        // The workspace-patched lopdf reserves one `per_decode` allocation for
        // page content and each ToUnicode stream before decoding either of
        // them. The same budget was already charged by eager xref/object-stream
        // loading, so no document stage can evade the aggregate cap.
        let chunks = catch_unwind(AssertUnwindSafe(|| {
            document.extract_text_chunks_with_limit_and_budget(&[*page_number], per_decode, budget)
        }))
        .map_err(|_panic_payload| InspectError::PdfExtractionError {
            detail: "PDF text extraction rejected the document".to_owned(),
        })?;
        for chunk in chunks {
            let chunk = chunk.map_err(|error| map_lopdf_error(&error))?;
            let next_len =
                text.len()
                    .checked_add(chunk.len())
                    .ok_or(InspectError::PdfLimitExceeded {
                        limit: "extracted text",
                    })?;
            if next_len > limits.max_extracted_text_bytes {
                return Err(InspectError::PdfLimitExceeded {
                    limit: "extracted text",
                });
            }
            text.push_str(&chunk);
        }
    }
    Ok(text)
}

fn map_lopdf_error(error: &lopdf::Error) -> InspectError {
    match error {
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => {
            InspectError::PdfLimitExceeded {
                limit: "aggregate decompression",
            }
        }
        _ => InspectError::PdfExtractionError {
            detail: "PDF parser rejected the document".to_owned(),
        },
    }
}
