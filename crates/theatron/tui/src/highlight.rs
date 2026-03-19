use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::theme::ThemeMode;

/// Lazily-loaded syntax highlighting resources.
/// syntect's SyntaxSet + ThemeSet are expensive to build: load once.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    theme_name: &'static str,
}

impl Highlighter {
    pub fn new(mode: ThemeMode) -> Self {
        let theme_name = match mode {
            ThemeMode::Light => "base16-ocean.light",
            ThemeMode::Dark => "base16-ocean.dark",
        };
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
            theme_name,
        }
    }

    /// Highlight a code block, returning ratatui Lines.
    /// Falls back to plain text if the language isn't recognized.
    #[expect(
        clippy::indexing_slicing,
        reason = "theme_name is set in new() to a string constant guaranteed to exist in the default ThemeSet; key absence would be a programming error"
    )]
    pub fn highlight(&self, code: &str, lang: &str) -> Vec<Line<'static>> {
        let theme = &self.theme_set.themes[self.theme_name];

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
                    lines.push(Line::raw(line_str.trim_end_matches('\n').to_string()));
                }
            }
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_rust_produces_lines() {
        let hl = Highlighter::new(ThemeMode::Dark);
        let lines = hl.highlight("let x = 42;", "rust");
        assert!(!lines.is_empty());
    }

    #[test]
    fn highlight_unknown_language_falls_back() {
        let hl = Highlighter::new(ThemeMode::Dark);
        let lines = hl.highlight("some text", "nonexistent_language_xyz");
        assert!(!lines.is_empty());
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("some text"));
    }

    #[test]
    fn highlight_empty_string() {
        let hl = Highlighter::new(ThemeMode::Dark);
        let lines = hl.highlight("", "rust");
        assert!(lines.len() <= 1);
    }

    #[test]
    fn highlight_multiline_code() {
        let hl = Highlighter::new(ThemeMode::Dark);
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = hl.highlight(code, "rust");
        assert!(lines.len() >= 3);
    }

    #[test]
    fn highlight_python() {
        let hl = Highlighter::new(ThemeMode::Dark);
        let lines = hl.highlight("def hello():\n    pass", "python");
        assert!(!lines.is_empty());
    }

    #[test]
    fn highlight_bold_italic_styles() {
        let hl = Highlighter::new(ThemeMode::Dark);
        let lines = hl.highlight("// comment\nlet x = 1;", "rust");
        assert!(lines.len() >= 2);
    }

    #[test]
    fn highlight_light_theme_produces_lines() {
        let hl = Highlighter::new(ThemeMode::Light);
        let lines = hl.highlight("let x = 42;", "rust");
        assert!(!lines.is_empty());
    }
}
