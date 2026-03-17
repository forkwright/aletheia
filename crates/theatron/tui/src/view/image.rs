use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Supported image file extensions (lowercase, no dot).
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Maximum height for inline image previews (in terminal rows).
/// Each row uses half-block characters representing two pixel rows.
const MAX_IMAGE_HEIGHT: u32 = 20;

/// Maximum number of cached image renders before eviction.
const MAX_CACHE_ENTRIES: usize = 32;

/// Terminal graphics protocol support level.
///
/// Detected once at startup and cached for the process lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphicsProtocol {
    /// Kitty graphics protocol available (terminal also supports true color).
    Kitty,
    /// Sixel protocol available (terminal also supports true color).
    Sixel,
    /// True-color terminal: use half-block character rendering.
    TrueColor,
    /// Basic terminal: show filename and file size only.
    TextOnly,
}

static GRAPHICS_PROTOCOL: LazyLock<GraphicsProtocol> = LazyLock::new(detect_protocol);

type ImageCache = HashMap<(PathBuf, usize), Vec<Line<'static>>>;

/// Image line cache: keyed by `(path, display_width)`.
///
/// Avoids reloading and re-rendering images every frame (~30 fps).
/// Entries are evicted in bulk when the cache exceeds [`MAX_CACHE_ENTRIES`].
static IMAGE_CACHE: LazyLock<Mutex<ImageCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Detect the terminal's graphics protocol support by inspecting environment
/// variables. Checked once and cached via [`LazyLock`].
fn detect_protocol() -> GraphicsProtocol {
    // Kitty: check KITTY_WINDOW_ID or TERM_PROGRAM
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        return GraphicsProtocol::Kitty;
    }
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    if term_program.eq_ignore_ascii_case("kitty") {
        return GraphicsProtocol::Kitty;
    }

    // Sixel: foot and mlterm have native support; WezTerm supports sixel
    let term_program_lower = term_program.to_lowercase();
    if matches!(term_program_lower.as_str(), "foot" | "mlterm" | "wezterm") {
        return GraphicsProtocol::Sixel;
    }
    let term = std::env::var("TERM").unwrap_or_default();
    if term.contains("mlterm") {
        return GraphicsProtocol::Sixel;
    }

    // True color: COLORTERM is the standard signal
    let colorterm = std::env::var("COLORTERM").unwrap_or_default();
    if colorterm == "truecolor" || colorterm == "24bit" {
        return GraphicsProtocol::TrueColor;
    }

    // Known true-color terminals that may not set COLORTERM
    let known_truecolor = [
        "ghostty",
        "iterm2",
        "iterm.app",
        "alacritty",
        "rio",
        "hyper",
        "tabby",
    ];
    if known_truecolor
        .iter()
        .any(|k| term_program_lower.contains(k))
    {
        return GraphicsProtocol::TrueColor;
    }

    // 256-color terminals can approximate half-blocks (lower quality)
    if term.contains("256color") {
        return GraphicsProtocol::TrueColor;
    }

    GraphicsProtocol::TextOnly
}

/// Return the detected graphics protocol (cached).
pub(crate) fn graphics_protocol() -> GraphicsProtocol {
    *GRAPHICS_PROTOCOL
}

/// Returns `true` if the terminal can render half-block image previews.
pub(crate) fn supports_image_preview() -> bool {
    !matches!(graphics_protocol(), GraphicsProtocol::TextOnly)
}

/// Extract local image file paths from message text.
///
/// Scans whitespace/punctuation-separated tokens for paths ending with known
/// image extensions. Only returns paths that exist on disk as regular files.
pub(crate) fn detect_image_paths(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for word in text.split(|c: char| c.is_whitespace() || c == '`' || c == '"' || c == '\'') {
        let cleaned = word.trim_matches(|c: char| "()[]<>,;".contains(c));
        if cleaned.is_empty() || cleaned.len() < 5 {
            continue;
        }

        let path = Path::new(cleaned);
        let ext_match = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()));

        if ext_match {
            let path_buf = PathBuf::from(cleaned);
            if seen.insert(path_buf.clone()) && path_buf.is_file() {
                paths.push(path_buf);
            }
        }
    }

    paths
}

/// Render image preview lines for a detected image path.
///
/// On true-color capable terminals, renders half-block characters with per-pixel
/// coloring. On basic terminals, shows filename and file size as dim text.
/// Results are cached to avoid re-loading images every frame.
pub(crate) fn render_preview_lines(path: &Path, max_width: usize) -> Vec<Line<'static>> {
    if supports_image_preview() {
        render_halfblock_cached(path, max_width)
    } else {
        vec![Line::from(vec![
            Span::raw("  "),
            Span::styled(format_file_info(path), Style::default().fg(Color::DarkGray)),
        ])]
    }
}

/// Render half-block lines with caching.
fn render_halfblock_cached(path: &Path, max_width: usize) -> Vec<Line<'static>> {
    let key = (path.to_path_buf(), max_width);

    // Check cache
    if let Ok(cache) = IMAGE_CACHE.lock()
        && let Some(lines) = cache.get(&key)
    {
        return lines.clone();
    }

    let lines = load_and_render_halfblocks(path, max_width);

    // Store in cache
    if let Ok(mut cache) = IMAGE_CACHE.lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, lines.clone());
    }

    lines
}

