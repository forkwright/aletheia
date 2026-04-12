//! PDF rendering backend using the `krilla` crate.
//!
//! `PdfRenderer` traverses the document tree and produces a single PDF byte
//! payload. Pages are A4 (595 × 842 pt). Text is laid out top-to-bottom with
//! simple line wrapping. A system font is required; if none is found the
//! renderer returns [`PdfError::NoFont`].

use krilla::Document as KrillaDocument;
use krilla::geom::Point;
use krilla::page::PageSettings;
use krilla::text::{Font, TextDirection};

use poiesis_core::{Block, Document, RichText, Renderer};
use snafu::Snafu;

/// A4 page width in PDF points (1 pt = 1/72 inch).
const PAGE_W: f32 = 595.0;
/// A4 page height in PDF points.
const PAGE_H: f32 = 842.0;
/// Left/right page margin in points.
const MARGIN_X: f32 = 56.0;
/// Top/bottom page margin in points.
const MARGIN_TOP: f32 = 56.0;
/// Default body font size in points.
const FONT_SIZE_BODY: f32 = 11.0;
/// H1 font size in points.
const FONT_SIZE_H1: f32 = 20.0;
/// H2 font size in points.
const FONT_SIZE_H2: f32 = 16.0;
/// Leading (line height) multiplier applied to font size.
const LEADING: f32 = 1.4;
/// Paragraph spacing below a paragraph block in points.
const PARA_SPACING: f32 = 6.0;
/// Spacing above/below a heading in points.
const HEAD_SPACING: f32 = 10.0;
/// Usable page width between margins.
const USABLE_W: f32 = PAGE_W - 2.0 * MARGIN_X;

/// Errors produced by the PDF renderer.
#[derive(Debug, Snafu)]
pub enum PdfError {
    /// No usable system font was found. Install a font (e.g. `liberation-sans-fonts`
    /// or `google-noto-sans-fonts`) and ensure it is readable.
    #[snafu(display("no usable system font found for PDF rendering"))]
    NoFont,

    /// `krilla` rejected the page dimensions. A4 constants are compile-time
    /// checked, so this variant is not reachable in practice.
    #[snafu(display("invalid page dimensions: {width}x{height}"))]
    InvalidPageSize {
        /// Attempted width in PDF points.
        width: f32,
        /// Attempted height in PDF points.
        height: f32,
    },

    /// `krilla` returned an error while producing the PDF.
    #[snafu(display("krilla PDF error: {message}"))]
    Krilla {
        /// Human-readable krilla error description.
        message: String,
    },
}

/// Renders a [`Document`] to a PDF byte vector using the `krilla` crate.
///
/// Requires at least one OpenType/TrueType font to be available at the paths
/// checked by [`PdfRenderer::font_data`]. On Fedora the `liberation-sans-fonts`
/// or `google-noto-sans-fonts` package satisfies this requirement.
pub struct PdfRenderer {
    /// Raw font bytes. Loaded once at construction time.
    font_data: Vec<u8>,
}

impl PdfRenderer {
    /// Try to construct a renderer using a discovered system font.
    ///
    /// # Errors
    ///
    /// Returns [`PdfError::NoFont`] if no readable font file is found.
    pub fn new() -> Result<Self, PdfError> {
        let data = Self::load_system_font().ok_or(PdfError::NoFont)?;
        Ok(Self { font_data: data })
    }

    /// Construct a renderer using caller-supplied raw font bytes.
    ///
    /// Use this when you want full control over the font (e.g. in tests).
    pub fn with_font_data(data: Vec<u8>) -> Self {
        Self { font_data: data }
    }

    /// Attempt to load a font from well-known system locations.
    fn load_system_font() -> Option<Vec<u8>> {
        let candidates = [
            // Fedora: liberation-sans-fonts
            "/usr/share/fonts/liberation-sans-fonts/LiberationSans-Regular.ttf",
            // Fedora: google-noto-vf
            "/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf",
            // Debian/Ubuntu
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            // macOS
            "/System/Library/Fonts/Helvetica.ttc",
            // Common Linux fallback
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        ];

        candidates.iter().find_map(|path| {
            std::fs::read(path)
                .ok()
                .filter(|data| !data.is_empty())
        })
    }
}

