use ratatui::style::{Color, Modifier, Style};

/// Terminal color depth, detected at startup.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// 24-bit RGB (COLORTERM=truecolor, iTerm2, Kitty, Alacritty, etc.)
    TrueColor,
    /// 256-color (xterm-256color)
    Color256,
    /// Basic 16 ANSI colors
    Basic,
}

/// Background brightness: drives palette selection.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Background and accent colors.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "complete semantic color set; not all fields used by every view"
)]
pub struct Colors {
    pub bg: Color,
    pub surface: Color,
    pub surface_bright: Color,
    pub surface_dim: Color,
    pub accent: Color,
    pub accent_dim: Color,
}

/// Foreground text and role-speaker colors.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "complete semantic color set; not all fields used by every view"
)]
pub struct TextColors {
    pub fg: Color,
    pub fg_muted: Color,
    pub fg_dim: Color,
    pub user: Color,
    pub assistant: Color,
    pub system: Color,
}

/// Structural border and selection colors.
#[derive(Debug, Clone)]
pub struct Borders {
    pub normal: Color,
    pub focused: Color,
    pub separator: Color,
    pub selected: Color,
}

/// Semantic feedback and animation-state colors.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "complete semantic color set; not all fields used by every view"
)]
pub struct StatusColors {
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    pub spinner: Color,
    pub idle: Color,
    pub streaming: Color,
    pub compacting: Color,
}

/// Code-block colors.
#[derive(Debug, Clone)]
pub struct CodeColors {
    pub fg: Color,
    pub bg: Color,
    pub lang: Color,
}

/// Thinking-block colors.
#[derive(Debug, Clone)]
pub struct ThinkingColors {
    pub fg: Color,
    pub border: Color,
}

/// Semantic color palette for the entire TUI.
/// Every color usage flows through this: no ad-hoc `Color::Cyan` calls.
///
/// Structured as nested groups so the active theme can be swapped as a single
/// value without touching individual call sites.
#[derive(Debug, Clone)]
pub struct Theme {
    pub colors: Colors,
    pub text: TextColors,
    pub borders: Borders,
    pub status: StatusColors,
    pub code: CodeColors,
    pub thinking: ThinkingColors,
    /// Color depth (for conditional rendering).
    pub depth: ColorDepth,
    /// Light or dark background.
    pub mode: ThemeMode,
}

/// Auto-detected theme from the terminal environment.
/// Used as the default when no config override is set.
#[cfg(test)]
pub static THEME: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(Theme::default);

impl Default for Theme {
    fn default() -> Self {
        Self::detect()
    }
}

impl Theme {
    /// Create theme based on detected terminal capability and background.
    pub fn detect() -> Self {
        Self::for_mode(None)
    }

    /// Create theme for a specific mode. `None` means auto-detect from the terminal.
    pub fn for_mode(mode: Option<ThemeMode>) -> Self {
        let resolved = mode.unwrap_or_else(detect_background);
        let depth = detect_color_depth();
        match (resolved, depth) {
            (ThemeMode::Light, ColorDepth::TrueColor) => Self::truecolor_light(),
            (ThemeMode::Light, ColorDepth::Color256) => Self::color256_light(),
            (ThemeMode::Light, ColorDepth::Basic) => Self::basic_light(),
            (ThemeMode::Dark, ColorDepth::TrueColor) => Self::truecolor(),
            (ThemeMode::Dark, ColorDepth::Color256) => Self::color256(),
            (ThemeMode::Dark, ColorDepth::Basic) => Self::basic(),
        }
    }

