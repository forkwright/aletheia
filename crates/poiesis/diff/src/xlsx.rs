//! XLSX workbook diffing implementation.

use std::collections::BTreeMap;
use std::io::Cursor;

use zip::ZipArchive;

use crate::CellDiff;
use crate::error::Result;

/// Parse cell value from XML worksheet using simple string matching.
fn extract_cells_from_worksheet(xml_data: &str) -> Result<BTreeMap<(u32, u32), String>> {
    let mut cells = BTreeMap::new();

    // Simple regex-free approach: split on <row> and <c> tags
    let mut current_row: u32 = 0;

    for row_chunk in xml_data.split("<row") {
        // Extract row number from r="N" attribute
        if let Some(r_start) = row_chunk.find("r=\"") {
            if let Some(r_end) = row_chunk[r_start + 3..].find('"') {
                if let Ok(r) = row_chunk[r_start + 3..r_start + 3 + r_end].parse::<u32>() {
                    current_row = r.saturating_sub(1);
                }
            }
        }

        // Extract cells from this row
        for cell_chunk in row_chunk.split("<c") {
            // Extract cell reference and value
            if let Some(r_start) = cell_chunk.find("r=\"") {
                if let Some(r_end) = cell_chunk[r_start + 3..].find('"') {
                    let cell_ref = &cell_chunk[r_start + 3..r_start + 3 + r_end];
                    // Parse column from cell reference
                    let col_idx = cell_ref
                        .chars()
                        .take_while(|c| c.is_alphabetic())
                        .fold(0u32, |acc, c| {
                            acc * 26 + (c.to_ascii_uppercase() as u32 - b'A' as u32 + 1)
                        })
                        .saturating_sub(1);

                    // Extract value from <v>...</v>
                    if let Some(v_start) = cell_chunk.find("<v>") {
                        if let Some(v_end) = cell_chunk[v_start + 3..].find("</v>") {
                            let value = cell_chunk[v_start + 3..v_start + 3 + v_end].to_string();
                            cells.insert((current_row, col_idx), value);
                        }
                    }
                }
            }
        }
    }

    Ok(cells)
}

/// Extract sheet names and cell data from XLSX archive.
fn read_workbook(bytes: &[u8]) -> Result<BTreeMap<String, BTreeMap<(u32, u32), String>>> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| crate::DiffError::ZipError { source: e })?;

    let mut workbook_data: BTreeMap<String, BTreeMap<(u32, u32), String>> = BTreeMap::new();

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
        if line.contains("<sheet") {
            if let Some(start) = line.find("name=\"") {
                if let Some(end) = line[start + 6..].find('"') {
                    let sheet_name = &line[start + 6..start + 6 + end];
                    workbook_data.insert(sheet_name.to_string(), BTreeMap::new());
                }
            }
        }
    }

    // Read worksheet files (xl/worksheets/sheet1.xml, etc.)
    let mut sheet_idx = 1;
    for (_sheet_name, cell_map) in &mut workbook_data {
        let worksheet_path = format!("xl/worksheets/sheet{}.xml", sheet_idx);
        if let Ok(mut file) = archive.by_name(&worksheet_path) {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut file, &mut content)
                .map_err(|e| crate::DiffError::Io { source: e })?;
            *cell_map = extract_cells_from_worksheet(&content)?;
        }
        sheet_idx += 1;
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
