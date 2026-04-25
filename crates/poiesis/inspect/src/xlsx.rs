//! XLSX workbook text extraction implementation.

use std::collections::BTreeMap;
use std::io::Cursor;

use zip::ZipArchive;

use crate::WorkbookSummary;
use crate::error::Result;

/// Extract text content from worksheet XML using simple string matching.
fn extract_text_from_worksheet(xml_data: &str) -> String {
    let mut text_content = String::new();

    // Simple approach: find all <v>...</v> tags (cell values)
    let mut in_row = false;
    for chunk in xml_data.split("<row") {
        if !chunk.is_empty() {
            in_row = true;
        }
        for cell_chunk in chunk.split("<c") {
            // Find value tags
            for value_chunk in cell_chunk.split("<v>") {
                if let Some(end) = value_chunk.find("</v>")
                    && let Some(value) = value_chunk.get(..end)
                {
                    text_content.push_str(value);
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

    let mut sheets: BTreeMap<String, String> = BTreeMap::new();

    // Read workbook.xml to get sheet names
    let workbook_xml = {
        let mut file = archive
            .by_name("xl/workbook.xml")
            .map_err(|e| crate::InspectError::ZipError { source: e })?;
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| crate::InspectError::Io { source: e })?;
        content
    };

    // Extract sheet names from workbook.xml
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
            let text = extract_text_from_worksheet(&content);
            sheets.insert(sheet_name, text);
        }
    }

    Ok(WorkbookSummary { sheets })
}
