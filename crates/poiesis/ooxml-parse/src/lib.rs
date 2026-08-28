#![deny(missing_docs)]
//! Shared OOXML parsing primitives used by `poiesis-inspect` and `poiesis-diff`.
//!
//! These helpers perform minimal, dependency-light extraction of text and
//! workbook metadata from Office Open XML parts. They intentionally avoid
//! pulling in a full XML parser; callers that need structural validation
//! should use a dedicated OOXML library.
//!
//! [`read_pptx_slides`] and [`read_workbook_parts`] additionally own the ZIP
//! archive plumbing (part enumeration, sheet/rels resolution) that both
//! `poiesis-inspect` and `poiesis-diff` need before they can apply their own
//! per-part transform.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::escape::unescape;
use snafu::Snafu;
use zip::ZipArchive;

/// Error type for shared OOXML archive-reading operations.
#[derive(Debug, Snafu)]
pub enum ArchiveError {
    /// Failed to parse or read from the ZIP archive.
    #[snafu(display("failed to parse ZIP archive: {source}"))]
    Zip {
        /// Source error from the zip crate.
        source: zip::result::ZipError,
    },

    /// IO error while reading an archive part.
    #[snafu(display("IO error: {source}"))]
    Io {
        /// Source IO error.
        source: std::io::Error,
    },
}

/// Result alias for shared OOXML archive-reading operations.
pub type ArchiveResult<T> = std::result::Result<T, ArchiveError>;

fn push_xml_text(output: &mut String, raw: &str) {
    match unescape(raw) {
        Ok(decoded) => output.push_str(&decoded),
        Err(_) => output.push_str(raw),
    }
}

/// Extract shared strings from `xl/sharedStrings.xml`.
///
/// Splits the XML on `<si>` elements and concatenates all `<t>...</t>` text
/// fragments inside each shared-string item. This mirrors the compact XML
/// emitted by common XLSX writers.
pub fn extract_shared_strings(xml_data: &str) -> Vec<String> {
    let mut strings = Vec::new();
    for chunk in xml_data.split("<si>") {
        if let Some(end) = chunk.find("</si>")
            && let Some(si) = chunk.get(..end)
        {
            let mut text = String::new();
            for t_chunk in si.split("<t") {
                if let Some(gt) = t_chunk.find('>')
                    && let Some(after_gt) = t_chunk.get(gt + 1..)
                    && let Some(lt) = after_gt.find("</t>")
                    && let Some(slice) = after_gt.get(..lt)
                {
                    push_xml_text(&mut text, slice);
                }
            }
            strings.push(text);
        }
    }
    strings
}

/// Extract text content from a PPTX slide XML using simple string matching.
///
/// Concatenates the raw text content of all `<a:t>...</a:t>` elements and
/// returns a single trimmed string.
pub fn extract_text_from_slide(xml_data: &str) -> String {
    let mut text_content = String::new();

    for chunk in xml_data.split("<a:t>") {
        if let Some(end) = chunk.find("</a:t>")
            && let Some(text) = chunk.get(..end)
            && !text.is_empty()
        {
            push_xml_text(&mut text_content, text);
            text_content.push(' ');
        }
    }

    text_content.trim().to_string()
}

/// Parse `(name, r:id)` pairs from each `<sheet>` element in `xl/workbook.xml`.
///
/// The returned vector preserves workbook order. Callers should resolve each
/// `r:id` to a ZIP entry path using [`parse_workbook_rels`].
pub fn parse_sheet_entries(workbook_xml: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    // WHY: rust_xlsxwriter emits compact XML — multiple sheet tags may share a line.
    for sheet_xml in workbook_xml.split("<sheet").skip(1) {
        let Some(name_start) = sheet_xml.find("name=\"") else {
            continue;
        };
        let after_name = name_start + 6;
        let Some(name_rest) = sheet_xml.get(after_name..) else {
            continue;
        };
        let Some(name_end) = name_rest.find('"') else {
            continue;
        };
        let Some(sheet_name) = name_rest.get(..name_end) else {
            continue;
        };

        let Some(rid_start) = sheet_xml.find("r:id=\"") else {
            continue;
        };
        let after_rid = rid_start + 6;
        let Some(rid_rest) = sheet_xml.get(after_rid..) else {
            continue;
        };
        let Some(rid_end) = rid_rest.find('"') else {
            continue;
        };
        let Some(rid) = rid_rest.get(..rid_end) else {
            continue;
        };

        entries.push((sheet_name.to_string(), rid.to_string()));
    }
    entries
}

