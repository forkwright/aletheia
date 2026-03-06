use ratatui::style::{Color, Modifier, Style};

/// Terminal color depth, detected at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// 24-bit RGB (COLORTERM=truecolor, iTerm2, Kitty, Alacritty, etc.)
    TrueColor,
    /// 256-color (xterm-256color)
    Color256,
    /// Basic 16 ANSI colors
    Basic,
}

/// Semantic color palette for the entire TUI.
/// Every color usage flows through this — no ad-hoc `Color::Cyan` calls.
#[derive(Debug, Clone)]
#[expect(dead_code, reason = "palette fields are the complete semantic color set")]
pub struct ThemePalette {
    // --- Surface colors ---
    pub bg: Color,
    pub surface: Color,
    pub surface_bright: Color,
    pub surface_dim: Color,

    // --- Text ---
    pub fg: Color,
    pub fg_muted: Color,
    pub fg_dim: Color,

    // --- Accent ---
    pub accent: Color,
    pub accent_dim: Color,

    // --- Semantic ---
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,

    // --- Agent role colors ---
    pub user: Color,
    pub assistant: Color,
    pub system: Color,

    // --- Interactive ---
    pub selected: Color,
    pub border: Color,
    pub border_focused: Color,
    pub separator: Color,

    // --- Code ---
    pub code_fg: Color,
    pub code_bg: Color,
    pub code_lang: Color,

    // --- Thinking ---
    pub thinking: Color,
    pub thinking_border: Color,

    // --- Status ---
    pub spinner: Color,
    pub idle: Color,
    pub streaming: Color,
    pub compacting: Color,

    // Color depth (for conditional rendering)
    pub depth: ColorDepth,
}

impl ThemePalette {
    /// Create theme based on detected terminal capability.
    pub fn detect() -> Self {
        let depth = detect_color_depth();
        match depth {
            ColorDepth::TrueColor => Self::truecolor(),
            ColorDepth::Color256 => Self::color256(),
            ColorDepth::Basic => Self::basic(),
        }
    }

    /// Full 24-bit RGB palette — the target experience.
    fn truecolor() -> Self {
        Self {
            bg: Color::Reset,
            surface: Color::Rgb(30, 30, 36),
            surface_bright: Color::Rgb(45, 45, 54),
            surface_dim: Color::Rgb(22, 22, 28),

            fg: Color::Rgb(220, 220, 230),
            fg_muted: Color::Rgb(140, 140, 160),
            fg_dim: Color::Rgb(85, 85, 100),

            accent: Color::Rgb(120, 180, 255),
            accent_dim: Color::Rgb(70, 110, 170),

            success: Color::Rgb(120, 220, 150),
            warning: Color::Rgb(240, 190, 80),
            error: Color::Rgb(240, 100, 100),
            info: Color::Rgb(120, 180, 255),

            user: Color::Rgb(120, 180, 255),
            assistant: Color::Rgb(120, 220, 150),
            system: Color::Rgb(140, 140, 160),

            selected: Color::Rgb(120, 180, 255),
            border: Color::Rgb(60, 60, 75),
            border_focused: Color::Rgb(120, 180, 255),
            separator: Color::Rgb(50, 50, 62),

            code_fg: Color::Rgb(200, 200, 215),
            code_bg: Color::Rgb(35, 35, 42),
            code_lang: Color::Rgb(100, 100, 120),

            thinking: Color::Rgb(100, 100, 120),
            thinking_border: Color::Rgb(60, 60, 75),

            spinner: Color::Rgb(240, 190, 80),
            idle: Color::Rgb(85, 85, 100),
            streaming: Color::Rgb(120, 220, 150),
            compacting: Color::Rgb(180, 120, 240),

            depth: ColorDepth::TrueColor,
        }
    }

    /// 256-color fallback — approximates the true color palette.
    fn color256() -> Self {
        Self {
            bg: Color::Reset,
            surface: Color::Indexed(235),        // #262626
            surface_bright: Color::Indexed(237), // #3a3a3a
            surface_dim: Color::Indexed(233),    // #121212

            fg: Color::Indexed(253),       // #dadada
            fg_muted: Color::Indexed(245), // #8a8a8a
            fg_dim: Color::Indexed(240),   // #585858

            accent: Color::Indexed(111),    // #87afff
            accent_dim: Color::Indexed(67), // #5f87af

            success: Color::Indexed(114), // #87d787
            warning: Color::Indexed(221), // #ffd75f
            error: Color::Indexed(167),   // #d75f5f
            info: Color::Indexed(111),    // #87afff

            user: Color::Indexed(111),
            assistant: Color::Indexed(114),
            system: Color::Indexed(245),

            selected: Color::Indexed(111),
            border: Color::Indexed(238), // #444444
            border_focused: Color::Indexed(111),
            separator: Color::Indexed(236), // #303030

            code_fg: Color::Indexed(252),   // #d0d0d0
            code_bg: Color::Indexed(236),   // #303030
            code_lang: Color::Indexed(242), // #6c6c6c

            thinking: Color::Indexed(242),
            thinking_border: Color::Indexed(238),

            spinner: Color::Indexed(221),
            idle: Color::Indexed(240),
            streaming: Color::Indexed(114),
            compacting: Color::Indexed(177), // #d787ff

            depth: ColorDepth::Color256,
        }
    }

