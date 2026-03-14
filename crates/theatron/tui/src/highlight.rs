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
        let hl = Highlighter::new();
        let lines = hl.highlight("let x = 42;", "rust");
        assert!(!lines.is_empty());
    }

    #[test]
    fn highlight_unknown_language_falls_back() {
        let hl = Highlighter::new();
        let lines = hl.highlight("some text", "nonexistent_language_xyz");
        assert!(!lines.is_empty());
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("some text"));
    }

    #[test]
    fn highlight_empty_string() {
        let hl = Highlighter::new();
        let lines = hl.highlight("", "rust");
        // Empty input should produce no lines (or one empty line)
        assert!(lines.len() <= 1);
    }

    #[test]
    fn highlight_multiline_code() {
        let hl = Highlighter::new();
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = hl.highlight(code, "rust");
        assert!(lines.len() >= 3);
    }

    #[test]
    fn highlight_python() {
        let hl = Highlighter::new();
        let lines = hl.highlight("def hello():\n    pass", "python");
        assert!(!lines.is_empty());
    }

    #[test]
    fn highlight_bold_italic_styles() {
        let hl = Highlighter::new();
        let lines = hl.highlight("// comment\nlet x = 1;", "rust");
        // Just verify it doesn't panic and produces output
        assert!(lines.len() >= 2);
    }
}
