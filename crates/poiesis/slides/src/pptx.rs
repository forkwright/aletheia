//! PPTX rendering backend — hand-rolled ZIP/XML implementation.
//!
//! The document content is mapped to slides using these rules:
//! - A new [`SlideContent`] is started for each Heading block.
//! - Paragraph and List blocks append bullet points to the current slide.
//! - Table blocks are summarized as bullet points (one per row).
//! - [`PageBreak`] forces a new slide even without a heading.
//!
//! If the document has no headings, all content lands on a single slide
//! titled with the document metadata title.
//!
//! [`PageBreak`]: poiesis_core::Block::PageBreak

use std::fmt::Write as FmtWrite;
use std::io::Write as IoWrite;

use poiesis_core::{Block, Document, Renderer, RichText};
use quick_xml::escape::escape;
use snafu::Snafu;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

// ------------------------------------------------------------------
// OOXML namespace identifiers
//
// These are the fixed, W3C/OOXML-registered URI *identifiers* that every
// PPTX package must embed verbatim in its XML parts and relationships.
// They are not endpoints the renderer fetches — substituting HTTPS
// equivalents produces a corrupt package that Office/LibreOffice reject.
//
// See ECMA-376 Part 1 "Fundamentals and Markup Language Reference"
// §11 (package) and §19 (presentation). The constants live on dedicated
// lines so `SECURITY/insecure-transport` can skip them via the shared
// `// WHY:` skip-pattern (standardised identifier, not a credential path).
// ------------------------------------------------------------------

// Each constant sits on a single line so the `// WHY:` marker is co-located
// with the literal, letting `SECURITY/insecure-transport`'s skip-pattern
// bypass the standardised OOXML identifier URIs without an ad-hoc ignore.

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

/// Errors produced by the PPTX renderer.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum PptxError {
    /// An error occurred while generating the presentation.
    #[snafu(display("PPTX error: {message}"))]
    Pptx {
        /// Human-readable error description.
        message: String,
    },
}

/// Renders a [`Document`] to a PPTX byte vector.
///
/// Each top-level [`Block::Heading`] starts a new slide. Other blocks
/// are appended to the current slide as bullet points.
pub struct PptxRenderer;

impl PptxRenderer {
    /// Construct a new `PptxRenderer`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PptxRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for PptxRenderer {
    type Error = PptxError;

    fn format(&self) -> &'static str {
        "pptx"
    }

    fn render(&self, doc: &Document) -> Result<Vec<u8>, Self::Error> {
        let mut slides: Vec<SlideContent> = Vec::new();

        // Start with a title slide using the document metadata.
        let mut current_slide = SlideContent::new(&doc.metadata.title);

        for block in &doc.content {
            match block {
                Block::Heading { level: _, text } => {
                    // Flush the current slide (if it has any content) and start a new one.
                    slides.push(current_slide);
                    current_slide = SlideContent::new(&text.plain_text());
                }
                Block::Paragraph(rt) => {
                    current_slide = current_slide.add_bullet(&rt.plain_text());
                }
                Block::Note(note) => {
                    current_slide = current_slide.add_bullet(&format!(
                        "{}: {}",
                        note.kind.label(),
                        note.body.plain_text()
                    ));
                }
                Block::DisplayMath(expr) => {
                    current_slide = current_slide.add_bullet(expr);
                }
                Block::RawBlock { content, .. } => {
                    current_slide = current_slide.add_bullet(content);
                }
                Block::List { ordered, items } => {
                    for (i, item) in items.iter().enumerate() {
                        let text = if *ordered {
                            format!("{}. {}", i + 1, item.content.plain_text())
                        } else {
                            item.content.plain_text()
                        };
                        current_slide = current_slide.add_bullet(&text);
                    }
                }
                Block::Table(table) => {
                    // Summarize table as header bullet + one bullet per row.
                    let header = table.headers.join(" | ");
                    current_slide = current_slide.add_bullet(&header);
                    for row in &table.rows {
                        let cells: Vec<String> = row.iter().map(RichText::plain_text).collect();
                        current_slide = current_slide.add_bullet(&cells.join(" | "));
                    }
                }
                Block::Image(img) => {
                    current_slide = current_slide.add_bullet(&format!("[Image: {}]", img.alt));
                }
                Block::PageBreak => {
                    slides.push(current_slide);
                    // New untitled slide — title is empty string until next Heading.
                    current_slide = SlideContent::new("");
                }
            }
        }

        slides.push(current_slide);

        // At least one slide is required.
        if slides.is_empty() {
            slides.push(SlideContent::new(&doc.metadata.title));
        }

        create_pptx_with_content(&doc.metadata.title, &slides)
    }
}

