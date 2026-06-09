//! Figure helpers for Pandoc document export.

use poiesis_charts::render::{Canvas, DocCanvas};
use poiesis_charts::{
    Chart, ColorMode as ChartColorMode, ResolvedTheme as ChartResolvedTheme, render_chart,
};
use poiesis_core::Image;
use snafu::ResultExt;
use snafu::Snafu;

/// Stable figure identifier prefix.
pub(crate) fn figure_id(index: usize) -> String {
    format!("apx-figure-{index}")
}

/// Figure payload parsing and chart rendering errors.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum FigureError {
    /// The embedded figure bytes were not valid UTF-8.
    #[snafu(display("figure SVG is not valid UTF-8: {source}"))]
    Utf8 {
        /// Underlying UTF-8 error.
        source: std::str::Utf8Error,
    },

    /// The embedded figure bytes were not valid JSON.
    #[snafu(display("figure chart JSON is invalid: {source}"))]
    Json {
        /// Underlying JSON parse error.
        source: serde_json::Error,
    },

    /// The figure MIME type is not supported by the chart path.
    #[snafu(display("unsupported figure MIME type: {mime}"))]
    UnsupportedMime {
        /// Observed MIME type.
        mime: String,
    },

    /// Chart rendering failed.
    #[snafu(display("chart render failed: {source}"))]
    ChartRender {
        /// Underlying chart renderer error.
        source: poiesis_charts::Error,
    },
}

/// Turn a document image payload into SVG bytes.
///
/// Supported figure payloads:
/// - `image/svg+xml` or raw `<svg ...>` bytes: treated as pre-rendered SVG.
/// - `application/json` or `application/vnd.poiesis.chart+json`: parsed as a
///   `poiesis_charts::Chart` and rendered to SVG with the resolved `summus`
///   palette.
///
/// # Errors
///
/// Returns [`FigureError`] if the payload is malformed or unsupported.
pub(crate) fn svg_from_image(image: &Image) -> Result<String, FigureError> {
    let mime = image.mime.trim();
    let is_svg = mime == "image/svg+xml" || mime.contains("svg");
    if is_svg || image.data.starts_with(b"<svg") {
        return std::str::from_utf8(&image.data)
            .map(std::borrow::ToOwned::to_owned)
            .context(Utf8Snafu);
    }

    let is_chart_json =
        mime.contains("json") || mime.contains("chart") || image.data.starts_with(b"{");
    if is_chart_json {
        let chart: Chart = serde_json::from_slice(&image.data).context(JsonSnafu)?;
        let theme = ChartResolvedTheme::summus_stub();
        return render_chart(
            &chart,
            &theme,
            &Canvas::Doc(DocCanvas::default()),
            ChartColorMode::Resolved,
        )
        .context(ChartRenderSnafu);
    }

    UnsupportedMimeSnafu {
        mime: mime.to_owned(),
    }
    .fail()
}
