//! PDF text extraction implementation.

use pdf_extract::extract_text_from_mem;

use crate::PdfSummary;
use crate::error::Result;

pub(crate) fn inspect_pdf_impl(bytes: &[u8]) -> Result<PdfSummary> {
    let text =
        extract_text_from_mem(bytes).map_err(|e| crate::InspectError::PdfExtractionError {
            detail: format!("{e:?}"),
        })?;

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