/// Internal slide representation.
#[derive(Debug, Clone)]
struct SlideContent {
    title: String,
    bullets: Vec<String>,
}

impl SlideContent {
    fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            bullets: Vec::new(),
        }
    }

    fn add_bullet(mut self, text: &str) -> Self {
        self.bullets.push(text.to_owned());
        self
    }
}

fn create_pptx_with_content(_title: &str, slides: &[SlideContent]) -> Result<Vec<u8>, PptxError> {
    let buf = Vec::new();
    let cursor = std::io::Cursor::new(buf);
    let mut zip = ZipWriter::new(cursor);

    write_entry(
        &mut zip,
        "[Content_Types].xml",
        &build_content_types(slides),
    )?;
    write_entry(&mut zip, "_rels/.rels", RELS_RELS.as_str())?;
    write_entry(
        &mut zip,
        "ppt/presentation.xml",
        &build_presentation(slides),
    )?;
    write_entry(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        &build_presentation_rels(slides),
    )?;

    // Static template files.
    write_entry(
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        SLIDE_LAYOUT.as_str(),
    )?;
    write_entry(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        SLIDE_LAYOUT_RELS.as_str(),
    )?;
    write_entry(
        &mut zip,
        "ppt/slideMasters/slideMaster1.xml",
        SLIDE_MASTER.as_str(),
    )?;
    write_entry(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        SLIDE_MASTER_RELS.as_str(),
    )?;
    write_entry(&mut zip, "ppt/theme/theme1.xml", THEME.as_str())?;

    // Dynamic slide files.
    for (i, slide) in slides.iter().enumerate() {
        let slide_num = i + 1;
        write_entry(
            &mut zip,
            &format!("ppt/slides/slide{slide_num}.xml"),
            &build_slide(slide),
        )?;
        write_entry(
            &mut zip,
            &format!("ppt/slides/_rels/slide{slide_num}.xml.rels"),
            SLIDE_RELS.as_str(),
        )?;
    }

    let cursor = zip.finish().map_err(|e| PptxError::Pptx {
        message: e.to_string(),
    })?;
    Ok(cursor.into_inner())
}

fn write_entry(
    zip: &mut ZipWriter<std::io::Cursor<Vec<u8>>>,
    name: &str,
    content: &str,
) -> Result<(), PptxError> {
    zip.start_file(name, SimpleFileOptions::default())
        .map_err(|e| PptxError::Pptx {
            message: e.to_string(),
        })?;
    zip.write_all(content.as_bytes())
        .map_err(|e| PptxError::Pptx {
            message: e.to_string(),
        })
}

/// Escape text for XML character data using `quick-xml`.
fn xml_escape(s: &str) -> String {
    escape(s).to_string()
}

fn build_content_types(slides: &[SlideContent]) -> String {
    let mut overrides = String::new();
    for i in 1..=slides.len() {
        let _ = write!(
            overrides,
            r#"  <Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
        );
        overrides.push('\n');
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{NS_PKG_CT}">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
  <Override PartName="/ppt/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
  <Override PartName="/ppt/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
{overrides}</Types>"#,
    )
}