/// Produce an A4 `PageSettings`, returning an error if the constants are invalid.
fn a4_page() -> Result<PageSettings, PdfError> {
    PageSettings::from_wh(PAGE_W, PAGE_H).ok_or(PdfError::InvalidPageSize {
        width: PAGE_W,
        height: PAGE_H,
    })
}

impl Renderer for PdfRenderer {
    type Error = PdfError;

    fn format(&self) -> &'static str {
        "pdf"
    }

    #[allow(clippy::too_many_lines)]
    fn render(&self, doc: &Document) -> Result<Vec<u8>, Self::Error> {
        let font = Font::new(
            self.font_data.clone().into(),
            0,
        )
        .ok_or(PdfError::NoFont)?;

        let mut krilla_doc = KrillaDocument::new();

        let mut page = krilla_doc.start_page_with(a4_page()?);
        let mut surface = page.surface();

        // Cursor: y position from the top of the page content area.
        let mut y = MARGIN_TOP;
        let x = MARGIN_X;

        // Draw document title as a pseudo-H1 if the title is non-empty.
        let title_text = doc.metadata.title.as_str();
        if !title_text.is_empty() {
            surface.draw_text(
                Point::from_xy(x, y + FONT_SIZE_H1),
                font.clone(),
                FONT_SIZE_H1,
                title_text,
                false,
                TextDirection::Auto,
            );
            y += FONT_SIZE_H1 * LEADING + HEAD_SPACING;
        }

        for block in &doc.content {
            // Check if we need a new page.
            if y > PAGE_H - MARGIN_TOP {
                surface.finish();
                page.finish();
                page = krilla_doc.start_page_with(a4_page()?);
                surface = page.surface();
                y = MARGIN_TOP;
            }

            match block {
                Block::Heading { level, text } => {
                    y += HEAD_SPACING;
                    let fs = heading_font_size(*level);
                    let plain = text.plain_text();
                    surface.draw_text(
                        Point::from_xy(x, y + fs),
                        font.clone(),
                        fs,
                        &plain,
                        false,
                        TextDirection::Auto,
                    );
                    y += fs * LEADING + HEAD_SPACING;
                }
                Block::Paragraph(rt) => {
                    y = draw_wrapped(
                        &mut surface,
                        &font,
                        FONT_SIZE_BODY,
                        &rt.plain_text(),
                        x,
                        y,
                        USABLE_W,
                    );
                    y += PARA_SPACING;
                }
                Block::List { ordered, items } => {
                    for (i, item) in items.iter().enumerate() {
                        let bullet = if *ordered {
                            format!("{}. ", i + 1)
                        } else {
                            "\u{2022} ".to_owned()
                        };
                        let text = format!("{}{}", bullet, item.content.plain_text());
                        y = draw_wrapped(
                            &mut surface,
                            &font,
                            FONT_SIZE_BODY,
                            &text,
                            x,
                            y,
                            USABLE_W,
                        );
                    }
                    y += PARA_SPACING;
                }
                Block::Table(table) => {
                    // Render table as header row + data rows, plain text.
                    let header_line = table.headers.join(" | ");
                    y = draw_wrapped(
                        &mut surface,
                        &font,
                        FONT_SIZE_BODY,
                        &header_line,
                        x,
                        y,
                        USABLE_W,
                    );
                    // Visual separator gap after header.
                    y += 4.0;
                    for row in &table.rows {
                        let cells: Vec<String> = row.iter().map(RichText::plain_text).collect();
                        let row_line = cells.join(" | ");
                        y = draw_wrapped(
                            &mut surface,
                            &font,
                            FONT_SIZE_BODY,
                            &row_line,
                            x,
                            y,
                            USABLE_W,
                        );
                    }
                    y += PARA_SPACING;
                }
                Block::Image(img) => {
                    // Images are not rendered inline — emit the alt text instead.
                    let alt = format!("[Image: {}]", img.alt);
                    y = draw_wrapped(
                        &mut surface,
                        &font,
                        FONT_SIZE_BODY,
                        &alt,
                        x,
                        y,
                        USABLE_W,
                    );
                    y += PARA_SPACING;
                }
                Block::PageBreak => {
                    surface.finish();
                    page.finish();
                    page = krilla_doc.start_page_with(a4_page()?);
                    surface = page.surface();
                    y = MARGIN_TOP;
                }
            }
        }

        surface.finish();
        page.finish();

        krilla_doc.finish().map_err(|e| PdfError::Krilla {
            message: format!("{e:?}"),
        })
    }
}

