//! PPTX presentation text extraction implementation.

use std::io::Cursor;

use zip::ZipArchive;

use crate::PresentationSummary;
use crate::error::Result;

/// Extract text content from slide XML using simple string matching.
fn extract_text_from_slide(xml_data: &str) -> Result<String> {
    let mut text_content = String::new();

    // Simple approach: find all <a:t>...</a:t> tags
    for chunk in xml_data.split("<a:t>") {
        if let Some(end) = chunk.find("</a:t>") {
            let text = &chunk[..end];
            if !text.is_empty() {
                text_content.push_str(text);
                text_content.push(' ');
            }
        }
    }

    Ok(text_content.trim().to_string())
}

pub(crate) fn inspect_pptx_impl(bytes: &[u8]) -> Result<PresentationSummary> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| crate::InspectError::ZipError { source: e })?;

    let mut slides: Vec<String> = Vec::new();

    // Read all slide files (ppt/slides/slide1.xml, ppt/slides/slide2.xml, etc.)
    let mut slide_idx = 1;
    loop {
        let slide_path = format!("ppt/slides/slide{}.xml", slide_idx);
        match archive.by_name(&slide_path) {
            Ok(mut file) => {
                let mut content = String::new();
                std::io::Read::read_to_string(&mut file, &mut content)
                    .map_err(|e| crate::InspectError::Io { source: e })?;
                let text = extract_text_from_slide(&content)?;
                slides.push(text);
                slide_idx += 1;
            }
            Err(zip::result::ZipError::FileNotFound) => {
                break;
            }
            Err(e) => {
                return Err(crate::InspectError::ZipError { source: e });
            }
        }
    }

    Ok(PresentationSummary { slides })
}
