#![deny(missing_docs)]
//! Shared OOXML parsing primitives used by `poiesis-inspect` and `poiesis-diff`.
//!
//! These helpers perform minimal, dependency-light extraction of text and
//! workbook metadata from Office Open XML parts. They intentionally avoid
//! pulling in a full XML parser; callers that need structural validation
//! should use a dedicated OOXML library.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_shared_strings_returns_text_content() {
        let xml = r#"<sst><si><t>Hello</t></si><si><t>World</t></si></sst>"#;
        let result = extract_shared_strings(xml);
        assert_eq!(result, vec!["Hello", "World"]);
    }

    #[test]
    fn extract_shared_strings_concatenates_multiple_t_elements() {
        let xml = r#"<sst><si><t>foo</t><t>bar</t></si></sst>"#;
        let result = extract_shared_strings(xml);
        assert_eq!(result, vec!["foobar"]);
    }

    #[test]
    fn extract_text_from_slide_joins_a_t_elements() {
        let xml = r#"<p:sp><a:t>Hello</a:t><a:t>world</a:t></p:sp>"#;
        let result = extract_text_from_slide(xml);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn extract_text_from_slide_empty_returns_empty_string() {
        assert_eq!(extract_text_from_slide("<p:sp></p:sp>"), "");
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
}
