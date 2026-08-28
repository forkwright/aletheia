//! PPTX presentation text extraction implementation.

use poiesis_ooxml_parse::{extract_text_from_slide, read_pptx_slides};

use crate::PresentationSummary;
use crate::error::Result;

pub(crate) fn inspect_pptx_impl(bytes: &[u8]) -> Result<PresentationSummary> {
    let slides = read_pptx_slides(bytes)?;
    Ok(PresentationSummary {
        slides: slides
            .iter()
            .map(|xml| extract_text_from_slide(xml))
            .collect(),
    })
}
