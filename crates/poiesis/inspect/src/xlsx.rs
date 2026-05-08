//! XLSX workbook text extraction implementation.

use std::io::Cursor;

use indexmap::IndexMap;
use zip::ZipArchive;

use crate::WorkbookSummary;
use crate::error::Result;

/// Extract shared strings from `xl/sharedStrings.xml`.
fn extract_shared_strings(xml_data: &str) -> Vec<String> {
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
                    text.push_str(slice);
                }
            }
            strings.push(text);
        }
    }
    strings
}

/// Extract text content from worksheet XML using simple string matching,
/// resolving shared-string indices via `shared_strings`.
fn extract_text_from_worksheet(xml_data: &str, shared_strings: &[String]) -> String {
    let mut text_content = String::new();

    let mut in_row = false;
    for chunk in xml_data.split("<row") {
        if !chunk.is_empty() {
            in_row = true;
        }
        for cell_chunk in chunk.split("<c") {
            // Determine if this is a shared-string cell
            let is_shared = cell_chunk.contains("t=\"s\"");
            // Find value tags
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
        if in_row {
            text_content.push('\n');
            in_row = false;
        }
    }

    text_content
}

pub(crate) fn inspect_xlsx_impl(bytes: &[u8]) -> Result<WorkbookSummary> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| crate::InspectError::ZipError { source: e })?;

    // Read shared strings if present
    let shared_strings = if let Ok(mut file) = archive.by_name("xl/sharedStrings.xml") {
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| crate::InspectError::Io { source: e })?;
        extract_shared_strings(&content)
    } else {
        Vec::new()
    };

    let mut sheets: IndexMap<String, String> = IndexMap::new();

    // Read workbook.xml to get sheet names in workbook order
    let workbook_xml = {
        let mut file = archive
            .by_name("xl/workbook.xml")
            .map_err(|e| crate::InspectError::ZipError { source: e })?;
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| crate::InspectError::Io { source: e })?;
        content
    };

    // Extract sheet names from workbook.xml, preserving order
    let mut sheet_names = Vec::new();
    for line in workbook_xml.lines() {
        if !line.contains("<sheet") {
            continue;
        }
        let Some(start) = line.find("name=\"") else {
            continue;
        };
        let after_name = start + 6;
        let Some(rest) = line.get(after_name..) else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        let Some(sheet_name) = rest.get(..end) else {
            continue;
        };
        sheet_names.push(sheet_name.to_string());
    }

    // Read worksheet files
    for (idx, sheet_name) in sheet_names.into_iter().enumerate() {
        let worksheet_path = format!("xl/worksheets/sheet{}.xml", idx + 1);
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
