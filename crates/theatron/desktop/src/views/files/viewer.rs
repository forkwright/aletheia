//! Syntax-highlighted file viewer with line numbers.

use dioxus::prelude::*;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::files::{extension_to_language, is_binary_content};
use crate::views::files::toolbar::ViewerToolbar;

const VIEWER_CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    flex: 1; \
    overflow: hidden; \
    background: var(--bg-surface, #1a1816); \
    border: 1px solid var(--border, #2e2b27); \
    border-radius: var(--radius-md, 6px);\
";

const CODE_AREA_STYLE: &str = "\
    display: flex; \
    flex: 1; \
    overflow: auto; \
    font-family: var(--font-mono, monospace); \
    font-size: 13px; \
    line-height: 1.5;\
";

const GUTTER_STYLE: &str = "\
    padding: 12px 8px 12px 12px; \
    text-align: right; \
    color: var(--text-muted, #706c66); \
    user-select: none; \
    flex-shrink: 0; \
    min-width: 40px; \
    border-right: 1px solid var(--border-separator, #221f1c);\
";

const CODE_STYLE_WRAP: &str = "\
    padding: 12px; \
    flex: 1; \
    white-space: pre-wrap; \
    word-wrap: break-word; \
    color: var(--code-fg, #d4d0ca);\
";

const CODE_STYLE_NOWRAP: &str = "\
    padding: 12px; \
    flex: 1; \
    white-space: pre; \
    overflow-x: auto; \
    color: var(--code-fg, #d4d0ca);\
";

const EMPTY_STATE_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: var(--text-muted, #706c66); \
    font-size: 14px;\
";

/// Highlighted line: a sequence of (html_color, bold, italic, text) spans.
struct HighlightedSpan {
    color: String,
    bold: bool,
    italic: bool,
    text: String,
}

struct HighlightedLine {
    spans: Vec<HighlightedSpan>,
}

#[derive(Debug, Clone)]
enum ViewerState {
    Empty,
    Loading,
    Binary {
        path: String,
    },
    Loaded {
        path: String,
        content: String,
        line_count: usize,
        byte_size: usize,
    },
    Error(String),
}

#[component]
pub(crate) fn FileViewer(selected_path: Signal<Option<String>>) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut state = use_signal(|| ViewerState::Empty);
    let word_wrap = use_signal(|| true);
    let mut last_loaded_path = use_signal(String::new);

    // NOTE: Fetch file content when selected_path changes.
    use_effect(move || {
        let path = selected_path.read().clone();
        if let Some(path) = path {
            if *last_loaded_path.read() == path {
                return;
            }
            last_loaded_path.set(path.clone());
            state.set(ViewerState::Loading);

            let cfg = config.read().clone();
            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String = form_urlencoded::byte_serialize(path.as_bytes()).collect();
                let url = format!("{base}/api/v1/workspace/files/content?path={encoded}");

                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                        Ok(bytes) => {
                            if is_binary_content(&bytes) {
                                state.set(ViewerState::Binary { path });
                            } else {
                                let content = String::from_utf8_lossy(&bytes).into_owned();
                                let line_count = content.lines().count();
                                let byte_size = bytes.len();
                                state.set(ViewerState::Loaded {
                                    path,
                                    content,
                                    line_count,
                                    byte_size,
                                });
                            }
                        }
                        Err(e) => {
                            state.set(ViewerState::Error(format!("read: {e}")));
                        }
                    },
                    Ok(resp) => {
                        state.set(ViewerState::Error(format!("status: {}", resp.status())));
                    }
                    Err(e) => {
                        state.set(ViewerState::Error(format!("connection: {e}")));
                    }
                }
            });
        } else {
            state.set(ViewerState::Empty);
            last_loaded_path.set(String::new());
        }
    });

    match &*state.read() {
        ViewerState::Empty => rsx! {
            div {
                style: "{VIEWER_CONTAINER_STYLE}",
                div {
                    style: "{EMPTY_STATE_STYLE}",
                    "Select a file to view"
                }
            }
        },
        ViewerState::Loading => rsx! {
            div {
                style: "{VIEWER_CONTAINER_STYLE}",
                div {
                    style: "{EMPTY_STATE_STYLE}",
                    "Loading..."
                }
            }
        },
        ViewerState::Binary { path } => rsx! {
            div {
                style: "{VIEWER_CONTAINER_STYLE}",
                ViewerToolbar {
                    path: path.clone(),
                    line_count: 0,
                    byte_size: 0,
                    word_wrap,
                }
                div {
                    style: "{EMPTY_STATE_STYLE}",
                    "Binary file \u{2014} cannot display"
                }
            }
        },
        ViewerState::Loaded {
            path,
            content,
            line_count,
            byte_size,
        } => {
            let highlighted = highlight_content(content, path);
            let code_style = if *word_wrap.read() {
                CODE_STYLE_WRAP
            } else {
                CODE_STYLE_NOWRAP
            };
            let lc = *line_count;
            let bs = *byte_size;
            let path = path.clone();

            rsx! {
                div {
                    style: "{VIEWER_CONTAINER_STYLE}",
                    ViewerToolbar {
                        path,
                        line_count: lc,
                        byte_size: bs,
                        word_wrap,
                    }
                    div {
                        style: "{CODE_AREA_STYLE}",
                        // Gutter with line numbers
                        pre {
                            style: "{GUTTER_STYLE}",
                            for i in 1..=lc {
                                "{i}\n"
                            }
                        }
                        // Code content with syntax highlighting
                        pre {
                            style: "{code_style}",
                            for line in highlighted {
                                {render_highlighted_line(&line)}
                            }
                        }
                    }
                }
            }
        }
        ViewerState::Error(err) => rsx! {
            div {
                style: "{VIEWER_CONTAINER_STYLE}",
                div {
                    style: "{EMPTY_STATE_STYLE} color: var(--status-error, #A04040);",
                    "Error: {err}"
                }
            }
        },
    }
}

