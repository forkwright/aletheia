#![deny(missing_docs)]
//! Shared OOXML parsing primitives used by `poiesis-inspect` and `poiesis-diff`.
//!
//! These helpers perform minimal, dependency-light extraction of text and
//! workbook metadata from Office Open XML parts. They intentionally avoid
//! pulling in a full XML parser; callers that need structural validation
//! should use a dedicated OOXML library.

use std::collections::HashMap;

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
                    text.push_str(slice);
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
            text_content.push_str(text);
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

/// Parse sheet names from `xl/workbook.xml` in workbook order.
///
/// Returns the `name` attribute of each `<sheet>` element. The caller is
/// responsible for correlating these names with worksheet ZIP entry paths.
pub fn parse_sheet_names(workbook_xml: &str) -> Vec<String> {
    let mut sheet_names = Vec::new();
    // WHY: rust_xlsxwriter emits compact XML — multiple sheet tags may share a line.
    for sheet_xml in workbook_xml.split("<sheet").skip(1) {
        let Some(start) = sheet_xml.find("name=\"") else {
            continue;
        };
        let after_name = start + 6;
        let Some(rest) = sheet_xml.get(after_name..) else {
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
    sheet_names
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
    fn extract_text_from_slide_joins_a_t_elements() {
        let xml = r"<p:sp><a:t>Hello</a:t><a:t>world</a:t></p:sp>";
        let result = extract_text_from_slide(xml);
        assert_eq!(result, "Hello world");
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
    fn parse_sheet_names_returns_names_in_order() {
        let xml = r#"<workbook><sheets><sheet name="Alpha" r:id="rId1"/><sheet name="Beta" r:id="rId2"/></sheets></workbook>"#;
        let result = parse_sheet_names(xml);
        assert_eq!(result, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn parse_sheet_names_no_sheets_returns_empty() {
        assert!(parse_sheet_names("<workbook><sheets/></workbook>").is_empty());
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
}