    /// Basic 16-color ANSI — widest compatibility.
    fn basic() -> Self {
        Self {
            bg: Color::Reset,
            surface: Color::Reset,
            surface_bright: Color::DarkGray,
            surface_dim: Color::Reset,

            fg: Color::White,
            fg_muted: Color::Gray,
            fg_dim: Color::DarkGray,

            accent: Color::Cyan,
            accent_dim: Color::DarkGray,

            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            info: Color::Cyan,

            user: Color::Cyan,
            assistant: Color::Green,
            system: Color::DarkGray,

            selected: Color::Cyan,
            border: Color::DarkGray,
            border_focused: Color::Cyan,
            separator: Color::DarkGray,

            code_fg: Color::White,
            code_bg: Color::DarkGray,
            code_lang: Color::DarkGray,

            thinking: Color::DarkGray,
            thinking_border: Color::DarkGray,

            spinner: Color::Yellow,
            idle: Color::DarkGray,
            streaming: Color::Green,
            compacting: Color::Magenta,

            depth: ColorDepth::Basic,
        }
    }

    // --- Convenience style constructors ---

    pub fn style_fg(&self) -> Style {
        Style::default().fg(self.fg)
    }

    pub fn style_muted(&self) -> Style {
        Style::default().fg(self.fg_muted)
    }

    pub fn style_dim(&self) -> Style {
        Style::default().fg(self.fg_dim)
    }

    pub fn style_accent(&self) -> Style {
        Style::default().fg(self.accent)
    }

    pub fn style_accent_bold(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_success(&self) -> Style {
        Style::default().fg(self.success)
    }

    #[expect(dead_code, reason = "part of the complete style API")]
    pub fn style_warning(&self) -> Style {
        Style::default().fg(self.warning)
    }

    pub fn style_error(&self) -> Style {
        Style::default().fg(self.error)
    }

    pub fn style_success_bold(&self) -> Style {
        Style::default()
            .fg(self.success)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_error_bold(&self) -> Style {
        Style::default().fg(self.error).add_modifier(Modifier::BOLD)
    }

    pub fn style_user(&self) -> Style {
        Style::default().fg(self.user).add_modifier(Modifier::BOLD)
    }

    pub fn style_assistant(&self) -> Style {
        Style::default()
            .fg(self.assistant)
            .add_modifier(Modifier::BOLD)
    }

    #[expect(dead_code, reason = "part of the complete style API")]
    pub fn style_code(&self) -> Style {
        Style::default().fg(self.code_fg).bg(self.code_bg)
    }

    pub fn style_inline_code(&self) -> Style {
        Style::default().fg(self.warning).bg(self.code_bg)
    }

    pub fn style_surface(&self) -> Style {
        Style::default().bg(self.surface)
    }

    pub fn style_border(&self) -> Style {
        Style::default().fg(self.border)
    }

    #[expect(dead_code, reason = "part of the complete style API")]
    pub fn style_border_focused(&self) -> Style {
        Style::default().fg(self.border_focused)
    }
}

/// Detect terminal color capability from environment variables.
fn detect_color_depth() -> ColorDepth {
    // Check COLORTERM first — most reliable indicator
    if let Ok(ct) = std::env::var("COLORTERM") {
        match ct.as_str() {
            "truecolor" | "24bit" => return ColorDepth::TrueColor,
            _ => {}
        }
    }

    // Known true-color terminals
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        match tp.as_str() {
            "iTerm.app" | "WezTerm" | "Alacritty" | "kitty" => return ColorDepth::TrueColor,
            _ => {}
        }
    }

    // GNOME Terminal sets COLORTERM=truecolor but check VTE_VERSION as backup
    if std::env::var("VTE_VERSION").is_ok() {
        return ColorDepth::TrueColor;
    }

    // Check TERM for 256-color support
    if let Ok(term) = std::env::var("TERM") {
        if term.contains("256color") {
            return ColorDepth::Color256;
        }
    }

    // tmux usually supports 256 colors minimum
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
