//! PDF text extraction implementation.

use pdf_extract::extract_text_from_mem;

use crate::PdfSummary;
use crate::error::Result;

pub(crate) fn inspect_pdf_impl(bytes: &[u8]) -> Result<PdfSummary> {
    // Use pdf-extract's memory-based API
    let text =
        extract_text_from_mem(bytes).map_err(|e| crate::InspectError::PdfExtractionError {
            detail: format!("{e:?}"),
        })?;

    // Split text by newlines to get snippets per page/section
    let text_snippets: Vec<String> = text
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .take(100) // Limit to first 100 snippets
        .map(std::string::ToString::to_string)
        .collect();

    // Real page count via lopdf
    let pages = lopdf::Document::load_mem(bytes)
        .map_or(1, |doc| doc.get_pages().len())
        .max(1);

    Ok(PdfSummary {
        pages,
        text_snippets,
    })
}
