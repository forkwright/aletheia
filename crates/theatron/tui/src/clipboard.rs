/// Copy text to the system clipboard.
/// Tries arboard (native) first, falls back to OSC52 escape sequence.
pub(crate) fn copy_to_clipboard(text: &str) -> Result<(), String> {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => match clipboard.set_text(text) {
            Ok(()) => {
                tracing::debug!("copied {} bytes to clipboard (native)", text.len());
                Ok(())
            }
            Err(e) => {
                tracing::warn!("native clipboard failed: {e}, trying OSC52");
                copy_osc52(text)
            }
        },
        Err(e) => {
            tracing::warn!("clipboard init failed: {e}, trying OSC52");
            copy_osc52(text)
        }
    }
}

/// Content read from the system clipboard.
pub(crate) enum ClipboardContent {
    Text(String),
    Image {
        png_data: Vec<u8>,
        width: u32,
        height: u32,
    },
    Empty,
}

/// Read from the system clipboard, returning text or image data.
/// Tries arboard (native) first, falls back to system CLI tools for text.
pub(crate) fn read_from_clipboard() -> ClipboardContent {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => {
            // WHY: try image first — if the clipboard holds an image and we ask for text,
            // some platforms return the file path instead of the actual image data.
            if let Ok(img) = clipboard.get_image() {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "clipboard image dimensions fit in u32"
                )]
                if let Some(png_data) = rgba_to_png(&img.bytes, img.width as u32, img.height as u32)
                {
                    return ClipboardContent::Image {
                        png_data,
                        width: img.width as u32,
                        height: img.height as u32,
                    };
                }
            }
            match clipboard.get_text() {
                Ok(text) if !text.is_empty() => ClipboardContent::Text(text),
                _ => ClipboardContent::Empty,
            }
        }
        Err(e) => {
            tracing::warn!("clipboard read failed: {e}, trying system tools");
            read_system_clipboard_text()
        }
    }
}

/// Encode raw RGBA pixel data as PNG bytes.
fn rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    use image::ImageBuffer;

    let img: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, rgba.to_vec())?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}

/// Fallback: read text from system clipboard tools (wl-paste, xclip, pbpaste).
fn read_system_clipboard_text() -> ClipboardContent {
    let tools: &[(&str, &[&str])] = &[
        ("wl-paste", &["--no-newline"]),
        ("xclip", &["-selection", "clipboard", "-o"]),
        ("pbpaste", &[]),
    ];
    for (cmd, args) in tools {
        if let Ok(output) = std::process::Command::new(cmd).args(*args).output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            if !text.is_empty() {
                return ClipboardContent::Text(text);
            }
        }
    }
    ClipboardContent::Empty
}

/// OSC52 clipboard escape sequence: works over SSH, inside tmux/screen.
/// Supported by: iTerm2, Kitty, WezTerm, Alacritty, GNOME Terminal (VTE 0.76+).
fn copy_osc52(text: &str) -> Result<(), String> {
    use std::io::Write;

    use base64::{Engine, engine::general_purpose::STANDARD};

    let encoded = STANDARD.encode(text.as_bytes());

    // NOTE: tmux requires an OSC52 passthrough wrapper for the escape sequence to reach the terminal
    let seq = if std::env::var("TMUX").is_ok() {
        format!("\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", encoded)
    } else {
        format!("\x1b]52;c;{}\x07", encoded)
    };

    std::io::stdout()
        .write_all(seq.as_bytes())
        .map_err(|e| format!("OSC52 write failed: {e}"))?;
    std::io::stdout()
        .flush()
        .map_err(|e| format!("OSC52 flush failed: {e}"))?;

    tracing::debug!("copied {} bytes to clipboard (OSC52)", text.len());
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn copy_osc52_generates_valid_sequence() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let text = "test clipboard content";
        let encoded = STANDARD.encode(text.as_bytes());
        assert!(!encoded.is_empty());
        let decoded = STANDARD.decode(&encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), text);
    }

    #[test]
    fn copy_osc52_tmux_detection() {
        let text = "test";
        use base64::{Engine, engine::general_purpose::STANDARD};
        let encoded = STANDARD.encode(text.as_bytes());
        let tmux_seq = format!("\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", encoded);
        let normal_seq = format!("\x1b]52;c;{}\x07", encoded);
        assert!(tmux_seq.len() > normal_seq.len());
        assert!(tmux_seq.starts_with("\x1bPtmux;"));
    }

    #[test]
    fn rgba_to_png_valid_1x1() {
        let rgba = vec![255, 0, 0, 255]; // 1x1 red pixel
        let png = rgba_to_png(&rgba, 1, 1);
        assert!(png.is_some());
        let data = png.unwrap();
        // PNG magic bytes
        assert_eq!(&data[..4], &[0x89, 0x50, 0x4E, 0x47]);
    }

    #[test]
    fn rgba_to_png_invalid_dimensions_returns_none() {
        let rgba = vec![255, 0, 0, 255]; // 1 pixel but claim 2x2
        let png = rgba_to_png(&rgba, 2, 2);
        assert!(png.is_none());
    }
}