/// Parse `xl/_rels/workbook.xml.rels` into an `rId -> target` map.
///
/// Targets are relative to the `xl/` directory. Only `Relationship` elements
/// carrying non-empty `Id` and `Target` attributes are included.
pub fn parse_workbook_rels(rels_xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for rel_xml in rels_xml.split("<Relationship").skip(1) {
        let Some(id_start) = rel_xml.find("Id=\"") else {
            continue;
        };
        let after_id = id_start + 4;
        let Some(id_rest) = rel_xml.get(after_id..) else {
            continue;
        };
        let Some(id_end) = id_rest.find('"') else {
            continue;
        };
        let Some(id) = id_rest.get(..id_end) else {
            continue;
        };

        let Some(target_start) = rel_xml.find("Target=\"") else {
            continue;
        };
        let after_target = target_start + 8;
        let Some(target_rest) = rel_xml.get(after_target..) else {
            continue;
        };
        let Some(target_end) = target_rest.find('"') else {
            continue;
        };
        let Some(target) = target_rest.get(..target_end) else {
            continue;
        };

        map.insert(id.to_string(), target.to_string());
    }
    map
}

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

/// Read every `ppt/slides/slideN.xml` part from a PPTX archive, gap-safe and
/// ordered by slide number.
///
/// WHY: slide part names are not guaranteed contiguous -- deleting a slide in
/// `PowerPoint` leaves the remaining part names unrenumbered, so a deck can hold
/// slide1, slide2 and slide4. Probing slide{n} upward and stopping at the
/// first missing index silently drops every slide past the gap. Enumerating
/// the archive and sorting by slide number reads the whole deck regardless of
/// where the gaps fall.
///
/// # Errors
///
/// Returns an error if `bytes` is not a valid ZIP archive or a slide part
/// cannot be read.
pub fn read_pptx_slides(bytes: &[u8]) -> ArchiveResult<Vec<String>> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|source| ArchiveError::Zip { source })?;

    let mut numbered: Vec<(u32, String)> = (0..archive.len())
        .map(|i| {
            archive
                .by_index(i)
                .map(|f| f.name().to_owned())
                .map_err(|source| ArchiveError::Zip { source })
        })
        .collect::<ArchiveResult<Vec<String>>>()?
        .into_iter()
        .filter_map(|name| slide_number(&name).map(|n| (n, name)))
        .collect();
    numbered.sort_by_key(|(n, _)| *n);

    let mut slides = Vec::with_capacity(numbered.len());
    for (_, name) in &numbered {
        let mut file = archive
            .by_name(name)
            .map_err(|source| ArchiveError::Zip { source })?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|source| ArchiveError::Io { source })?;
        slides.push(content);
    }

    Ok(slides)
}

/// Shared parts of an XLSX workbook read from its archive.
///
/// Carries the raw material every worksheet reader needs; each caller applies
/// its own transform (text concatenation, cell-map extraction, ...) to the
/// worksheet XML in `sheets`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct WorkbookParts {
    /// Shared string table from `xl/sharedStrings.xml`, empty when the part
    /// is absent.
    pub shared_strings: Vec<String>,
    /// `(sheet_name, worksheet_xml)` pairs in workbook order.
    ///
    /// `worksheet_xml` is `None` when the worksheet part resolved from
    /// `xl/workbook.xml` and its rels is missing from the archive; callers
    /// decide how to represent an absent sheet.
    pub sheets: Vec<(String, Option<String>)>,
}

