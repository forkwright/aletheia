//! XLSX workbook diffing implementation.

use indexmap::IndexMap;
use poiesis_ooxml_parse::read_workbook_parts;

use crate::CellDiff;
use crate::error::Result;

/// Map of cell coordinates to their string values within a single sheet.
type SheetCells = IndexMap<(u32, u32), String>;

/// Map of sheet names to their cell data across a workbook, in workbook order.
type WorkbookData = IndexMap<String, SheetCells>;

/// Parse cell value from XML worksheet using simple string matching,
/// resolving shared-string indices via `shared_strings`.
fn extract_cells_from_worksheet(xml_data: &str, shared_strings: &[String]) -> SheetCells {
    let mut cells = IndexMap::new();

    let mut current_row: u32 = 0;

    for row_chunk in xml_data.split("<row") {
        if let Some(r_start) = row_chunk.find("r=\"")
            && let Some(rest) = row_chunk.get(r_start + 3..)
            && let Some(r_end) = rest.find('"')
            && let Some(num_str) = rest.get(..r_end)
            && let Ok(r) = num_str.parse::<u32>()
        {
            current_row = r.saturating_sub(1);
        }

        for cell_chunk in row_chunk.split("<c") {
            let is_shared = cell_chunk.contains("t=\"s\"");

            if let Some(r_start) = cell_chunk.find("r=\"")
                && let Some(rest) = cell_chunk.get(r_start + 3..)
                && let Some(r_end) = rest.find('"')
                && let Some(cell_ref) = rest.get(..r_end)
            {
                let col_idx = cell_ref
                    .chars()
                    .take_while(|c| c.is_alphabetic())
                    .fold(0u32, |acc, c| {
                        acc * 26 + (u32::from(c.to_ascii_uppercase()) - u32::from(b'A') + 1)
                    })
                    .saturating_sub(1);

                if let Some(v_start) = cell_chunk.find("<v>")
                    && let Some(rest) = cell_chunk.get(v_start + 3..)
                    && let Some(v_end) = rest.find("</v>")
                    && let Some(value) = rest.get(..v_end)
                {
                    let resolved = if is_shared {
                        value
                            .parse::<usize>()
                            .ok()
                            .and_then(|idx| shared_strings.get(idx))
                            .cloned()
                            .unwrap_or_else(|| value.to_string())
                    } else {
                        value.to_string()
                    };
                    cells.insert((current_row, col_idx), resolved);
                }
            }
        }
    }

    cells
}

/// Extract sheet names and cell data from XLSX archive, preserving workbook order.
///
/// A sheet whose worksheet part is missing from the archive still appears in
/// the result, with an empty cell map — a diff against a version of the
/// workbook where that sheet has content must report the difference rather
/// than silently omitting the sheet.
fn read_workbook(bytes: &[u8]) -> Result<WorkbookData> {
    let parts = read_workbook_parts(bytes)?;

    let mut workbook_data: WorkbookData = IndexMap::new();
    for (sheet_name, content) in parts.sheets {
        let cells = content
            .map(|xml| extract_cells_from_worksheet(&xml, &parts.shared_strings))
            .unwrap_or_default();
        workbook_data.insert(sheet_name, cells);
    }

    Ok(workbook_data)
}

pub(crate) fn diff_workbooks_impl(a: &[u8], b: &[u8]) -> Result<Vec<CellDiff>> {
    let workbook_a = read_workbook(a)?;
    let workbook_b = read_workbook(b)?;

    let mut diffs = Vec::new();

    for (sheet_name, cells_a) in &workbook_a {
        let cells_b = workbook_b.get(sheet_name).cloned().unwrap_or_default();

        // Find deleted and modified cells
        for ((row, col), value_a) in cells_a {
            match cells_b.get(&(*row, *col)) {
                None => {
                    diffs.push(CellDiff {
                        sheet: sheet_name.clone(),
                        row: *row,
                        col: *col,
                        before: Some(value_a.clone()),
                        after: None,
                    });
                }
                Some(value_b) if value_a != value_b => {
                    diffs.push(CellDiff {
                        sheet: sheet_name.clone(),
                        row: *row,
                        col: *col,
                        before: Some(value_a.clone()),
                        after: Some(value_b.clone()),
                    });
                }
                _ => (), // kanon:ignore RUST/empty-match-arm — intentional no-op for matching cells; kanon:ignore RUST/silent-wildcard-success — cells with identical values require no diff action
            }
        }

        // Find inserted cells
        for ((row, col), value_b) in &cells_b {
            if !cells_a.contains_key(&(*row, *col)) {
                diffs.push(CellDiff {
                    sheet: sheet_name.clone(),
                    row: *row,
                    col: *col,
                    before: None,
                    after: Some(value_b.clone()),
                });
            }
        }
    }

    // NOTE: emit insertions for sheets that exist only in workbook_b; the
    // loop above never visits them because it iterates over workbook_a keys.
    for (sheet_name, cells_b) in &workbook_b {
        if !workbook_a.contains_key(sheet_name) {
            for ((row, col), value_b) in cells_b {
                diffs.push(CellDiff {
                    sheet: sheet_name.clone(),
                    row: *row,
                    col: *col,
                    before: None,
                    after: Some(value_b.clone()),
                });
            }
        }
    }

    Ok(diffs)
}