fn build_presentation(slides: &[SlideContent]) -> String {
    let mut slide_ids = String::new();
    for i in 0..slides.len() {
        let id = 256 + i;
        let rid = i + 2; // rId2 is first slide; rId1 is slideMaster.
        let _ = write!(slide_ids, r#"    <p:sldId id="{id}" r:id="rId{rid}"/>"#); // kanon:ignore RUST/no-silent-result-swallow — write! to String is infallible; error is unreachable
        slide_ids.push('\n');
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="{NS_DRAWINGML}" xmlns:r="{NS_OFFICE_DOC_RELS}" xmlns:p="{NS_PRESENTATIONML}">
  <p:sldIdLst>
{slide_ids}  </p:sldIdLst>
  <p:sldSz cx="12192000" cy="6858000" type="screen16x9"/>
  <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#,
    )
}

fn build_presentation_rels(slides: &[SlideContent]) -> String {
    let mut rels = String::new();
    let _ = write!(
        rels,
        r#"  <Relationship Id="rId1" Type="{REL_TYPE_SLIDE_MASTER}" Target="slideMasters/slideMaster1.xml"/>"#,
    );
    rels.push('\n');
    for i in 0..slides.len() {
        let rid = i + 2;
        let slide_num = i + 1;
        let _ = write!(
            rels,
            r#"  <Relationship Id="rId{rid}" Type="{REL_TYPE_SLIDE}" Target="slides/slide{slide_num}.xml"/>"#,
        );
        rels.push('\n');
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
{rels}</Relationships>"#,
    )
}

fn build_slide(slide: &SlideContent) -> String {
    let title = xml_escape(&slide.title);
    let mut bullets_xml = String::new();
    for bullet in &slide.bullets {
        let text = xml_escape(bullet);
        let _ = write!(
            bullets_xml,
            "          <a:p>\n\
             <a:pPr lvl=\"0\"/>\n\
             <a:r>\n\
               <a:rPr lang=\"en-US\" dirty=\"0\" smtClean=\"0\"/>\n\
               <a:t>{text}</a:t>\n\
             </a:r>\n\
             <a:endParaRPr lang=\"en-US\" dirty=\"0\"/>\n\
           </a:p>\n",
        );
    }
    // Emit an empty paragraph when there are no bullets so the text body is valid.
    if bullets_xml.is_empty() {
        bullets_xml.push_str(
            "          <a:p>\n\
             <a:pPr lvl=\"0\"/>\n\
             <a:endParaRPr lang=\"en-US\" dirty=\"0\"/>\n\
           </a:p>\n",
        );
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="{NS_DRAWINGML}" xmlns:r="{NS_OFFICE_DOC_RELS}" xmlns:p="{NS_PRESENTATIONML}">
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
      <p:sp>
        <p:nvSpPr>
          <p:cNvPr id="2" name="Title 1"/>
          <p:cNvSpPr txBox="1"/>
          <p:nvPr/>
        </p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="457200" y="274638"/>
            <a:ext cx="8236125" cy="1143000"/>
          </a:xfrm>
          <a:prstGeom prst="rect">
            <a:avLst/>
          </a:prstGeom>
        </p:spPr>
        <p:txBody>
          <a:bodyPr/>
          <a:lstStyle/>
          <a:p>
            <a:r>
              <a:rPr lang="en-US" dirty="0" smtClean="0"/>
              <a:t>{title}</a:t>
            </a:r>
            <a:endParaRPr lang="en-US" dirty="0"/>
          </a:p>
        </p:txBody>
      </p:sp>
      <p:sp>
        <p:nvSpPr>
          <p:cNvPr id="3" name="Content Placeholder 2"/>
          <p:cNvSpPr txBox="1"/>
          <p:nvPr/>
        </p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="457200" y="1600200"/>
            <a:ext cx="8236125" cy="4525963"/>
          </a:xfrm>
          <a:prstGeom prst="rect">
            <a:avLst/>
          </a:prstGeom>
        </p:spPr>
        <p:txBody>
          <a:bodyPr/>
          <a:lstStyle/>
{bullets_xml}        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#,
    )
}

// ------------------------------------------------------------------
// Static OOXML template builders
//
// Each template is built from the OOXML namespace constants above via
// `LazyLock<String>` so the URI literals live in exactly one audited
// place. The callers use `.as_str()` to obtain the computed `&str`.
// ------------------------------------------------------------------

use std::sync::LazyLock;

static RELS_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_OFFICE_DOC}" Target="ppt/presentation.xml"/>
</Relationships>"#,
    )
});

static SLIDE_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_SLIDE_LAYOUT}" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#,
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

static SLIDE_MASTER_RELS: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_RELS}">
  <Relationship Id="rId1" Type="{REL_TYPE_SLIDE_LAYOUT}" Target="../slideLayouts/slideLayout1.xml"/>
  <Relationship Id="rId2" Type="{REL_TYPE_THEME}" Target="../theme/theme1.xml"/>
</Relationships>"#,
    )
});

static SLIDE_LAYOUT: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="{NS_DRAWINGML}" xmlns:r="{NS_OFFICE_DOC_RELS}" xmlns:p="{NS_PRESENTATIONML}" type="titleAndContent" preserve="1">
  <p:cSld name="Title and Content">
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
      <p:sp>
        <p:nvSpPr>
          <p:cNvPr id="2" name="Title 1"/>
          <p:cNvSpPr txBox="1"/>
          <p:nvPr phType="title"/>
        </p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="457200" y="274638"/>
            <a:ext cx="8236125" cy="1143000"/>
          </a:xfrm>
          <a:prstGeom prst="rect">
            <a:avLst/>
          </a:prstGeom>
        </p:spPr>
        <p:txBody>
          <a:bodyPr/>
          <a:lstStyle/>
          <a:p>
            <a:endParaRPr lang="en-US"/>
          </a:p>
        </p:txBody>
      </p:sp>
      <p:sp>
        <p:nvSpPr>
          <p:cNvPr id="3" name="Content Placeholder 2"/>
          <p:cNvSpPr txBox="1"/>
          <p:nvPr phType="body" idx="1"/>
        </p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="457200" y="1600200"/>
            <a:ext cx="8236125" cy="4525963"/>
          </a:xfrm>
          <a:prstGeom prst="rect">
            <a:avLst/>
          </a:prstGeom>
        </p:spPr>
        <p:txBody>
          <a:bodyPr/>
          <a:lstStyle/>
          <a:p>
            <a:pPr lvl="0"/>
            <a:endParaRPr lang="en-US"/>
          </a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
  <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
