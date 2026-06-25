//! PPTX presentation diffing implementation.

use std::collections::BTreeMap;
use std::io::Cursor;

use poiesis_ooxml_parse::extract_text_from_slide;
use zip::ZipArchive;

use crate::SlideDiff;
use crate::error::Result;

/// Read slide contents from PPTX archive.
fn read_presentation(bytes: &[u8]) -> Result<BTreeMap<usize, String>> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| crate::DiffError::ZipError { source: e })?;

    let mut slides: BTreeMap<usize, String> = BTreeMap::new();

    let mut slide_idx = 1;
    loop {
        let slide_path = format!("ppt/slides/slide{slide_idx}.xml");
        match archive.by_name(&slide_path) {
            Ok(mut file) => {
                let mut content = String::new();
                std::io::Read::read_to_string(&mut file, &mut content)
                    .map_err(|e| crate::DiffError::Io { source: e })?;
                let text = extract_text_from_slide(&content);
                slides.insert(slide_idx - 1, text);
                slide_idx += 1;
            }
            Err(zip::result::ZipError::FileNotFound) => {
                break;
            }
            Err(e) => {
                return Err(crate::DiffError::ZipError { source: e });
            }
        }
    }

    Ok(slides)
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
