use std::collections::HashMap;

use poiesis_core::bodies::{Deck, Slide};
use poiesis_core::components::ComponentRegistry;
use poiesis_core::envelope::Meta;
use poiesis_core::scalar::AspectRatio;
use poiesis_deck_layout::SlideLayout;
use serde_json::Value;
use snafu::ResultExt;

use crate::css::three_layer_css;
use crate::error::{ComponentNotFoundSnafu, DeckError, TemplateLoadSnafu, TemplateRenderSnafu};

/// Minimal HTML-escape for text content.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render a complete deck to a standalone HTML string.
pub(crate) fn render_deck(
    registry: &ComponentRegistry,
    layout: &SlideLayout,
    deck: &Deck,
    meta: &Meta,
) -> Result<String, DeckError> {
    let css = three_layer_css(layout);
    let title_escaped = html_escape(&meta.title);
    let deck_class = if deck.aspect == AspectRatio::WIDESCREEN_16_9 {
        "deck deck--16-9"
    } else {
        "deck deck--4-3"
    };

    let mut slides_html = String::new();
    let total = deck.slides.len();

    for (idx, slide) in deck.slides.iter().enumerate() {
        let slide_markup = render_slide(registry, slide, idx, total, meta)?;
        slides_html.push_str(&slide_markup);
    }

    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=1280">
  <title>{title_escaped}</title>
  <style>{css}</style>
</head>
<body>
<div class="{deck_class}">
{slides_html}</div>
</body>
</html>"#
    ))
}

/// Render a single slide to HTML.
fn render_slide(
    registry: &ComponentRegistry,
    slide: &Slide,
    idx: usize,
    total: usize,
    meta: &Meta,
) -> Result<String, DeckError> {
    let id = &slide.component;
    let def = registry.get(id).ok_or_else(|| {
        ComponentNotFoundSnafu {
            id: id.as_str().to_owned(),
        }
        .build()
    })?;

    let template_source = std::fs::read_to_string(&def.html).context(TemplateLoadSnafu {
        id: id.as_str().to_owned(),
    })?;

    let mut env = minijinja::Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
    let template = env
        .template_from_str(&template_source)
        .context(TemplateRenderSnafu {
            id: id.as_str().to_owned(),
        })?;

    let mut ctx = HashMap::<&str, Value>::new();
    ctx.insert("fields", slide.fields.clone());
    ctx.insert("slide_idx", Value::from(idx));
    ctx.insert("total", Value::from(total));
    ctx.insert(
        "meta",
        serde_json::json!({
            "title": meta.title,
            "author": meta.author,
        }),
    );

    let inner = template.render(ctx).context(TemplateRenderSnafu {
        id: id.as_str().to_owned(),
    })?;

    let component_id = id.as_str();
    Ok(format!(
        r#"  <div class="slide slide--{component_id}" data-slide="{idx}">
    {inner}
  </div>
"#,
    ))
}