fn highlight_content(content: &str, path: &str) -> Vec<HighlightedLine> {
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set = ThemeSet::load_defaults();

    let lang = extension_to_language(path);
    let syntax = syntax_set
        .find_syntax_by_token(lang)
        .or_else(|| syntax_set.find_syntax_by_extension(lang))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());

    // WHY: Use the same base16-ocean.dark theme as the TUI highlighter for
    // visual consistency across frontends.
    let theme = theme_set
        .themes
        .get("base16-ocean.dark")
        .unwrap_or_else(|| {
            theme_set
                .themes
                .values()
                .next()
                .expect("syntect default themes are non-empty")
        });

    let mut h = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for line_str in LinesWithEndings::from(content) {
        match h.highlight_line(line_str, &syntax_set) {
            Ok(ranges) => {
                let spans = ranges
                    .into_iter()
                    .map(|(style, text)| {
                        let color = format!(
                            "rgb({}, {}, {})",
                            style.foreground.r, style.foreground.g, style.foreground.b
                        );
                        HighlightedSpan {
                            color,
                            bold: style.font_style.contains(FontStyle::BOLD),
                            italic: style.font_style.contains(FontStyle::ITALIC),
                            text: text.trim_end_matches('\n').to_string(),
                        }
                    })
                    .collect();
                lines.push(HighlightedLine { spans });
            }
            Err(_) => {
                lines.push(HighlightedLine {
                    spans: vec![HighlightedSpan {
                        color: "var(--code-fg, #d4d0ca)".into(),
                        bold: false,
                        italic: false,
                        text: line_str.trim_end_matches('\n').to_string(),
                    }],
                });
            }
        }
    }

    lines
}

fn render_highlighted_line(line: &HighlightedLine) -> Element {
    rsx! {
        div {
            style: "min-height: 1.5em;",
            for span in &line.spans {
                span {
                    style: "color: {span.color};{bold_style(span.bold)}{italic_style(span.italic)}",
                    "{span.text}"
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
