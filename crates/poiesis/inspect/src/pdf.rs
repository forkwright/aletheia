//! PDF text extraction implementation.

use pdf_extract::extract_text_from_mem;

use crate::PdfSummary;
use crate::error::Result;

/// Every line of text in a PDF, with no summary cap applied.
///
/// WHY(#6751) this is separate from [`inspect_pdf_impl`]: that function exists to
/// SUMMARISE a document for an operator or an agent glancing at it, and caps its output
/// at 100 non-empty lines. That cap is right for a summary and wrong for anything that
/// consumes the document's content -- ingesting a capped extraction would put the first
/// hundred lines of a PDF into the knowledge graph and record it as the whole document,
/// with `truncated` sitting unread on a struct the caller discarded.
///
/// # Errors
///
/// Returns [`crate::InspectError::PdfExtractionError`] when the PDF cannot be decoded.
pub(crate) fn extract_pdf_text_impl(bytes: &[u8]) -> Result<String> {
    extract_text_from_mem(bytes).map_err(|e| crate::InspectError::PdfExtractionError {
        detail: format!("{e:?}"),
    })
}

pub(crate) fn inspect_pdf_impl(bytes: &[u8]) -> Result<PdfSummary> {
    let text = extract_pdf_text_impl(bytes)?;

    let lines: Vec<&str> = text
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .collect();
    let total_lines = lines.len();
    let truncated = total_lines > 100;
    let text_snippets: Vec<String> = lines
        .into_iter()
        .take(100)
        .map(std::string::ToString::to_string)
        .collect();

    let (pages, page_count_reliable) = match lopdf::Document::load_mem(bytes) {
        Ok(doc) => (doc.get_pages().len().max(1), true),
        Err(e) => {
            tracing::warn!(error = %e, "lopdf page-count failed; reporting 1");
            (1, false)
        }
    };

    Ok(PdfSummary::new(
        pages,
        page_count_reliable,
        text_snippets,
        truncated,
        total_lines,
    ))
}
