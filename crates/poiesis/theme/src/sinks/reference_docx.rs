//! Pandoc-compatible `reference.docx` sink.
//!
//! Emits a ZIP archive containing a minimal DOCX package whose
//! `word/styles.xml` carries the theme's brand fonts and colors so that
//! Pandoc-generated Word documents inherit the typography and palette.

use std::io::Write;
use std::sync::LazyLock;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::error::ThemeError;
use crate::resolved::ResolvedTheme;

// WHY: standardised OOXML identifier URI, not an endpoint
const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
// WHY: standardised OOXML identifier URI, not an endpoint
const NS_PKG_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
// WHY: standardised OOXML identifier URI, not an endpoint
const NS_PKG_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
// WHY: standardised OOXML identifier URI, not an endpoint
const REL_TYPE_OFFICE_DOC: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
// WHY: standardised OOXML identifier URI, not an endpoint
const REL_TYPE_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
// WHY: standardised OOXML identifier URI, not an endpoint
const REL_TYPE_SETTINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
// WHY: standardised OOXML identifier URI, not an endpoint
const REL_TYPE_THEME: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";

/// Emit a Pandoc-compatible `reference.docx` as a ZIP byte vector.
///
/// The archive contains the minimal OOXML package required by Pandoc's
/// reference-doc reader, with `word/styles.xml` applying the theme's brand
/// fonts and colors to standard paragraph styles.
///
/// # Errors
///
/// Returns [`ThemeError::ZipWrite`] if any ZIP entry fails to write.
pub fn emit_reference_docx(theme: &ResolvedTheme) -> Result<Vec<u8>, ThemeError> {
    let buf = Vec::new();
    let cursor = std::io::Cursor::new(buf);
    let mut zip = ZipWriter::new(cursor);

    pack_entry(&mut zip, "[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
    pack_entry(&mut zip, "_rels/.rels", RELS_RELS.as_bytes())?;
    pack_entry(
        &mut zip,
        "word/document.xml",
        word_document_xml().as_bytes(),
    )?;
    pack_entry(
        &mut zip,
        "word/_rels/document.xml.rels",
        WORD_DOCUMENT_RELS.as_bytes(),
    )?;
    pack_entry(
        &mut zip,
        "word/styles.xml",
        build_styles_xml(theme).as_bytes(),
    )?;
    pack_entry(&mut zip, "word/settings.xml", WORD_SETTINGS.as_bytes())?;
    pack_entry(
        &mut zip,
        "word/theme/theme1.xml",
        crate::sinks::ooxml::emit_theme_xml(theme)?.as_bytes(),
    )?;

    let cursor = zip.finish().map_err(|e| ThemeError::ZipWrite {
        sink: "reference_docx".into(),
        entry: "__finish__".into(),
        message: e.to_string(),
    })?;
    Ok(cursor.into_inner())
}

fn pack_entry(
    zip: &mut ZipWriter<std::io::Cursor<Vec<u8>>>,
    name: &str,
    data: &[u8],
) -> Result<(), ThemeError> {
    zip.start_file(name, SimpleFileOptions::default())
        .map_err(|e| ThemeError::ZipWrite {
            sink: "reference_docx".into(),
            entry: name.into(),
            message: e.to_string(),
        })?;
    zip.write_all(data).map_err(|e| ThemeError::ZipWrite {
        sink: "reference_docx".into(),
        entry: name.into(),
        message: e.to_string(),
    })
}

fn build_styles_xml(theme: &ResolvedTheme) -> String {
    let serif = primary_typeface(theme.lookup_family("serif"));
    let sans = primary_typeface(theme.lookup_family("sans"));
    let ink = theme
        .lookup_color("ink")
        .or_else(|| theme.lookup_color("surface:ink"))
        .map_or_else(|| "000000".to_owned(), |c| c.body().to_owned());

    let h1_size = theme.lookup_scale("title").unwrap_or(64) * 2;
    let h2_size = theme.lookup_scale("subtitle").unwrap_or(44) * 2;
    let h3_size = theme.lookup_scale("body").unwrap_or(32) * 2;

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{NS_W}">
  <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
    <w:name w:val="normal"/>
    <w:rPr>
      <w:rFonts w:ascii="{sans}" w:hAnsi="{sans}"/>
    </w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:rPr>
      <w:rFonts w:ascii="{serif}" w:hAnsi="{serif}"/>
      <w:color w:val="{ink}"/>
      <w:sz w:val="{h1_size}"/>
    </w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading2">
    <w:name w:val="heading 2"/>
    <w:rPr>
      <w:rFonts w:ascii="{serif}" w:hAnsi="{serif}"/>
      <w:color w:val="{ink}"/>
      <w:sz w:val="{h2_size}"/>
    </w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading3">
    <w:name w:val="heading 3"/>
    <w:rPr>
      <w:rFonts w:ascii="{serif}" w:hAnsi="{serif}"/>
      <w:color w:val="{ink}"/>
      <w:sz w:val="{h3_size}"/>
    </w:rPr>
  </w:style>
  <w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont">
    <w:name w:val="Default Paragraph Font"/>
  </w:style>
</w:styles>"#,
    )
}

