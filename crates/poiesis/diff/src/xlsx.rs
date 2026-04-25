//! XLSX workbook diffing implementation.

use std::collections::BTreeMap;
use std::io::Cursor;

use zip::ZipArchive;

use crate::CellDiff;
use crate::error::Result;

/// Map of cell coordinates to their string values within a single sheet.
type SheetCells = BTreeMap<(u32, u32), String>;

/// Map of sheet names to their cell data across a workbook.
type WorkbookData = BTreeMap<String, SheetCells>;

/// Parse cell value from XML worksheet using simple string matching.
fn extract_cells_from_worksheet(xml_data: &str) -> SheetCells {
    let mut cells = BTreeMap::new();

    // Simple regex-free approach: split on <row> and <c> tags
    let mut current_row: u32 = 0;

    for row_chunk in xml_data.split("<row") {
        // Extract row number from r="N" attribute
        if let Some(r_start) = row_chunk.find("r=\"")
            && let Some(rest) = row_chunk.get(r_start + 3..)
            && let Some(r_end) = rest.find('"')
            && let Some(num_str) = rest.get(..r_end)
            && let Ok(r) = num_str.parse::<u32>()
        {
            current_row = r.saturating_sub(1);
        }

        // Extract cells from this row
        for cell_chunk in row_chunk.split("<c") {
            // Extract cell reference and value
            if let Some(r_start) = cell_chunk.find("r=\"")
                && let Some(rest) = cell_chunk.get(r_start + 3..)
                && let Some(r_end) = rest.find('"')
                && let Some(cell_ref) = rest.get(..r_end)
            {
                // Parse column from cell reference
                let col_idx = cell_ref
                    .chars()
                    .take_while(|c| c.is_alphabetic())
                    .fold(0u32, |acc, c| {
                        acc * 26 + (u32::from(c.to_ascii_uppercase()) - u32::from(b'A') + 1)
                    })
                    .saturating_sub(1);

                // Extract value from <v>...</v>
                if let Some(v_start) = cell_chunk.find("<v>")
                    && let Some(rest) = cell_chunk.get(v_start + 3..)
                    && let Some(v_end) = rest.find("</v>")
                    && let Some(value) = rest.get(..v_end)
                {
                    cells.insert((current_row, col_idx), value.to_string());
                }
            }
        }
    }

    cells
}

/// Extract sheet names and cell data from XLSX archive.
fn read_workbook(bytes: &[u8]) -> Result<WorkbookData> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| crate::DiffError::ZipError { source: e })?;

    let mut workbook_data: WorkbookData = BTreeMap::new();

    // Read workbook.xml to get sheet names
    let workbook_xml = {
        let mut file = archive
            .by_name("xl/workbook.xml")
            .map_err(|e| crate::DiffError::ZipError { source: e })?;
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| crate::DiffError::Io { source: e })?;
        content
    };

    // Simple extraction of sheet names
    for line in workbook_xml.lines() {
        if line.contains("<sheet")
            && let Some(start) = line.find("name=\"")
            && let Some(rest) = line.get(start + 6..)
            && let Some(end) = rest.find('"')
            && let Some(sheet_name) = rest.get(..end)
        {
            workbook_data.insert(sheet_name.to_string(), BTreeMap::new());
        }
    }

    // Read worksheet files (xl/worksheets/sheet1.xml, etc.)
    for (sheet_idx, cell_map) in (1..).zip(workbook_data.values_mut()) {
        let worksheet_path = format!("xl/worksheets/sheet{sheet_idx}.xml");
        if let Ok(mut file) = archive.by_name(&worksheet_path) {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut file, &mut content)
                .map_err(|e| crate::DiffError::Io { source: e })?;
            *cell_map = extract_cells_from_worksheet(&content);
        }
    }

    Ok(workbook_data)
}

pub(crate) fn diff_workbooks_impl(a: &[u8], b: &[u8]) -> Result<Vec<CellDiff>> {
    let workbook_a = read_workbook(a)?;
    let workbook_b = read_workbook(b)?;

    let mut diffs = Vec::new();

    // Compare all sheets
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
                _ => {}
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

    Ok(diffs)
}
