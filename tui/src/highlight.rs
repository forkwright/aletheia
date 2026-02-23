use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Lazily-loaded syntax highlighting resources.
/// syntect's SyntaxSet + ThemeSet are expensive to build — load once.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Highlight a code block, returning ratatui Lines.
    /// Falls back to plain text if the language isn't recognized.
    pub fn highlight(&self, code: &str, lang: &str) -> Vec<Line<'static>> {
        let theme = &self.theme_set.themes["base16-ocean.dark"];

        // Try to find syntax by language token
        let syntax = self
            .syntax_set
            .find_syntax_by_token(lang)
            .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut h = HighlightLines::new(syntax, theme);
        let mut lines = Vec::new();

        for line_str in LinesWithEndings::from(code) {
            match h.highlight_line(line_str, &self.syntax_set) {
                Ok(ranges) => {
                    let spans: Vec<Span<'static>> = ranges
                        .into_iter()
                        .map(|(style, text)| {
                            let fg = Color::Rgb(
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            );
                            let mut ratatui_style = Style::default().fg(fg);
                            if style.font_style.contains(FontStyle::BOLD) {
                                ratatui_style =
                                    ratatui_style.add_modifier(ratatui::style::Modifier::BOLD);
                            }
                            if style.font_style.contains(FontStyle::ITALIC) {
                                ratatui_style =
                                    ratatui_style.add_modifier(ratatui::style::Modifier::ITALIC);
                            }
                            Span::styled(text.trim_end_matches('\n').to_string(), ratatui_style)
                        })
                        .collect();
                    lines.push(Line::from(spans));
                }
                Err(_) => {
                    // Fallback: plain text
                    lines.push(Line::raw(line_str.trim_end_matches('\n').to_string()));
                }
            }
        }

        lines
    }
}
