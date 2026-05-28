//! Canvas geometry: outer viewBox + inner plot box.
//!
//! Two canvases are first-class because two pixel-density regimes exist:
//! [`DeckCanvas`] is the deck stage (1600×540 viewBox, `preserveAspectRatio=none`,
//! stretched to fit the slide), [`DocCanvas`] is a document figure (intrinsic
//! viewBox per writer, `xMidYMid meet` to preserve aspect when scaled).
//!
//! The inner plot box is computed from canvas-specific margins. Per-kind
//! emitter arms consume the plot box, not the raw viewBox.

/// Either canvas, named by the target surface.
#[derive(Debug, Clone)]
pub enum Canvas {
    /// Deck stage canvas.
    Deck(DeckCanvas),
    /// Document figure canvas.
    Doc(DocCanvas),
}

/// Deck stage canvas: 1600×540 viewBox.
///
/// The pixel choice matches the offsite slide-3 chart: 1600 wide gives crisp
/// text at typical 16:9 slide rendering, 540 keeps the chart in the lower
/// two-thirds of the slide. Inner margins reserve space for x-tick labels
/// (bottom) and dual y-axis labels (left + right).
#[derive(Debug, Clone, Copy)]
pub struct DeckCanvas {
    /// viewBox width.
    pub width: u32,
    /// viewBox height.
    pub height: u32,
    /// Left margin (y-left tick labels).
    pub margin_left: u32,
    /// Right margin (y-right tick labels for combo).
    pub margin_right: u32,
    /// Top margin (title + legend headroom).
    pub margin_top: u32,
    /// Bottom margin (x-tick labels).
    pub margin_bottom: u32,
}

impl Default for DeckCanvas {
    fn default() -> Self {
        Self {
            width: 1600,
            height: 540,
            margin_left: 80,
            margin_right: 80,
            margin_top: 60,
            margin_bottom: 80,
        }
    }
}

/// Document figure canvas (intrinsic box per writer).
///
/// Defaults to 640×360 — a typical figure aspect that survives both
/// half-page (krilla PDF, Typst) and full-page (DOCX, ODT) embedding.
#[derive(Debug, Clone, Copy)]
pub struct DocCanvas {
    /// viewBox width.
    pub width: u32,
    /// viewBox height.
    pub height: u32,
    /// Left margin.
    pub margin_left: u32,
    /// Right margin.
    pub margin_right: u32,
    /// Top margin.
    pub margin_top: u32,
    /// Bottom margin.
    pub margin_bottom: u32,
}

impl Default for DocCanvas {
    fn default() -> Self {
        Self {
            width: 640,
            height: 360,
            margin_left: 60,
            margin_right: 60,
            margin_top: 40,
            margin_bottom: 60,
        }
    }
}

/// Inner plot box (x0, y0, x1, y1) in viewBox pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotBox {
    /// Left edge.
    pub x0: f64,
    /// Top edge.
    pub y0: f64,
    /// Right edge.
    pub x1: f64,
    /// Bottom edge.
    pub y1: f64,
}

impl PlotBox {
    /// Plot-box width.
    #[must_use]
    pub const fn width(&self) -> f64 {
        self.x1 - self.x0
    }
    /// Plot-box height.
    #[must_use]
    pub const fn height(&self) -> f64 {
        self.y1 - self.y0
    }
}

impl Canvas {
    /// viewBox width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        match self {
            Self::Deck(c) => c.width,
            Self::Doc(c) => c.width,
        }
    }
    /// viewBox height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        match self {
            Self::Deck(c) => c.height,
            Self::Doc(c) => c.height,
        }
    }
    /// `preserveAspectRatio` attribute value.
    #[must_use]
    pub const fn preserve_aspect_ratio(&self) -> &'static str {
        match self {
            Self::Deck(_) => "none",
            Self::Doc(_) => "xMidYMid meet",
        }
    }
    /// Inner plot box computed from the canvas margins.
    #[must_use]
    pub fn plot_box(&self) -> PlotBox {
        match self {
            Self::Deck(c) => PlotBox {
                x0: f64::from(c.margin_left),
                y0: f64::from(c.margin_top),
                x1: f64::from(c.width - c.margin_right),
                y1: f64::from(c.height - c.margin_bottom),
            },
            Self::Doc(c) => PlotBox {
                x0: f64::from(c.margin_left),
                y0: f64::from(c.margin_top),
                x1: f64::from(c.width - c.margin_right),
                y1: f64::from(c.height - c.margin_bottom),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_canvas_default_is_offsite_geometry() {
        let c = Canvas::Deck(DeckCanvas::default());
        assert_eq!(c.width(), 1600);
        assert_eq!(c.height(), 540);
        assert_eq!(c.preserve_aspect_ratio(), "none");
    }

    #[test]
    fn deck_plot_box_subtracts_margins() {
        let c = Canvas::Deck(DeckCanvas::default());
        let p = c.plot_box();
        assert!((p.x0 - 80.0).abs() < 1e-9);
        assert!((p.x1 - 1520.0).abs() < 1e-9);
        assert!((p.y0 - 60.0).abs() < 1e-9);
        assert!((p.y1 - 460.0).abs() < 1e-9);
        assert!((p.width() - 1440.0).abs() < 1e-9);
        assert!((p.height() - 400.0).abs() < 1e-9);
    }
}
