//! Syntax-highlighted code block component.

use std::str::FromStr;
use std::sync::OnceLock;

use dioxus::prelude::*;
use syntect::highlighting::{
    Color as SynColor, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::SyntaxSet;

/// Cached syntax set (loaded once, shared across renders).
fn syntax_set() -> &'static SyntaxSet {
    static SS: OnceLock<SyntaxSet> = OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Warm-shifted theme matching the Aletheia design system.
///
/// Constructed programmatically rather than loading a file to avoid
/// bundling a binary asset. Colors align with the CSS `--syntax-*` tokens.
fn warm_theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        // WHY: hand-tuned to match --syntax-* CSS tokens from themes.css
        let settings = ThemeSettings {
            foreground: Some(SynColor {
                r: 0xd4,
                g: 0xd0,
                b: 0xca,
                a: 0xff,
            }),
            background: Some(SynColor {
                r: 0x1a,
                g: 0x18,
                b: 0x16,
                a: 0xff,
            }),
            ..ThemeSettings::default()
        };

        let items = vec![
            theme_item("keyword", 0xCC, 0x77, 0x55),
            theme_item("storage.type", 0xCC, 0x77, 0x55),
            theme_item("string", 0x7A, 0x9B, 0x6A),
            theme_item("comment", 0x70, 0x6c, 0x66),
            theme_item("entity.name.function", 0xB0, 0x8E, 0x5C),
            theme_item("entity.name.type", 0x8A, 0x9A, 0xB0),
            theme_item("support.type", 0x8A, 0x9A, 0xB0),
            theme_item("constant.numeric", 0xC4, 0x91, 0x3A),
            theme_item("keyword.operator", 0xa8, 0xa4, 0x9e),
            theme_item("punctuation", 0xa8, 0xa4, 0x9e),
            theme_item("variable", 0xd4, 0xd0, 0xca),
            theme_item("meta.attribute", 0x70, 0x6c, 0x66),
        ];

        Theme {
            name: Some("aletheia-warm".to_string()),
            settings,
            scopes: items,
            ..Theme::default()
        }
    })
}

fn theme_item(scope: &str, r: u8, g: u8, b: u8) -> ThemeItem {
    let scope_selector = ScopeSelectors::from_str(scope).unwrap_or_default();
    ThemeItem {
        scope: scope_selector,
        style: StyleModifier {
            foreground: Some(SynColor { r, g, b, a: 0xff }),
            background: None,
            font_style: if scope == "keyword" || scope == "storage.type" {
                Some(FontStyle::BOLD)
            } else {
                None
            },
        },
    }
}

/// Highlighted code line: a sequence of styled spans.
#[derive(Debug, Clone)]
pub(crate) struct HighlightedSpan {
    pub text: String,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
}

/// Highlight source code, returning lines of styled spans.
pub(crate) fn highlight_code(code: &str, language: &str) -> Vec<Vec<HighlightedSpan>> {
    let ss = syntax_set();
    let theme = warm_theme();

    let syntax = if language.is_empty() {
        ss.find_syntax_plain_text()
    } else {
        ss.find_syntax_by_token(language)
            .unwrap_or_else(|| ss.find_syntax_plain_text())
    };

    let mut highlighter = syntect::easy::HighlightLines::new(syntax, theme);
    let mut result = Vec::new();

    for line in syntect::util::LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, ss).unwrap_or_default();

        let spans: Vec<HighlightedSpan> = ranges
            .into_iter()
            .map(|(style, text)| HighlightedSpan {
                text: text.to_string(),
                color: syn_color_to_css(style.foreground),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
            })
            .collect();

        result.push(spans);
    }

    result
}

fn syn_color_to_css(c: SynColor) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// Detect language from fence info string (strip attributes after space).
#[must_use]
pub(crate) fn detect_language(info: &str) -> &str {
    info.split_whitespace().next().unwrap_or("")
}

