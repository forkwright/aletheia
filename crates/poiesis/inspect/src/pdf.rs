//! Bounded PDF parsing and text extraction.
//!
//! `lopdf` is the sole PDF parser in this crate. In particular, this module
//! must not grow a second parser through `pdf-extract`: mixed parser versions
//! can disagree about malformed documents and make a successful page count
//! meaningless next to a failed extraction.

use std::cmp::min;
use std::io::Read;
use std::path::Path;

use lopdf::{
    DecompressionBudget, Document, LoadOptions, RetainedBytesBudget, SourceWorkBudget,
    ToUnicodeMappingBudget,
};

use crate::error::Result;
use crate::{InspectError, PdfSummary};

/// Maximum input accepted by the backwards-compatible convenience functions.
///
/// Deployed callers pass their own authoritative boundary limit instead: the
/// CLI uses the workspace PDF-file limit and the tool uses its resolved
/// `ToolLimitsConfig::max_pdf_bytes` value.
const DEFAULT_MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
// 128 pages × one 256 KiB content decoder exactly fits the 32 MiB shared
// aggregate budget. Fonts and extra filter layers consume additional budget,
// so real hostile inputs fail earlier rather than violating the aggregate cap.
const DEFAULT_MAX_PAGES: usize = 128;
const DEFAULT_MAX_OBJECTS: usize = 16_384;
const DEFAULT_MAX_DECOMPRESSED_STREAM_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_DECOMPRESSED_PAGE_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_DECOMPRESSED_TOTAL_BYTES: usize = DEFAULT_MAX_INPUT_BYTES;
// Cumulative document-owned allocation ceiling, separate from accepted input
// and from source-work intervals. It is an explicit operation cap rather than
// an input multiplier: repeated reparses consume it generation by generation.
const DEFAULT_MAX_RETAINED_ALLOCATION_BYTES: usize = 128 * 1024 * 1024;
const DEFAULT_MAX_EXTRACTED_TEXT_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_TOUNICODE_MAPPINGS: usize = 65_536;
// lopdf's bounded ToUnicode admission allows at most four UTF-16 units per
// source byte. UTF-8 needs at most three bytes per unit, and content operators
// can add a separator per source byte; reserve a little extra for separators.
const MAX_TEXT_BYTES_PER_DECODED_CONTENT_BYTE: usize = 16;

