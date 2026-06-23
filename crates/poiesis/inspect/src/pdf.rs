//! PDF text extraction implementation.

use pdf_extract::extract_text_from_mem;

use crate::PdfSummary;
use crate::error::Result;

pub(crate) fn inspect_pdf_impl(bytes: &[u8]) -> Result<PdfSummary> {
    let text =
        extract_text_from_mem(bytes).map_err(|e| crate::InspectError::PdfExtractionError {
            detail: format!("{e:?}"),
        })?;

    let text_snippets: Vec<String> = text
        .split('\n')
        .filter(|line| !line.trim().is_empty())
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

    Ok(PdfSummary {
        pages,
        page_count_reliable,
        text_snippets,
    })
}