fn word_document_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{NS_W}">
  <w:body>
    <w:p/>
  </w:body>
</w:document>"#,
    )
}

fn primary_typeface(family: Option<&[String]>) -> String {
    family
        .and_then(|stack| stack.first().cloned())
        .unwrap_or_else(|| "Calibri".to_owned())
}

static CONTENT_TYPES: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{NS_PKG_CT}">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
  <Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>
</Types>"#,
    )
});

static RELS_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_OFFICE_DOC}" Target="word/document.xml"/>
</Relationships>"#,
    )
});

static WORD_DOCUMENT_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_STYLES}" Target="styles.xml"/>
  <Relationship Id="rId2" Type="{REL_TYPE_SETTINGS}" Target="settings.xml"/>
  <Relationship Id="rId3" Type="{REL_TYPE_THEME}" Target="theme/theme1.xml"/>
</Relationships>"#,
    )
});

static WORD_SETTINGS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="{NS_W}"/>"#,
    )
});

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::io::{Cursor, Read};
    use std::path::PathBuf;

    use zip::ZipArchive;

    use super::*;

    fn summus() -> ResolvedTheme {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes");
        let registry = crate::registry::Registry::load_dir(&dir).expect("load");
        registry
            .resolve(&crate::registry::parse_theme_id("summus").expect("id"))
            .expect("resolve")
    }

    #[test]
    fn docx_produces_non_empty_bytes() {
        let bytes = emit_reference_docx(&summus()).expect("emit");
        assert!(!bytes.is_empty(), "reference.docx must not be empty");
    }

    #[test]
    fn docx_is_valid_zip() {
        let bytes = emit_reference_docx(&summus()).expect("emit");
        assert_eq!(
            bytes.get(..2),
            Some(b"PK".as_slice()),
            "DOCX output must be a valid ZIP"
        );
    }

    #[test]
    fn docx_contains_styles_xml() {
        let bytes = emit_reference_docx(&summus()).expect("emit");
        let cursor = Cursor::new(&bytes);
        let archive = ZipArchive::new(cursor).expect("valid zip");
        let names: Vec<String> = archive.file_names().map(String::from).collect();
        assert!(
            names.contains(&"word/styles.xml".to_owned()),
            "archive must contain word/styles.xml: {names:?}"
        );
    }

    #[test]
    fn docx_styles_xml_carries_body_font() {
        let bytes = emit_reference_docx(&summus()).expect("emit");
        let cursor = Cursor::new(&bytes);
        let mut archive = ZipArchive::new(cursor).expect("valid zip");
        let mut styles = String::new();
        archive
            .by_name("word/styles.xml")
            .expect("styles.xml entry")
            .read_to_string(&mut styles)
            .expect("read styles");
        let sans = summus()
            .lookup_family("sans")
            .and_then(|family| family.first())
            .cloned()
            .expect("sans family");
        assert!(
            styles.contains(&format!(r#"w:ascii="{sans}""#)),
            "styles.xml must carry body sans font {sans}: {styles}"
        );
    }

    #[test]
    fn docx_byte_stable_across_runs() {
        let a = emit_reference_docx(&summus()).expect("first");
        let b = emit_reference_docx(&summus()).expect("second");
        assert_eq!(a, b, "two emissions must match byte-for-byte");
    }
}
