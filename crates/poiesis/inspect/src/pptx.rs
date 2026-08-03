//! PPTX presentation text extraction implementation.

use std::io::Cursor;

use poiesis_ooxml_parse::extract_text_from_slide;
use zip::ZipArchive;

use crate::PresentationSummary;
use crate::error::Result;

/// Parse the `N` out of a `ppt/slides/slideN.xml` part name.
///
/// Returns `None` for any entry that is not a numbered slide part, which is
/// what filters `slideLayout`/`slideMaster` and `_rels` entries out of the
/// enumeration.
fn slide_number(name: &str) -> Option<u32> {
    let stem = name
        .strip_prefix("ppt/slides/slide")?
        .strip_suffix(".xml")?;
    stem.parse().ok()
}

pub(crate) fn inspect_pptx_impl(bytes: &[u8]) -> Result<PresentationSummary> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| crate::InspectError::ZipError { source: e })?;

    // WHY: slide part names are not guaranteed contiguous — deleting a slide in
    // PowerPoint leaves the remaining part names unrenumbered, so a deck can hold
    // slide1, slide2 and slide4. Probing slide{n} upward and stopping at the first
    // missing index silently dropped every slide past the gap. Enumerating the
    // archive and sorting by slide number reads the whole deck, and matches how
    // `poiesis_slides::inspect_pptx` already enumerates the same parts.
    let mut numbered: Vec<(u32, String)> = (0..archive.len())
        .map(|i| {
            archive
                .by_index(i)
                .map(|f| f.name().to_owned())
                .map_err(|e| crate::InspectError::ZipError { source: e })
        })
        .collect::<Result<Vec<String>>>()?
        .into_iter()
        .filter_map(|name| slide_number(&name).map(|n| (n, name)))
        .collect();
    numbered.sort_by_key(|(n, _)| *n);

    let mut slides: Vec<String> = Vec::with_capacity(numbered.len());
    for (_, name) in &numbered {
        let mut file = archive
            .by_name(name)
            .map_err(|e| crate::InspectError::ZipError { source: e })?;
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| crate::InspectError::Io { source: e })?;
        slides.push(extract_text_from_slide(&content));
    }

    Ok(PresentationSummary { slides })
}
