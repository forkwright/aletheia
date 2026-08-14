use std::collections::BTreeMap;

use poiesis_core::scalar::AspectRatio;
use serde::{Deserialize, Serialize};

/// Normalized zone coordinates in [0, 1].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    /// Normalized x position.
    pub x: f64,
    /// Normalized y position.
    pub y: f64,
    /// Normalized width.
    pub w: f64,
    /// Normalized height.
    pub h: f64,
}

/// Named zone on a slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneName {
    /// Full slide.
    Full,
    /// Top header area.
    Header,
    /// Sub-header below header.
    SubHeader,
    /// Main body area.
    Body,
    /// Content area.
    Content,
    /// Bottom footer.
    Footer,
    /// Left half split.
    LeftHalf,
    /// Right half split.
    RightHalf,
    /// Center KPI card.
    CenterKpi,
}

impl ZoneName {
    /// CSS class for this zone.
    #[must_use]
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Full => "zone-full",
            Self::Header => "zone-header",
            Self::SubHeader => "zone-subheader",
            Self::Body => "zone-body",
            Self::Content => "zone-content",
            Self::Footer => "zone-footer",
            Self::LeftHalf => "zone-left-half",
            Self::RightHalf => "zone-right-half",
            Self::CenterKpi => "zone-center-kpi",
        }
    }
}

/// Canvas dimensions in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Canvas {
    /// Width in pixels.
    pub width_px: u32,
    /// Height in pixels.
    pub height_px: u32,
}

impl Canvas {
    /// Build a canvas from an aspect ratio.
    ///
    /// - 16:9 → 1280×720
    /// - 4:3 → 1024×768
    #[must_use]
    pub fn from_aspect(aspect: &AspectRatio) -> Self {
        match (aspect.width(), aspect.height()) {
            (16, 9) => Self {
                width_px: 1280,
                height_px: 720,
            },
            (4, 3) => Self {
                width_px: 1024,
                height_px: 768,
            },
            _ => Self {
                width_px: u32::from(aspect.width()) * 80,
                height_px: u32::from(aspect.height()) * 80,
            },
        }
    }
}

/// The complete layout of one slide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlideLayout {
    /// Canvas dimensions.
    pub canvas: Canvas,
    /// Named zones in display order.
    pub zones: BTreeMap<ZoneName, Zone>,
}