    /// Full 24-bit RGB palette: the target experience.
    pub(crate) fn truecolor() -> Self {
        Self {
            colors: Colors {
                bg: Color::Reset,
                surface: Color::Rgb(30, 30, 36),
                surface_bright: Color::Rgb(45, 45, 54),
                surface_dim: Color::Rgb(22, 22, 28),
                accent: Color::Rgb(120, 180, 255),
                accent_dim: Color::Rgb(70, 110, 170),
            },
            text: TextColors {
                fg: Color::Rgb(220, 220, 230),
                fg_muted: Color::Rgb(140, 140, 160),
                fg_dim: Color::Rgb(85, 85, 100),
                user: Color::Rgb(120, 180, 255),
                assistant: Color::Rgb(120, 220, 150),
                system: Color::Rgb(140, 140, 160),
            },
            borders: Borders {
                normal: Color::Rgb(60, 60, 75),
                focused: Color::Rgb(120, 180, 255),
                separator: Color::Rgb(50, 50, 62),
                selected: Color::Rgb(120, 180, 255),
            },
            status: StatusColors {
                success: Color::Rgb(120, 220, 150),
                warning: Color::Rgb(240, 190, 80),
                error: Color::Rgb(240, 100, 100),
                info: Color::Rgb(120, 180, 255),
                spinner: Color::Rgb(240, 190, 80),
                idle: Color::Rgb(85, 85, 100),
                streaming: Color::Rgb(120, 220, 150),
                compacting: Color::Rgb(180, 120, 240),
            },
            code: CodeColors {
                fg: Color::Rgb(200, 200, 215),
                bg: Color::Rgb(35, 35, 42),
                lang: Color::Rgb(100, 100, 120),
            },
            thinking: ThinkingColors {
                fg: Color::Rgb(100, 100, 120),
                border: Color::Rgb(60, 60, 75),
            },
            depth: ColorDepth::TrueColor,
            mode: ThemeMode::Dark,
        }
    }

    /// 24-bit RGB light palette: readable on white/light terminal backgrounds.
    pub(crate) fn truecolor_light() -> Self {
        Self {
            colors: Colors {
                bg: Color::Reset,
                surface: Color::Rgb(245, 245, 248),
                surface_bright: Color::Rgb(255, 255, 255),
                surface_dim: Color::Rgb(230, 230, 236),
                accent: Color::Rgb(30, 100, 210),
                accent_dim: Color::Rgb(100, 140, 200),
            },
            text: TextColors {
                fg: Color::Rgb(30, 30, 40),
                fg_muted: Color::Rgb(100, 100, 120),
                fg_dim: Color::Rgb(150, 150, 165),
                user: Color::Rgb(30, 100, 210),
                assistant: Color::Rgb(20, 140, 60),
                system: Color::Rgb(100, 100, 120),
            },
            borders: Borders {
                normal: Color::Rgb(200, 200, 212),
                focused: Color::Rgb(30, 100, 210),
                separator: Color::Rgb(215, 215, 225),
                selected: Color::Rgb(30, 100, 210),
            },
            status: StatusColors {
                success: Color::Rgb(20, 140, 60),
                warning: Color::Rgb(180, 130, 0),
                error: Color::Rgb(200, 50, 50),
                info: Color::Rgb(30, 100, 210),
                spinner: Color::Rgb(180, 130, 0),
                idle: Color::Rgb(150, 150, 165),
                streaming: Color::Rgb(20, 140, 60),
                compacting: Color::Rgb(130, 60, 200),
            },
            code: CodeColors {
                fg: Color::Rgb(40, 40, 50),
                bg: Color::Rgb(235, 235, 240),
                lang: Color::Rgb(130, 130, 145),
            },
            thinking: ThinkingColors {
                fg: Color::Rgb(130, 130, 145),
                border: Color::Rgb(200, 200, 212),
            },
            depth: ColorDepth::TrueColor,
            mode: ThemeMode::Light,
        }
    }

    /// 256-color fallback: approximates the true color palette.
    pub(crate) fn color256() -> Self {
        Self {
            colors: Colors {
                bg: Color::Reset,
                surface: Color::Indexed(235),
                surface_bright: Color::Indexed(237),
                surface_dim: Color::Indexed(233),
                accent: Color::Indexed(111),
                accent_dim: Color::Indexed(67),
            },
            text: TextColors {
                fg: Color::Indexed(253),
                fg_muted: Color::Indexed(245),
                fg_dim: Color::Indexed(240),
                user: Color::Indexed(111),
                assistant: Color::Indexed(114),
                system: Color::Indexed(245),
            },
            borders: Borders {
                normal: Color::Indexed(238),
                focused: Color::Indexed(111),
                separator: Color::Indexed(236),
                selected: Color::Indexed(111),
            },
            status: StatusColors {
                success: Color::Indexed(114),
                warning: Color::Indexed(221),
                error: Color::Indexed(167),
                info: Color::Indexed(111),
                spinner: Color::Indexed(221),
                idle: Color::Indexed(240),
                streaming: Color::Indexed(114),
                compacting: Color::Indexed(177),
            },
            code: CodeColors {
                fg: Color::Indexed(252),
                bg: Color::Indexed(236),
                lang: Color::Indexed(242),
            },
            thinking: ThinkingColors {
                fg: Color::Indexed(242),
                border: Color::Indexed(238),
            },
            depth: ColorDepth::Color256,
            mode: ThemeMode::Dark,
        }
    }

