use std::fmt::Write;

use poiesis_deck_layout::zone_to_css;

use crate::SlideLayout;

/// Generate the three-layer CSS stylesheet for a deck.
#[must_use]
pub(crate) fn three_layer_css(layout: &SlideLayout) -> String {
    let mut css = String::new();

    // Layer 1: box-sizing reset + deck/slide structure
    css.push_str("*, *::before, *::after { box-sizing: border-box; }\n");
    css.push_str(".deck { display: flex; flex-direction: column; }\n");
    let w = layout.canvas.width_px;
    let h = layout.canvas.height_px;
    let _ = writeln!(
        css,
        ".slide {{ position: relative; overflow: hidden; page-break-after: always; width: {w}px; height: {h}px; }}"
    );

    // Layer 2: CSS variables
    css.push_str(":root {\n");
    css.push_str("  --theme-primary: #1a56db;\n");
    css.push_str("  --theme-secondary: #f3f4f6;\n");
    css.push_str("  --theme-text: #111827;\n");
    css.push_str("  --theme-accent: #059669;\n");
    css.push_str("  --theme-bg: #ffffff;\n");
    css.push_str("  --font-heading: 'Inter', sans-serif;\n");
    css.push_str("  --font-body: 'Inter', sans-serif;\n");
    css.push_str("}\n");

    // Layer 3: zone positioning
    for (name, zone) in &layout.zones {
        let class = name.css_class();
        let style = zone_to_css(zone, &layout.canvas);
        let _ = writeln!(css, ".{class} {{ position: absolute; {style} }}");
    }

    css
}
