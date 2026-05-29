use poiesis_core::scalar::AspectRatio;

use crate::{Canvas, SlideLayout, Zone, ZoneName};

/// Resolve the default slide layout for a given aspect ratio.
///
/// Builds a canvas and inserts all 9 predefined zones.
#[must_use]
pub fn resolve_layout(aspect: &AspectRatio) -> SlideLayout {
    let canvas = Canvas::from_aspect(aspect);
    let mut zones = std::collections::BTreeMap::new();
    zones.insert(
        ZoneName::Full,
        Zone {
            x: 0.00,
            y: 0.00,
            w: 1.00,
            h: 1.00,
        },
    );
    zones.insert(
        ZoneName::Header,
        Zone {
            x: 0.05,
            y: 0.05,
            w: 0.90,
            h: 0.15,
        },
    );
    zones.insert(
        ZoneName::SubHeader,
        Zone {
            x: 0.05,
            y: 0.22,
            w: 0.90,
            h: 0.10,
        },
    );
    zones.insert(
        ZoneName::Body,
        Zone {
            x: 0.05,
            y: 0.22,
            w: 0.90,
            h: 0.70,
        },
    );
    zones.insert(
        ZoneName::Content,
        Zone {
            x: 0.05,
            y: 0.35,
            w: 0.90,
            h: 0.57,
        },
    );
    zones.insert(
        ZoneName::Footer,
        Zone {
            x: 0.00,
            y: 0.90,
            w: 1.00,
            h: 0.10,
        },
    );
    zones.insert(
        ZoneName::LeftHalf,
        Zone {
            x: 0.05,
            y: 0.22,
            w: 0.44,
            h: 0.70,
        },
    );
    zones.insert(
        ZoneName::RightHalf,
        Zone {
            x: 0.51,
            y: 0.22,
            w: 0.44,
            h: 0.70,
        },
    );
    zones.insert(
        ZoneName::CenterKpi,
        Zone {
            x: 0.25,
            y: 0.30,
            w: 0.50,
            h: 0.40,
        },
    );
    SlideLayout { canvas, zones }
}
