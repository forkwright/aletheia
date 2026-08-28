//! PPTX presentation diffing implementation.

use std::collections::BTreeMap;

use poiesis_ooxml_parse::{extract_text_from_slide, read_pptx_slides};

use crate::SlideDiff;
use crate::error::Result;

/// Read slide contents from PPTX archive, keyed by 0-based slide position.
fn read_presentation(bytes: &[u8]) -> Result<BTreeMap<usize, String>> {
    let slides = read_pptx_slides(bytes)?;
    Ok(slides
        .iter()
        .enumerate()
        .map(|(idx, xml)| (idx, extract_text_from_slide(xml)))
        .collect())
}

pub(crate) fn diff_presentations_impl(a: &[u8], b: &[u8]) -> Result<Vec<SlideDiff>> {
    let presentation_a = read_presentation(a)?;
    let presentation_b = read_presentation(b)?;

    let mut diffs = Vec::new();

    let max_slide = presentation_a.len().max(presentation_b.len());

    for slide_idx in 0..max_slide {
        let text_a = presentation_a.get(&slide_idx).cloned();
        let text_b = presentation_b.get(&slide_idx).cloned();

        if text_a != text_b {
            diffs.push(SlideDiff {
                slide_index: slide_idx,
                before: text_a,
                after: text_b,
            });
        }
    }

    Ok(diffs)
}
