//! PDF text extraction implementation.

use pdf_extract::extract_text_from_mem;

use crate::PdfSummary;
use crate::error::Result;

pub(crate) fn inspect_pdf_impl(bytes: &[u8]) -> Result<PdfSummary> {
    // Use pdf-extract's memory-based API
    let text =
        extract_text_from_mem(bytes).map_err(|e| crate::InspectError::PdfExtractionError {
            detail: format!("{:?}", e),
        })?;

    // Split text by newlines to get snippets per page/section
    let text_snippets: Vec<String> = text
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .take(100) // Limit to first 100 snippets
        .map(|s| s.to_string())
        .collect();

    // Estimate page count (rough heuristic: split by form feed or every ~2000 chars)
    let pages = (bytes.len() / 2000).max(1).min(text_snippets.len());

    Ok(PdfSummary {
        pages,
        text_snippets,
    })
}
