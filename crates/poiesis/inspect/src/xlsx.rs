//! XLSX workbook text extraction implementation.

use std::io::Cursor;

use indexmap::IndexMap;
use poiesis_ooxml_parse::{extract_shared_strings, parse_sheet_entries, parse_workbook_rels};
use zip::ZipArchive;

use crate::WorkbookSummary;
use crate::error::Result;

/// Extract text content from worksheet XML using simple string matching,
/// resolving shared-string indices via `shared_strings`.
fn extract_text_from_worksheet(xml_data: &str, shared_strings: &[String]) -> String {
    let mut text_content = String::new();

    // WHY: split("<row") yields the XML preamble as its first element; skipping it
    // ensures every iterated chunk corresponds to exactly one row and prevents a
    // spurious leading newline.
    for chunk in xml_data.split("<row").skip(1) {
        for cell_chunk in chunk.split("<c") {
            let is_shared = cell_chunk.contains("t=\"s\"");
            for value_chunk in cell_chunk.split("<v>") {
                if let Some(end) = value_chunk.find("</v>")
                    && let Some(value) = value_chunk.get(..end)
                {
                    let resolved = if is_shared {
                        value.parse::<usize>().ok().map_or(value, |idx| {
                            shared_strings.get(idx).map_or(value, String::as_str)
                        })
                    } else {
                        value
                    };
                    text_content.push_str(resolved);
                    text_content.push('\t');
                }
            }
        }
        text_content.push('\n');
    }

    text_content
}

pub(crate) fn inspect_xlsx_impl(bytes: &[u8]) -> Result<WorkbookSummary> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| crate::InspectError::ZipError { source: e })?;

    let shared_strings = if let Ok(mut file) = archive.by_name("xl/sharedStrings.xml") {
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| crate::InspectError::Io { source: e })?;
        extract_shared_strings(&content)
    } else {
        Vec::new()
    };

    let mut sheets: IndexMap<String, String> = IndexMap::new();

    let workbook_xml = {
        let mut file = archive
            .by_name("xl/workbook.xml")
            .map_err(|e| crate::InspectError::ZipError { source: e })?;
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| crate::InspectError::Io { source: e })?;
        content
    };

    let rels_xml = if let Ok(mut file) = archive.by_name("xl/_rels/workbook.xml.rels") {
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| crate::InspectError::Io { source: e })?;
        content
    } else {
        String::new()
    };

    let rels = parse_workbook_rels(&rels_xml);
    let sheet_entries = parse_sheet_entries(&workbook_xml);

    for (idx, (sheet_name, rid)) in sheet_entries.into_iter().enumerate() {
        let worksheet_path = rels.get(&rid).map_or_else(
            || format!("xl/worksheets/sheet{}.xml", idx + 1),
            |target| format!("xl/{target}"),
        );
        if let Ok(mut file) = archive.by_name(&worksheet_path) {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut file, &mut content)
                .map_err(|e| crate::InspectError::Io { source: e })?;
            let text = extract_text_from_worksheet(&content, &shared_strings);
            sheets.insert(sheet_name, text);
        }
    }

    Ok(WorkbookSummary { sheets })
}