/// Open and read an input PDF through one filesystem handle, refusing a
/// concurrent append before the result vector grows beyond `max_input_bytes`.
/// Once opened, a later path replacement cannot swap a different file into the
/// parse operation.
pub fn read_pdf_file_bounded(path: &Path, max_input_bytes: usize) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path).map_err(|source| InspectError::Io { source })?;
    let metadata = file
        .metadata()
        .map_err(|source| InspectError::Io { source })?;
    if !metadata.is_file() || metadata.len() > u64::try_from(max_input_bytes).unwrap_or(u64::MAX) {
        return Err(InspectError::PdfInputTooLarge);
    }

    let mut bytes = Vec::with_capacity(min(max_input_bytes, 64 * 1024));
    let mut chunk = [0_u8; 8192];
    loop {
        let count = file
            .read(&mut chunk)
            .map_err(|source| InspectError::Io { source })?;
        if count == 0 {
            return Ok(bytes);
        }
        let next = bytes
            .len()
            .checked_add(count)
            .ok_or(InspectError::PdfInputTooLarge)?;
        if next > max_input_bytes {
            return Err(InspectError::PdfInputTooLarge);
        }
        // INVARIANT: `count` is `Read::read`'s return value against `chunk`,
        // which never exceeds the buffer it filled -- `.get` still avoids a
        // direct-index panic if that contract is ever violated upstream.
        let Some(filled) = chunk.get(..count) else {
            return Err(InspectError::Io {
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "read reported more bytes than the buffer holds",
                ),
            });
        };
        bytes.extend_from_slice(filled);
    }
}

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
    /// Aggregate bytes copied into loader-owned stream buffers, object-stream
    /// members, encrypted staging, and explicit reparse/clone generations.
    pub max_retained_stream_bytes: usize,
    /// Maximum distinct input bytes inspected by parser structural work.
    pub max_source_work_bytes: usize,
    /// Aggregate UTF-8 text returned by extraction.
    pub max_extracted_text_bytes: usize,
    /// Aggregate source-code mappings expanded across every `/ToUnicode` CMap.
    pub max_tounicode_mappings: usize,
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
            max_retained_stream_bytes: DEFAULT_MAX_RETAINED_ALLOCATION_BYTES,
            // Source work admits interval union, not every reparse; valid
            // incremental/xref chains therefore fit within the accepted input.
            max_source_work_bytes: max_input_bytes,
            max_extracted_text_bytes: min(DEFAULT_MAX_EXTRACTED_TEXT_BYTES, max_input_bytes),
            max_tounicode_mappings: DEFAULT_MAX_TOUNICODE_MAPPINGS,
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
    if limits.max_tounicode_mappings == 0 {
        return Err(InspectError::PdfLimitExceeded {
            limit: "ToUnicode mappings",
        });
    }
    if limits.max_decompressed_stream_bytes == 0
        || limits.max_decompressed_page_bytes == 0
        || limits.max_decompressed_total_bytes == 0
    {
        return Err(InspectError::PdfLimitExceeded {
            limit: "decompression budget",
        });
    }

    let budget = DecompressionBudget::new(limits.max_decompressed_total_bytes);
    if limits.max_retained_stream_bytes == 0 {
        return Err(InspectError::PdfLimitExceeded {
            limit: "retained stream bytes",
        });
    }
    if limits.max_source_work_bytes == 0 {
        return Err(InspectError::PdfLimitExceeded {
            limit: "PDF source work",
        });
    }
    let retained_bytes_budget = RetainedBytesBudget::new(limits.max_retained_stream_bytes);
    let source_work_budget = SourceWorkBudget::new(limits.max_source_work_bytes);
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            strict: true,
            max_decompressed_size: Some(limits.max_decompressed_stream_bytes),
            decompression_budget: Some(budget.clone()),
            retained_bytes_budget: Some(retained_bytes_budget),
            source_work_budget: Some(source_work_budget),
            max_objects: Some(limits.max_objects),
            // This check happens once the trailer is parsed, before lopdf's
            // empty-password authentication path or any decryption work.
            reject_encrypted: true,
            ..LoadOptions::default()
        },
    )
    .map_err(|error| map_lopdf_error(&error))?;

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
    let mapping_budget = ToUnicodeMappingBudget::new(limits.max_tounicode_mappings);

    for page_number in pages {
        let remaining_text = limits
            .max_extracted_text_bytes
            .checked_sub(text.len())
            .ok_or(InspectError::PdfLimitExceeded {
                limit: "extracted text",
            })?;
        // Limit page-content decode by remaining output *before* lopdf creates
        // its per-page Strings or UTF-16 intermediates. The conservative
        // expansion factor is coupled to the ToUnicode target admission in the
        // fork, so this is a pre-allocation cumulative text boundary.
        let text_decode_cap = remaining_text / MAX_TEXT_BYTES_PER_DECODED_CONTENT_BYTE;
        if text_decode_cap == 0 {
            return Err(InspectError::PdfLimitExceeded {
                limit: "extracted text",
            });
        }
        let per_decode = min(
            min(
                limits.max_decompressed_stream_bytes,
                limits.max_decompressed_page_bytes,
            ),
            min(budget.remaining(), text_decode_cap),
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
        let chunks = document.extract_text_chunks_with_limit_and_budget(
            &[*page_number],
            per_decode,
            budget,
            &mapping_budget,
        );
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
        lopdf::Error::EncryptedDocument => InspectError::EncryptedPdf,
        lopdf::Error::ObjectLimitExceeded { .. } => InspectError::PdfLimitExceeded {
            limit: "object count",
        },
        lopdf::Error::RetainedBytesLimitExceeded { .. } => InspectError::PdfLimitExceeded {
            limit: "retained stream bytes",
        },
        lopdf::Error::SourceWorkLimitExceeded { .. } => InspectError::PdfLimitExceeded {
            limit: "PDF source work",
        },
        lopdf::Error::OverlappingObjectSpan => InspectError::PdfOverlappingObjectSpans,
        lopdf::Error::ToUnicodeCMap(_) => InspectError::PdfLimitExceeded {
            limit: "ToUnicode mappings",
        },
        _ => InspectError::PdfExtractionError {
            detail: "PDF parser rejected the document".to_owned(),
        },
    }
}
