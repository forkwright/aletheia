//! Base-PPTX sink — emit a minimal, valid OOXML ZIP with the theme baked in.
//!
//! Downstream renders unpack this template and overlay slide content onto it.

use std::io::Write as IoWrite;
use std::sync::LazyLock;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::error::ThemeError;
use crate::resolved::ResolvedTheme;

// WHY: the constants below are the fixed ECMA-376 OOXML URI *identifiers*
// every PPTX package must embed verbatim — not endpoints; substituting HTTPS
// equivalents produces a corrupt package. Each sits on one line so its EOL
// `// WHY:` marker hits `SECURITY/insecure-transport`'s skip-pattern.

/// OOXML content-types namespace.
const NS_PKG_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML package relationships namespace.
const NS_PKG_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML `officeDocument` relationship type.
const REL_TYPE_OFFICE_DOC: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML `slide` relationship type.
const REL_TYPE_SLIDE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML `slideMaster` relationship type.
const REL_TYPE_SLIDE_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML `slideLayout` relationship type.
const REL_TYPE_SLIDE_LAYOUT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML `theme` relationship type.
const REL_TYPE_THEME: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML `DrawingML` namespace (`a:` prefix).
const NS_DRAWINGML: &str = "http://schemas.openxmlformats.org/drawingml/2006/main"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML `PresentationML` namespace (`p:` prefix).
const NS_PRESENTATIONML: &str = "http://schemas.openxmlformats.org/presentationml/2006/main"; // WHY: standardised OOXML identifier URI, not an endpoint
/// OOXML officeDocument relationships namespace (`r:` prefix).
const NS_OFFICE_DOC_RELS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships"; // WHY: standardised OOXML identifier URI, not an endpoint

/// Emit a minimal, valid PPTX ZIP byte vector — a "base template" file —
/// with the [`ResolvedTheme`]'s color and font scheme baked into
/// `ppt/theme/theme1.xml`.
///
/// # Errors
///
/// Returns [`ThemeError::ZipWrite`] if any ZIP entry fails to write.
pub fn emit_base_pptx(theme: &ResolvedTheme) -> Result<Vec<u8>, ThemeError> {
    let buf = Vec::new();
    let cursor = std::io::Cursor::new(buf);
    let mut zip = ZipWriter::new(cursor);

    pack_entry(&mut zip, "[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
    pack_entry(&mut zip, "_rels/.rels", RELS_RELS.as_bytes())?;
    pack_entry(&mut zip, "ppt/presentation.xml", PRESENTATION.as_bytes())?;
    pack_entry(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        PRESENTATION_RELS.as_bytes(),
    )?;
    pack_entry(
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        SLIDE_LAYOUT.as_bytes(),
    )?;
    pack_entry(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        SLIDE_LAYOUT_RELS.as_bytes(),
    )?;
    pack_entry(
        &mut zip,
        "ppt/slideMasters/slideMaster1.xml",
        SLIDE_MASTER.as_bytes(),
    )?;
    pack_entry(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        SLIDE_MASTER_RELS.as_bytes(),
    )?;

    let theme_xml = crate::sinks::ooxml::emit_theme_xml(theme)?;
    pack_entry(&mut zip, "ppt/theme/theme1.xml", theme_xml.as_bytes())?;

    pack_entry(&mut zip, "ppt/slides/slide1.xml", SLIDE1.as_bytes())?;
    pack_entry(
        &mut zip,
        "ppt/slides/_rels/slide1.xml.rels",
        SLIDE1_RELS.as_bytes(),
    )?;

    let cursor = zip.finish().map_err(|e| ThemeError::ZipWrite {
        sink: "pptx".into(),
        entry: "(finish)".into(),
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
            sink: "pptx".into(),
            entry: name.into(),
            message: e.to_string(),
        })?;
    zip.write_all(data).map_err(|e| ThemeError::ZipWrite {
        sink: "pptx".into(),
        entry: name.into(),
        message: e.to_string(),
    })
}

// ── Static OOXML templates ──

static CONTENT_TYPES: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{NS_PKG_CT}">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
  <Override PartName="/ppt/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
  <Override PartName="/ppt/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
  <Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
</Types>"#,
    )
});

static RELS_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_OFFICE_DOC}" Target="ppt/presentation.xml"/>
</Relationships>"#,
    )
});

static PRESENTATION: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="{NS_DRAWINGML}" xmlns:r="{NS_OFFICE_DOC_RELS}" xmlns:p="{NS_PRESENTATIONML}">
  <p:sldIdLst>
    <p:sldId id="256" r:id="rId2"/>
  </p:sldIdLst>
  <p:sldSz cx="12192000" cy="6858000" type="screen16x9"/>
  <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#,
    )
});

static PRESENTATION_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_SLIDE_MASTER}" Target="slideMasters/slideMaster1.xml"/>
  <Relationship Id="rId2" Type="{REL_TYPE_SLIDE}" Target="slides/slide1.xml"/>
</Relationships>"#,
    )
});