/// Load an image from disk and convert to half-block colored text lines.
///
/// Uses `▀` (upper half block) with `fg = top_pixel`, `bg = bottom_pixel`.
/// Each text row represents two pixel rows, doubling effective vertical resolution.
/// Images are scaled to fit within `max_width` columns and [`MAX_IMAGE_HEIGHT`] rows,
/// maintaining aspect ratio. Never upscales.
fn load_and_render_halfblocks(path: &Path, max_width: usize) -> Vec<Line<'static>> {
    let img = match image::open(path) {
        Ok(img) => img,
        Err(_) => return render_error_line(path),
    };

    let img = img.to_rgba8();
    let (orig_w, orig_h) = (img.width(), img.height());

    if orig_w == 0 || orig_h == 0 {
        return render_error_line(path);
    }

    // Reserve 2 columns for left margin
    #[expect(
        clippy::cast_possible_truncation,
        reason = "terminal dimensions fit in u32"
    )]
    let avail_width = max_width.saturating_sub(2).max(1) as u32;
    let max_pixel_h = MAX_IMAGE_HEIGHT * 2;

    let scale_w = f64::from(avail_width) / f64::from(orig_w);
    let scale_h = f64::from(max_pixel_h) / f64::from(orig_h);
    let scale = scale_w.min(scale_h).min(1.0);

    #[expect(
        clippy::cast_possible_truncation,
        reason = "scaled dimension bounded by original u32 dimension"
    )]
    let new_w = ((f64::from(orig_w) * scale) as u32).max(1);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "scaled dimension bounded by original u32 dimension"
    )]
    let new_h = ((f64::from(orig_h) * scale) as u32).max(1);

    let resized =
        image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Triangle);

    let mut lines = Vec::new();

    // Header: filename + original dimensions
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("┌─ {} ({}x{})", filename, orig_w, orig_h),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // Pixel rows → half-block text rows
    let mut y = 0u32;
    while y < new_h {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(new_w as usize + 1); // u32→usize: widening on 32/64-bit
        spans.push(Span::raw("  ")); // left margin

        for x in 0..new_w {
            let top = resized.get_pixel(x, y);
            let bottom = if y + 1 < new_h {
                *resized.get_pixel(x, y + 1)
            } else {
                image::Rgba([0, 0, 0, 0])
            };

            let top_visible = top[3] > 0;
            let bottom_visible = bottom[3] > 0;

            if !top_visible && !bottom_visible {
                spans.push(Span::raw(" "));
            } else if !top_visible {
                spans.push(Span::styled(
                    "▄",
                    Style::default().fg(Color::Rgb(bottom[0], bottom[1], bottom[2])),
                ));
            } else if !bottom_visible {
                spans.push(Span::styled(
                    "▀",
                    Style::default().fg(Color::Rgb(top[0], top[1], top[2])),
                ));
            } else {
                spans.push(Span::styled(
                    "▀",
                    Style::default()
                        .fg(Color::Rgb(top[0], top[1], top[2]))
                        .bg(Color::Rgb(bottom[0], bottom[1], bottom[2])),
                ));
            }
        }

        lines.push(Line::from(spans));
        y += 2;
    }

    lines
}

/// Render a fallback error line when an image fails to load.
fn render_error_line(path: &Path) -> Vec<Line<'static>> {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    vec![Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("[could not load: {filename}]"),
            Style::default().fg(Color::DarkGray),
        ),
    ])]
}

/// Format file info for the text-only fallback: `[image: filename (size)]`.
fn format_file_info(path: &Path) -> String {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    let size = std::fs::metadata(path)
        .map(|m| format_size(m.len()))
        .unwrap_or_else(|_| "unknown size".to_string());

    format!("[image: {filename} ({size})]")
}

/// Human-readable file size formatting.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_non_image_extensions() {
        let paths = detect_image_paths("/tmp/test.rs /tmp/test.toml /tmp/readme.md");
        assert!(paths.is_empty());
    }

    #[test]
    fn detects_image_extension_but_skips_missing_files() {
        let paths = detect_image_paths("result: /nonexistent/path/to/photo.png");
        assert!(paths.is_empty());
    }

    #[test]
    fn strips_punctuation_around_paths() {
        // Paths wrapped in backticks, parens, or brackets
        let paths = detect_image_paths("`/nonexistent/image.jpg` (/no/file.png)");
        assert!(paths.is_empty()); // Files don't exist, but shouldn't crash
    }

    #[test]
    fn deduplicates_paths() {
        let text = "/tmp/test.png and again /tmp/test.png";
        let paths = detect_image_paths(text);
        // Even if the file existed, it would only appear once
        assert!(paths.len() <= 1);
    }

    #[test]
    fn skips_very_short_tokens() {
        let paths = detect_image_paths("a.png");
        assert!(paths.is_empty()); // too short and doesn't exist
    }

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(500), "500 B");
    }

    #[test]
    fn format_size_kilobytes() {
        assert_eq!(format_size(2048), "2.0 KB");
    }

    #[test]
    fn format_size_megabytes() {
        assert_eq!(format_size(1_500_000), "1.4 MB");
    }

    #[test]
    fn format_file_info_missing_file() {
        let info = format_file_info(Path::new("/nonexistent/photo.png"));
        assert!(info.contains("photo.png"));
        assert!(info.contains("unknown size"));
    }

    #[test]
    fn protocol_detection_does_not_panic() {
        let _protocol = detect_protocol();
    }

    #[test]
    fn render_error_line_contains_filename() {
        let lines = render_error_line(Path::new("/tmp/broken.png"));
        assert_eq!(lines.len(), 1);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("broken.png"));
    }

    #[test]
    fn render_preview_lines_graceful_on_missing_file() {
        let lines = render_preview_lines(Path::new("/nonexistent/missing.png"), 80);
        assert!(!lines.is_empty()); // Should produce fallback text, not crash
    }
}
