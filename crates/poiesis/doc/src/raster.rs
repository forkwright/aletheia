//! SVG rasterizer for chart figures.

use resvg::tiny_skia;
use resvg::usvg;
use snafu::ResultExt;
use snafu::Snafu;

/// Default rasterization DPI for document figures.
pub(crate) const DEFAULT_DPI: f32 = 192.0;

/// Rasterization errors.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum RasterError {
    /// SVG parsing failed.
    #[snafu(display("invalid SVG: {source}"))]
    ParseSvg {
        /// Underlying SVG parse error.
        source: usvg::Error,
    },

    /// Pixmap allocation failed for the rendered size.
    #[snafu(display("failed to allocate pixmap {width}x{height}"))]
    PixmapAlloc {
        /// Output width in pixels.
        width: u32,
        /// Output height in pixels.
        height: u32,
    },

    /// PNG encoding failed after rendering.
    #[snafu(display("PNG encoding failed: {message}"))]
    EncodePng {
        /// Human-readable PNG encoding error.
        message: String,
    },
}

/// Rasterize an SVG string to PNG bytes.
///
/// # Errors
///
/// Returns [`RasterError`] if the SVG cannot be parsed, rendered, or encoded.
pub(crate) fn svg_to_png(svg: &str, dpi: f32) -> Result<Vec<u8>, RasterError> {
    let opt = usvg::Options {
        dpi,
        ..usvg::Options::default()
    };

    let tree = usvg::Tree::from_str(svg, &opt).context(ParseSvgSnafu)?;
    let size = tree.size().to_int_size();
    let mut pixmap =
        tiny_skia::Pixmap::new(size.width(), size.height()).ok_or(RasterError::PixmapAlloc {
            width: size.width(),
            height: size.height(),
        })?;

    let mut pixmap_mut = pixmap.as_mut();
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap_mut);
    pixmap
        .encode_png()
        .map_err(|source| RasterError::EncodePng {
            message: source.to_string(),
        })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_svg_to_png() {
        let png = svg_to_png(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
                 <rect x="0" y="0" width="10" height="10" fill="#232E54"/>
               </svg>"##,
            DEFAULT_DPI,
        )
        .expect("svg must rasterize");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