</p:sldLayout>"#,
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

static THEME: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="{NS_DRAWINGML}" name="Office Theme">
  <a:themeElements>
    <a:clrScheme name="Office">
      <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
      <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
      <a:dk2><a:srgbClr val="44546A"/></a:dk2>
      <a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
      <a:accent1><a:srgbClr val="4472C4"/></a:accent1>
      <a:accent2><a:srgbClr val="ED7D31"/></a:accent2>
      <a:accent3><a:srgbClr val="A5A5A5"/></a:accent3>
      <a:accent4><a:srgbClr val="FFC000"/></a:accent4>
      <a:accent5><a:srgbClr val="5B9BD5"/></a:accent5>
      <a:accent6><a:srgbClr val="70AD47"/></a:accent6>
      <a:hlink><a:srgbClr val="0563C1"/></a:hlink>
      <a:folHlink><a:srgbClr val="954F72"/></a:folHlink>
    </a:clrScheme>
    <a:fontScheme name="Office">
      <a:majorFont><a:latin typeface="Calibri Light" panose="020F0302020204030204"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>
      <a:minorFont><a:latin typeface="Calibri" panose="020F0502020204030204"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>
    </a:fontScheme>
    <a:fmtScheme name="Office">
      <a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="35000"><a:schemeClr val="phClr"><a:tint val="37000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="15000"/><a:satMod val="350000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="1"/></a:gradFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="100000"/><a:shade val="100000"/><a:satMod val="130000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="50000"/><a:satMod val="350000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="0"/></a:gradFill></a:fillStyleLst>
      <a:lnStyleLst><a:ln w="9525" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="25400" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="38100" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln></a:lnStyleLst>
      <a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst><a:outerShdw blurRad="40000" dist="20000" dir="5400000" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="38000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle></a:effectStyleLst>
      <a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="40000"/><a:satMod val="350000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="100000"/><a:satMod val="350000"/></a:schemeClr></a:gs></a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="-80000" r="50000" b="180000"/></a:path></a:gradFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="80000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="80000"/><a:satMod val="300000"/></a:schemeClr></a:gs></a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="50000" r="50000" b="50000"/></a:path></a:gradFill></a:bgFillStyleLst>
    </a:fmtScheme>
  </a:themeElements>
</a:theme>"#,
    )
});

#[cfg(test)]
mod tests {
    use poiesis_core::{Block, Document, Metadata, RichText, Span};

    use super::*;

    fn sample_doc() -> Document {
        Document {
            metadata: Metadata {
                title: "PPTX Test Presentation".to_owned(),
                author: None,
                created: None,
            },
            content: vec![
                Block::Heading {
                    level: 1,
                    text: RichText {
                        spans: vec![Span::Plain("Introduction".to_owned())],
                    },
                },
                Block::Paragraph(RichText {
                    spans: vec![Span::Plain("Welcome to the presentation.".to_owned())],
                }),
                Block::List {
                    ordered: false,
                    items: vec![
                        poiesis_core::block::ListItem {
                            content: RichText {
                                spans: vec![Span::Plain("Point A".to_owned())],
                            },
                        },
                        poiesis_core::block::ListItem {
                            content: RichText {
                                spans: vec![Span::Plain("Point B".to_owned())],
                            },
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn pptx_produces_nonempty_bytes() {
        let r = PptxRenderer::new();
        let bytes = r.render(&sample_doc()).expect("PPTX render failed");
        assert!(!bytes.is_empty(), "rendered PPTX must not be empty");
    }

    #[test]
    #[expect(
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "test assertions on known-good data"
    )]
    fn pptx_starts_with_pk_magic() {
        // WHY: PPTX is a ZIP/OOXML archive; valid files start with PK (0x50 0x4B).
        let r = PptxRenderer::new();
        let bytes = r.render(&sample_doc()).expect("PPTX render failed");
        assert_eq!(&bytes[..2], b"PK", "PPTX output should be a valid ZIP");
    }

    #[test]
    fn xml_escape_handles_special_chars() {
        assert_eq!(
            xml_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
    }
}