/// Map heading level (1–6) to font size in points.
fn heading_font_size(level: u8) -> f32 {
    match level {
        1 => FONT_SIZE_H1,
        2 => FONT_SIZE_H2,
        3 => 14.0,
        4 => 12.0,
        _ => FONT_SIZE_BODY,
    }
}

/// Draw `text` word-wrapped within `max_width` points, returning the new y cursor.
///
/// The y cursor on entry points to the top of where the first line should start.
/// Each line advances by `font_size * LEADING`.
#[allow(clippy::too_many_arguments)]
fn draw_wrapped(
    surface: &mut krilla::surface::Surface<'_>,
    font: &Font,
    font_size: f32,
    text: &str,
    x: f32,
    mut y: f32,
    max_width: f32,
) -> f32 {
    // Approximate character width: krilla does not expose advance widths without
    // shaping, so we use an empirical factor for the font at the given size.
    // Liberation Sans / Noto Sans at 11pt: ~6 pt per average character.
    let char_w_approx = font_size * 0.55;
    // WHY: safe cast — max_width and char_w_approx are positive finite f32 values
    // produced from compile-time page constants; the ratio fits comfortably in usize.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::as_conversions)]
    let chars_per_line = (max_width / char_w_approx).max(1.0) as usize;

    let line_h = font_size * LEADING;
    let baseline_offset = font_size; // PDF baseline is font_size below y

    for line in wrap_words(text, chars_per_line) {
        surface.draw_text(
            Point::from_xy(x, y + baseline_offset),
            font.clone(),
            font_size,
            &line,
            false,
            TextDirection::Auto,
        );
        y += line_h;
    }
    y
}

/// Split `text` into lines no longer than `max_chars` characters,
/// breaking at word boundaries.
fn wrap_words(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= max_chars {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current.clear();
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use poiesis_core::{Block, Document, Metadata, RichText, Span};

    fn minimal_doc() -> Document {
        Document {
            metadata: Metadata {
                title: "PDF Test".to_owned(),
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
                    spans: vec![Span::Plain(
                        "This is a short paragraph of body text.".to_owned(),
                    )],
                }),
            ],
        }
    }

    #[test]
    fn wrap_words_short() {
        let lines = wrap_words("hello world", 20);
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn wrap_words_splits() {
        let lines = wrap_words("one two three four five six", 10);
        for line in &lines {
            assert!(
                line.len() <= 10,
                "line too long: {line:?} ({} chars)",
                line.len()
            );
        }
    }

    #[test]
    fn wrap_words_empty() {
        let lines = wrap_words("", 80);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].is_empty());
    }

    #[test]
    fn pdf_renderer_produces_nonempty_bytes() {
        // WHY: skips if no system font is installed (CI may not have one).
        let renderer = match PdfRenderer::new() {
            Ok(r) => r,
            Err(PdfError::NoFont) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };
        let doc = minimal_doc();
        let bytes = renderer.render(&doc).expect("PDF rendering failed");
        assert!(!bytes.is_empty(), "rendered PDF must not be empty");
        assert!(
            bytes.starts_with(b"%PDF-"),
            "output should start with PDF magic"
        );
    }
}