    /// 256-color light palette.
    pub(crate) fn color256_light() -> Self {
        Self {
            colors: Colors {
                bg: Color::Reset,
                surface: Color::Indexed(255),
                surface_bright: Color::Indexed(231),
                surface_dim: Color::Indexed(254),
                accent: Color::Indexed(25),
                accent_dim: Color::Indexed(67),
            },
            text: TextColors {
                fg: Color::Indexed(234),
                fg_muted: Color::Indexed(243),
                fg_dim: Color::Indexed(249),
                user: Color::Indexed(25),
                assistant: Color::Indexed(28),
                system: Color::Indexed(243),
            },
            borders: Borders {
                normal: Color::Indexed(252),
                focused: Color::Indexed(25),
                separator: Color::Indexed(254),
                selected: Color::Indexed(25),
            },
            status: StatusColors {
                success: Color::Indexed(28),
                warning: Color::Indexed(136),
                error: Color::Indexed(160),
                info: Color::Indexed(25),
                spinner: Color::Indexed(136),
                idle: Color::Indexed(249),
                streaming: Color::Indexed(28),
                compacting: Color::Indexed(128),
            },
            code: CodeColors {
                fg: Color::Indexed(234),
                bg: Color::Indexed(254),
                lang: Color::Indexed(246),
            },
            thinking: ThinkingColors {
                fg: Color::Indexed(246),
                border: Color::Indexed(252),
            },
            depth: ColorDepth::Color256,
            mode: ThemeMode::Light,
        }
    }

    /// Basic 16-color ANSI: widest compatibility.
    pub(crate) fn basic() -> Self {
        Self {
            colors: Colors {
                bg: Color::Reset,
                surface: Color::Reset,
                surface_bright: Color::DarkGray,
                surface_dim: Color::Reset,
                accent: Color::Cyan,
                accent_dim: Color::DarkGray,
            },
            text: TextColors {
                fg: Color::White,
                fg_muted: Color::Gray,
                fg_dim: Color::DarkGray,
                user: Color::Cyan,
                assistant: Color::Green,
                system: Color::DarkGray,
            },
            borders: Borders {
                normal: Color::DarkGray,
                focused: Color::Cyan,
                separator: Color::DarkGray,
                selected: Color::Cyan,
            },
            status: StatusColors {
                success: Color::Green,
                warning: Color::Yellow,
                error: Color::Red,
                info: Color::Cyan,
                spinner: Color::Yellow,
                idle: Color::DarkGray,
                streaming: Color::Green,
                compacting: Color::Magenta,
            },
            code: CodeColors {
                fg: Color::White,
                bg: Color::DarkGray,
                lang: Color::DarkGray,
            },
            thinking: ThinkingColors {
                fg: Color::DarkGray,
                border: Color::DarkGray,
            },
            depth: ColorDepth::Basic,
            mode: ThemeMode::Dark,
        }
    }

    /// Basic 16-color light palette.
    pub(crate) fn basic_light() -> Self {
        Self {
            colors: Colors {
                bg: Color::Reset,
                surface: Color::Reset,
                surface_bright: Color::White,
                surface_dim: Color::Reset,
                accent: Color::Blue,
                accent_dim: Color::DarkGray,
            },
            text: TextColors {
                fg: Color::Black,
                fg_muted: Color::DarkGray,
                fg_dim: Color::Gray,
                user: Color::Blue,
                assistant: Color::Green,
                system: Color::DarkGray,
            },
            borders: Borders {
                normal: Color::Gray,
                focused: Color::Blue,
                separator: Color::Gray,
                selected: Color::Blue,
            },
            status: StatusColors {
                success: Color::Green,
                warning: Color::Yellow,
                error: Color::Red,
                info: Color::Blue,
                spinner: Color::Yellow,
                idle: Color::Gray,
                streaming: Color::Green,
                compacting: Color::Magenta,
            },
            code: CodeColors {
                fg: Color::Black,
                bg: Color::White,
                lang: Color::DarkGray,
            },
            thinking: ThinkingColors {
                fg: Color::DarkGray,
                border: Color::Gray,
            },
            depth: ColorDepth::Basic,
            mode: ThemeMode::Light,
        }
    }