/// Read the shared parts of an XLSX workbook: the shared-string table and
/// each sheet's worksheet XML, in workbook order.
///
/// Resolves each sheet's worksheet path via `xl/_rels/workbook.xml.rels`,
/// falling back to the conventional `xl/worksheets/sheet{n}.xml` name when a
/// sheet has no matching relationship.
///
/// # Errors
///
/// Returns an error if `bytes` is not a valid ZIP archive, or if the required
/// `xl/workbook.xml` part cannot be read.
pub fn read_workbook_parts(bytes: &[u8]) -> ArchiveResult<WorkbookParts> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|source| ArchiveError::Zip { source })?;

    let shared_strings = if let Ok(mut file) = archive.by_name("xl/sharedStrings.xml") {
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|source| ArchiveError::Io { source })?;
        extract_shared_strings(&content)
    } else {
        Vec::new()
    };

    let workbook_xml = {
        let mut file = archive
            .by_name("xl/workbook.xml")
            .map_err(|source| ArchiveError::Zip { source })?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|source| ArchiveError::Io { source })?;
        content
    };

    let rels_xml = if let Ok(mut file) = archive.by_name("xl/_rels/workbook.xml.rels") {
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|source| ArchiveError::Io { source })?;
        content
    } else {
        String::new()
    };

    let rels = parse_workbook_rels(&rels_xml);
    let sheet_entries = parse_sheet_entries(&workbook_xml);

    let mut sheets = Vec::with_capacity(sheet_entries.len());
    for (idx, (sheet_name, rid)) in sheet_entries.into_iter().enumerate() {
        let worksheet_path = rels.get(&rid).map_or_else(
            || format!("xl/worksheets/sheet{}.xml", idx + 1),
            |target| format!("xl/{target}"),
        );
        let content = if let Ok(mut file) = archive.by_name(&worksheet_path) {
            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|source| ArchiveError::Io { source })?;
            Some(content)
        } else {
            None
        };
        sheets.push((sheet_name, content));
    }

    Ok(WorkbookParts {
        shared_strings,
        sheets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_shared_strings_returns_text_content() {
        let xml = r"<sst><si><t>Hello</t></si><si><t>World</t></si></sst>";
        let result = extract_shared_strings(xml);
        assert_eq!(result, vec!["Hello", "World"]);
    }

    #[test]
    fn extract_shared_strings_concatenates_multiple_t_elements() {
        let xml = r"<sst><si><t>foo</t><t>bar</t></si></sst>";
        let result = extract_shared_strings(xml);
        assert_eq!(result, vec!["foobar"]);
    }

    #[test]
    fn extract_shared_strings_unescapes_xml_character_entities() {
        let xml = r"<sst><si><t>A &amp; B &lt; C &gt; D &apos;Q&apos; &quot;R&quot; &#x2019;</t></si></sst>";
        let result = extract_shared_strings(xml);
        assert_eq!(result, vec!["A & B < C > D 'Q' \"R\" \u{2019}"]);
    }

    #[test]
    fn extract_text_from_slide_joins_a_t_elements() {
        let xml = r"<p:sp><a:t>Hello</a:t><a:t>world</a:t></p:sp>";
        let result = extract_text_from_slide(xml);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn extract_text_from_slide_unescapes_xml_character_entities() {
        let xml =
            r"<p:sp><a:t>A &amp; B &lt; C &gt; D &apos;Q&apos; &quot;R&quot; &#x2019;</a:t></p:sp>";
        let result = extract_text_from_slide(xml);
        assert_eq!(result, "A & B < C > D 'Q' \"R\" \u{2019}");
    }

    #[test]
    fn extract_text_from_slide_empty_returns_empty_string() {
        assert_eq!(extract_text_from_slide("<p:sp></p:sp>"), "");
    }

    #[test]
    fn parse_sheet_entries_returns_names_and_rids_in_order() {
        let xml = r#"<workbook><sheets><sheet name="Alpha" r:id="rId1"/><sheet name="Beta" r:id="rId2"/></sheets></workbook>"#;
        let result = parse_sheet_entries(xml);
        assert_eq!(
            result,
            vec![
                ("Alpha".to_string(), "rId1".to_string()),
                ("Beta".to_string(), "rId2".to_string())
            ]
        );
    }

    #[test]
    fn parse_sheet_entries_skips_sheets_without_rid() {
        let xml = r#"<workbook><sheets><sheet name="Alpha" r:id="rId1"/><sheet name="Orphan"/></sheets></workbook>"#;
        let result = parse_sheet_entries(xml);
        assert_eq!(result, vec![("Alpha".to_string(), "rId1".to_string())]);
    }

    #[test]
    fn parse_workbook_rels_builds_rid_to_target_map() {
        let xml = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
            <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet3.xml"/>
            <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
        </Relationships>"#;
        let result = parse_workbook_rels(xml);
        assert_eq!(
            result.get("rId1"),
            Some(&"worksheets/sheet1.xml".to_string())
        );
        assert_eq!(
            result.get("rId2"),
            Some(&"worksheets/sheet3.xml".to_string())
        );
        assert_eq!(result.get("rId3"), Some(&"sharedStrings.xml".to_string()));
    }

    #[test]
    fn parse_workbook_rels_ignores_malformed_relationships() {
        let xml = r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Target="no-id.xml"/></Relationships>"#;
        let result = parse_workbook_rels(xml);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.get("rId1"),
            Some(&"worksheets/sheet1.xml".to_string())
        );
    }

    /// Build a PPTX whose slide parts carry exactly the given numbers,
    /// written to the archive in the order supplied.
    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn pptx_with_numbered_slides(slides: &[(u32, &str)]) -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let mut cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

        for (number, text) in slides {
            let slide = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#
            );
            let name = format!("ppt/slides/slide{number}.xml");
            zip.start_file(&name, options).expect("start slide part");
            zip.write_all(slide.as_bytes()).expect("write slide part");
        }

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn read_pptx_slides_reads_past_a_numbering_gap() {
        // WHY: deleting a slide in PowerPoint leaves the surviving part names
        // unrenumbered, so slide3 can be absent while slide4 exists. Probing
        // upward from slide1 and stopping at the first missing index returned
        // only the first two slides and silently dropped the rest.
        let bytes = pptx_with_numbered_slides(&[(1, "first"), (2, "second"), (4, "fourth")]);
        let slides = read_pptx_slides(&bytes).expect("read must succeed");

        let texts: Vec<String> = slides
            .iter()
            .map(|xml| extract_text_from_slide(xml))
            .collect();
        assert_eq!(
            texts,
            vec!["first".to_owned(), "second".to_owned(), "fourth".to_owned()],
            "every slide part must be read, including those past a numbering gap"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn read_pptx_slides_orders_by_number_not_archive_order() {
        // WHY: ZIP entry order is arbitrary, so slide order must come from the
        // part number rather than the order the parts happen to be stored in.
        let bytes = pptx_with_numbered_slides(&[(3, "third"), (1, "first"), (2, "second")]);
        let slides = read_pptx_slides(&bytes).expect("read must succeed");

        let texts: Vec<String> = slides
            .iter()
            .map(|xml| extract_text_from_slide(xml))
            .collect();
        assert_eq!(
            texts,
            vec!["first".to_owned(), "second".to_owned(), "third".to_owned()],
            "slides must be ordered by slide number"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn read_pptx_slides_ignores_non_slide_parts() {
        // WHY: sibling prefixes under `ppt/` hold template parts whose names
        // also begin with `slide`, and `_rels` entries repeat the slide part
        // names. Only the exact `ppt/slides/slideN.xml` shape is a slide.
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let mut cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);
        for name in [
            "ppt/slideLayouts/slideLayout1.xml",
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/slides/_rels/slide1.xml.rels",
        ] {
            zip.start_file(name, options).expect("start part");
            zip.write_all(b"<x/>").expect("write part");
        }
        zip.finish().expect("finish zip");
        let bytes = cursor.into_inner();

        let slides = read_pptx_slides(&bytes).expect("read must succeed");
        assert!(
            slides.is_empty(),
            "non-slide parts must not be counted, got: {slides:?}"
        );
    }

    /// Build an XLSX whose workbook declares sheets `Alpha` (backed by
    /// `xl/worksheets/sheet1.xml`) and `Ghost` (declared in `xl/workbook.xml`
    /// but never written to the archive, with no rels part to resolve it).
    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn xlsx_with_missing_worksheet() -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Alpha" sheetId="1" r:id="rId1"/>
    <sheet name="Ghost" sheetId="2" r:id="rId2"/>
  </sheets>
</workbook>"#;

        const SHEET1: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1"><v>1</v></c></row>
  </sheetData>
</worksheet>"#;

        let mut cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("xl/workbook.xml", options)
            .expect("start workbook.xml");
        zip.write_all(WORKBOOK.as_bytes())
            .expect("write workbook.xml");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("start sheet1.xml");
        zip.write_all(SHEET1.as_bytes()).expect("write sheet1.xml");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn read_workbook_parts_marks_missing_worksheet_as_none() {
        let bytes = xlsx_with_missing_worksheet();
        let parts = read_workbook_parts(&bytes).expect("read must succeed");

        assert_eq!(
            parts.sheets.len(),
            2,
            "every declared sheet must appear, even when its worksheet part is missing"
        );
        let (alpha_name, alpha_content) = parts.sheets.first().expect("Alpha entry present");
        assert_eq!(alpha_name, "Alpha");
        assert!(alpha_content.is_some(), "Alpha's worksheet part is present");

        let (ghost_name, ghost_content) = parts.sheets.get(1).expect("Ghost entry present");
        assert_eq!(ghost_name, "Ghost");
        assert!(
            ghost_content.is_none(),
            "Ghost has no resolvable worksheet part, and must read as absent rather than erroring"
        );
    }
}