static SLIDE_LAYOUT: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="{NS_DRAWINGML}" xmlns:r="{NS_OFFICE_DOC_RELS}" xmlns:p="{NS_PRESENTATIONML}" type="blank" preserve="1">
  <p:cSld name="Blank">
    <p:spTree>
      <p:nvGrpSpPr>
        <p:cNvPr id="1" name=""/>
        <p:cNvGrpSpPr/>
        <p:nvPr/>
      </p:nvGrpSpPr>
      <p:grpSpPr>
        <a:xfrm>
          <a:off x="0" y="0"/>
          <a:ext cx="0" cy="0"/>
          <a:chOff x="0" y="0"/>
          <a:chExt cx="0" cy="0"/>
        </a:xfrm>
      </p:grpSpPr>
    </p:spTree>
  </p:cSld>
  <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
</p:sldLayout>"#,
    )
});

static SLIDE_LAYOUT_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_SLIDE_MASTER}" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#,
    )
});

static SLIDE_MASTER: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="{NS_DRAWINGML}" xmlns:r="{NS_OFFICE_DOC_RELS}" xmlns:p="{NS_PRESENTATIONML}">
  <p:cSld>
    <p:bg>
      <p:bgRef idx="1001">
        <a:schemeClr val="bg1"/>
      </p:bgRef>
    </p:bg>
    <p:spTree>
      <p:nvGrpSpPr>
        <p:cNvPr id="1" name=""/>
        <p:cNvGrpSpPr/>
        <p:nvPr/>
      </p:nvGrpSpPr>
      <p:grpSpPr>
        <a:xfrm>
          <a:off x="0" y="0"/>
          <a:ext cx="0" cy="0"/>
          <a:chOff x="0" y="0"/>
          <a:chExt cx="0" cy="0"/>
        </a:xfrm>
      </p:grpSpPr>
    </p:spTree>
  </p:cSld>
  <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
  <p:txStyles>
    <p:titleStyle>
      <a:defPPr>
        <a:defRPr lang="en-US"/>
      </a:defPPr>
    </p:titleStyle>
    <p:bodyStyle>
      <a:defPPr>
        <a:defRPr lang="en-US"/>
      </a:defPPr>
    </p:bodyStyle>
    <p:otherStyle>
      <a:defPPr>
        <a:defRPr lang="en-US"/>
      </a:defPPr>
    </p:otherStyle>
  </p:txStyles>
</p:sldMaster>"#,
    )
});

static SLIDE_MASTER_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_SLIDE_LAYOUT}" Target="../slideLayouts/slideLayout1.xml"/>
  <Relationship Id="rId2" Type="{REL_TYPE_THEME}" Target="../theme/theme1.xml"/>
</Relationships>"#,
    )
});

static SLIDE1: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="{NS_DRAWINGML}" xmlns:r="{NS_OFFICE_DOC_RELS}" xmlns:p="{NS_PRESENTATIONML}">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr>
        <p:cNvPr id="1" name=""/>
        <p:cNvGrpSpPr/>
        <p:nvPr/>
      </p:nvGrpSpPr>
      <p:grpSpPr>
        <a:xfrm>
          <a:off x="0" y="0"/>
          <a:ext cx="0" cy="0"/>
          <a:chOff x="0" y="0"/>
          <a:chExt cx="0" cy="0"/>
        </a:xfrm>
      </p:grpSpPr>
    </p:spTree>
  </p:cSld>
</p:sld>"#,
    )
});

static SLIDE1_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_SLIDE_LAYOUT}" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#,
    )
});

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::registry::Registry;

    fn summus() -> ResolvedTheme {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes");
        let registry = Registry::load_dir(&dir).expect("load summus");
        registry
            .resolve(&crate::registry::parse_theme_id("summus").expect("id"))
            .expect("resolve")
    }

    #[test]
    fn pptx_produces_non_empty_bytes() {
        let bytes = emit_base_pptx(&summus()).expect("emit base pptx");
        assert!(!bytes.is_empty(), "rendered PPTX must not be empty");
    }

    #[test]
    fn pptx_is_valid_zip() {
        let bytes = emit_base_pptx(&summus()).expect("emit base pptx");
        let cursor = std::io::Cursor::new(bytes);
        let archive = zip::ZipArchive::new(cursor).expect("valid zip archive");
        assert!(!archive.is_empty(), "archive must contain entries");
    }

    #[test]
    fn pptx_contains_theme1_xml() {
        let bytes = emit_base_pptx(&summus()).expect("emit base pptx");
        let cursor = std::io::Cursor::new(bytes);
        let archive = zip::ZipArchive::new(cursor).expect("valid zip archive");
        assert!(
            archive.file_names().any(|n| n == "ppt/theme/theme1.xml"),
            "archive must contain ppt/theme/theme1.xml"
        );
    }

    #[test]
    fn pptx_theme1_xml_carries_brand_colors() {
        let bytes = emit_base_pptx(&summus()).expect("emit base pptx");
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("valid zip archive");
        let mut theme1 = archive
            .by_name("ppt/theme/theme1.xml")
            .expect("theme1.xml entry");
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut theme1, &mut contents)
            .expect("read theme1.xml as string");
        assert!(
            contents.contains("232E54"),
            "theme1.xml must contain summus navy hex (232E54): {contents}"
        );
    }

    #[test]
    fn pptx_byte_stable_across_runs() {
        let a = emit_base_pptx(&summus()).expect("first");
        let b = emit_base_pptx(&summus()).expect("second");
        assert_eq!(a, b, "two emissions must match byte-for-byte");
    }
}