    pub fn style_fg(&self) -> Style {
        Style::default().fg(self.text.fg)
    }

    pub fn style_muted(&self) -> Style {
        Style::default().fg(self.text.fg_muted)
    }

    pub fn style_dim(&self) -> Style {
        Style::default().fg(self.text.fg_dim)
    }

    pub fn style_accent(&self) -> Style {
        Style::default().fg(self.colors.accent)
    }

    pub fn style_accent_bold(&self) -> Style {
        Style::default()
            .fg(self.colors.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_success(&self) -> Style {
        Style::default().fg(self.status.success)
    }

    pub fn style_warning(&self) -> Style {
        Style::default().fg(self.status.warning)
    }

    pub fn style_error(&self) -> Style {
        Style::default().fg(self.status.error)
    }

    pub fn style_success_bold(&self) -> Style {
        Style::default()
            .fg(self.status.success)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_error_bold(&self) -> Style {
        Style::default()
            .fg(self.status.error)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_user(&self) -> Style {
        Style::default()
            .fg(self.text.user)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_assistant(&self) -> Style {
        Style::default()
            .fg(self.text.assistant)
            .add_modifier(Modifier::BOLD)
    }

    #[expect(dead_code, reason = "part of the complete style API")]
    pub fn style_code(&self) -> Style {
        Style::default().fg(self.code.fg).bg(self.code.bg)
    }

    pub fn style_inline_code(&self) -> Style {
        Style::default().fg(self.status.warning).bg(self.code.bg)
    }

    pub fn style_surface(&self) -> Style {
        Style::default().bg(self.colors.surface)
    }

    pub fn style_border(&self) -> Style {
        Style::default().fg(self.borders.normal)
    }

    #[expect(dead_code, reason = "part of the complete style API")]
    pub fn style_border_focused(&self) -> Style {
        Style::default().fg(self.borders.focused)
    }
}

/// Detect terminal background brightness from `$COLORFGBG`.
///
/// Format: `fg;bg` or `fg;X;bg` where values are ANSI color indices.
/// Indices 0-6 are dark colors, 7+ are light. Defaults to dark when unset.
fn detect_background() -> ThemeMode {
    if let Ok(val) = std::env::var("COLORFGBG") {
        // WHY: Some terminals emit three values (e.g., "15;0;0"). The background
        // is always the last component.
        if let Some(bg_str) = val.rsplit(';').next()
            && let Ok(bg) = bg_str.parse::<u8>()
        {
            return if bg >= 8 {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            };
        }
    }
    ThemeMode::Dark
}

/// Detect terminal color capability from environment variables.
fn detect_color_depth() -> ColorDepth {
    // WHY: COLORTERM is the most reliable indicator: check it before TERM.
    if let Ok(ct) = std::env::var("COLORTERM") {
        match ct.as_str() {
            "truecolor" | "24bit" => return ColorDepth::TrueColor,
            // NOTE: unrecognized COLORTERM value, check other env vars
            _ => {}
        }
    }

    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        match tp.as_str() {
            "iTerm.app" | "WezTerm" | "Alacritty" | "kitty" => return ColorDepth::TrueColor,
            // NOTE: unrecognized terminal program, continue probing
            _ => {}
        }
    }

    // NOTE: GNOME Terminal sets COLORTERM=truecolor, but VTE_VERSION is a reliable backup.
    if std::env::var("VTE_VERSION").is_ok() {
        return ColorDepth::TrueColor;
    }

    if let Ok(term) = std::env::var("TERM")
        && term.contains("256color")
    {
        return ColorDepth::Color256;
    }

    if std::env::var("TMUX").is_ok() {
        return ColorDepth::Color256;
    }

    ColorDepth::Basic
}

/// Braille spinner frames for smooth animation.
pub const BRAILLE_SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Get the current braille spinner frame based on tick count.
pub fn spinner_frame(tick: u64) -> char {
    BRAILLE_SPINNER[(tick as usize / 3) % BRAILLE_SPINNER.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truecolor_palette_has_correct_depth() {
        let theme = Theme::truecolor();
        assert_eq!(theme.depth, ColorDepth::TrueColor);
        assert_eq!(theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn color256_palette_has_correct_depth() {
        let theme = Theme::color256();
        assert_eq!(theme.depth, ColorDepth::Color256);
        assert_eq!(theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn basic_palette_has_correct_depth() {
        let theme = Theme::basic();
        assert_eq!(theme.depth, ColorDepth::Basic);
        assert_eq!(theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn truecolor_light_palette_has_correct_mode() {
        let theme = Theme::truecolor_light();
        assert_eq!(theme.depth, ColorDepth::TrueColor);
        assert_eq!(theme.mode, ThemeMode::Light);
    }

    #[test]
    fn color256_light_palette_has_correct_mode() {
        let theme = Theme::color256_light();
        assert_eq!(theme.depth, ColorDepth::Color256);
        assert_eq!(theme.mode, ThemeMode::Light);
    }

    #[test]
    fn basic_light_palette_has_correct_mode() {
        let theme = Theme::basic_light();
        assert_eq!(theme.depth, ColorDepth::Basic);
        assert_eq!(theme.mode, ThemeMode::Light);
    }

    #[test]
    fn for_mode_dark_returns_dark() {
        let theme = Theme::for_mode(Some(ThemeMode::Dark));
        assert_eq!(theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn for_mode_light_returns_light() {
        let theme = Theme::for_mode(Some(ThemeMode::Light));
        assert_eq!(theme.mode, ThemeMode::Light);
    }

    #[test]
    fn style_fg_uses_fg_color() {
        let theme = Theme::truecolor();
        let style = theme.style_fg();
        assert_eq!(style.fg, Some(theme.text.fg));
    }

    #[test]
    fn style_muted_uses_fg_muted_color() {
        let theme = Theme::truecolor();
        let style = theme.style_muted();
        assert_eq!(style.fg, Some(theme.text.fg_muted));
    }

    #[test]
    fn style_accent_bold_has_bold_modifier() {
        let theme = Theme::truecolor();
        let style = theme.style_accent_bold();
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(style.fg, Some(theme.colors.accent));
    }

    #[test]
    fn style_user_has_bold() {
        let theme = Theme::basic();
        let style = theme.style_user();
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn style_surface_sets_bg() {
        let theme = Theme::truecolor();
        let style = theme.style_surface();
        assert_eq!(style.bg, Some(theme.colors.surface));
    }

    #[test]
    fn style_inline_code_uses_warning_fg() {
        let theme = Theme::truecolor();
        let style = theme.style_inline_code();
        assert_eq!(style.fg, Some(theme.status.warning));
        assert_eq!(style.bg, Some(theme.code.bg));
    }

    #[test]
    fn spinner_frame_cycles() {
        let f0 = spinner_frame(0);
        let f3 = spinner_frame(3);
        assert_ne!(f0, f3);
        // After a full cycle, it wraps
        let total = BRAILLE_SPINNER.len() * 3;
        assert_eq!(spinner_frame(0), spinner_frame(total as u64));
    }

    #[test]
    fn spinner_frame_all_braille() {
        for frame in BRAILLE_SPINNER {
            assert!(
                ('\u{2800}'..='\u{28FF}').contains(frame),
                "spinner frame {:?} is not a braille character",
                frame
            );
        }
    }

    #[test]
    fn detect_returns_valid_depth() {
        let theme = Theme::detect();
        let _ = theme.depth;
    }

    #[test]
    fn all_palettes_have_reset_bg() {
        for theme in [
            Theme::truecolor(),
            Theme::color256(),
            Theme::basic(),
            Theme::truecolor_light(),
            Theme::color256_light(),
            Theme::basic_light(),
        ] {
            assert_eq!(theme.colors.bg, Color::Reset);
        }
    }

    #[test]
    fn light_palettes_have_dark_text() {
        let theme = Theme::truecolor_light();
        // Text on a light background must be dark
        if let Color::Rgb(r, g, b) = theme.text.fg {
            assert!(
                r < 100 && g < 100 && b < 100,
                "light theme fg should be dark: ({r}, {g}, {b})"
            );
        }
    }

    #[test]
    fn theme_static_is_accessible() {
        let _ = THEME.depth;
    }

    #[test]
    fn struct_of_structs_groups_are_populated() {
        let theme = Theme::truecolor();
        let _ = theme.colors.accent;
        let _ = theme.text.fg;
        let _ = theme.borders.normal;
        let _ = theme.status.success;
        let _ = theme.code.fg;
        let _ = theme.thinking.fg;
    }
}
