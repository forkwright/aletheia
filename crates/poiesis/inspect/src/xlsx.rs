//! XLSX workbook text extraction implementation.

use indexmap::IndexMap;
use poiesis_ooxml_parse::read_workbook_parts;

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
    let parts = read_workbook_parts(bytes)?;

    let mut sheets: IndexMap<String, String> = IndexMap::new();
    for (sheet_name, content) in parts.sheets {
        if let Some(content) = content {
            let text = extract_text_from_worksheet(&content, &parts.shared_strings);
            sheets.insert(sheet_name, text);
        }
    }

    Ok(WorkbookSummary { sheets })
}