/// Render a syntax-highlighted code block.
#[component]
pub(crate) fn CodeBlock(code: String, language: String) -> Element {
    let lang_display = if language.is_empty() {
        "text".to_string()
    } else {
        language.clone()
    };

    let highlighted = highlight_code(&code, &language);
    let line_count = highlighted.len();
    // WHY: digit width for line number gutter padding
    let gutter_width = format!("{line_count}").len();

    rsx! {
        div {
            class: "code-block",
            style: "
                position: relative;
                background: var(--code-bg);
                border: 1px solid var(--border);
                border-radius: var(--radius-lg);
                margin: var(--space-2) 0;
                overflow: hidden;
            ",
            // Header: language label + copy button
            div {
                style: "
                    display: flex;
                    justify-content: space-between;
                    align-items: center;
                    padding: var(--space-1) var(--space-3);
                    background: var(--bg-surface-dim);
                    border-bottom: 1px solid var(--border);
                    font-family: var(--font-mono);
                    font-size: var(--text-xs);
                    color: var(--code-lang);
                ",
                span { "{lang_display}" }
                button {
                    onclick: {
                        let code_clone = code.clone();
                        move |_| {
                            // WHY: clipboard API via eval — Dioxus desktop uses webview
                            let escaped = code_clone.replace('\\', "\\\\")
                                .replace('`', "\\`")
                                .replace('$', "\\$");
                            let js = format!("navigator.clipboard.writeText(`{escaped}`)");
                            document::eval(&js);
                        }
                    },
                    style: "
                        background: none;
                        border: 1px solid var(--border);
                        border-radius: var(--radius-md);
                        color: var(--text-muted);
                        font-family: var(--font-mono);
                        font-size: var(--text-xs);
                        padding: 2px var(--space-2);
                        cursor: pointer;
                    ",
                    "copy"
                }
            }
            // Code content with line numbers
            div {
                style: "
                    overflow-x: auto;
                    padding: var(--space-2) 0;
                    font-family: var(--font-mono);
                    font-size: var(--text-sm);
                    line-height: var(--leading-normal);
                ",
                for (i , line_spans) in highlighted.iter().enumerate() {
                    div {
                        key: "{i}",
                        style: "display: flex; min-height: 1.5em;",
                        // Line number gutter
                        span {
                            style: "
                                display: inline-block;
                                width: {gutter_width + 2}ch;
                                text-align: right;
                                padding-right: var(--space-3);
                                color: var(--text-muted);
                                user-select: none;
                                flex-shrink: 0;
                            ",
                            "{i + 1}"
                        }
                        // Code content
                        span {
                            style: "white-space: pre; flex: 1;",
                            for (j , span) in line_spans.iter().enumerate() {
                                span {
                                    key: "{j}",
                                    style: "color: {span.color};{bold_style(span.bold)}{italic_style(span.italic)}",
                                    "{span.text}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn bold_style(bold: bool) -> &'static str {
    if bold { " font-weight: bold;" } else { "" }
}

fn italic_style(italic: bool) -> &'static str {
    if italic { " font-style: italic;" } else { "" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_language_basic() {
        assert_eq!(detect_language("rust"), "rust");
        assert_eq!(detect_language("python file.py"), "python");
        assert_eq!(detect_language(""), "");
    }

    #[test]
    fn highlight_code_returns_lines() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = highlight_code(code, "rust");
        assert_eq!(lines.len(), 3);
        // Each line has at least one span.
        for line in &lines {
            assert!(!line.is_empty());
        }
    }

    #[test]
    fn highlight_code_plain_text_fallback() {
        let code = "some random text";
        let lines = highlight_code(code, "nonexistent-lang-xyz");
        assert!(!lines.is_empty());
    }

    #[test]
    fn syn_color_to_css_format() {
        let c = SynColor {
            r: 0xCC,
            g: 0x77,
            b: 0x55,
            a: 0xff,
        };
        assert_eq!(syn_color_to_css(c), "#cc7755");
    }
}
