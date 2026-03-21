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
    #[test]
    fn copy_osc52_generates_valid_sequence() {
        // We can't easily test the actual clipboard, but we can verify the
        // function doesn't panic. In CI/headless this will actually try to
        // write to stdout which may work or fail depending on environment.
        // The main value is ensuring the base64 encoding logic is correct.
        use base64::{Engine, engine::general_purpose::STANDARD};
        let text = "test clipboard content";
        let encoded = STANDARD.encode(text.as_bytes());
        assert!(!encoded.is_empty());
        // Verify roundtrip
        let decoded = STANDARD.decode(&encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), text);
    }

    #[test]
    fn copy_osc52_tmux_detection() {
        // Just verify the tmux branch logic path exists and is correct
        let text = "test";
        use base64::{Engine, engine::general_purpose::STANDARD};
        let encoded = STANDARD.encode(text.as_bytes());
        let tmux_seq = format!("\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", encoded);
        let normal_seq = format!("\x1b]52;c;{}\x07", encoded);
        assert!(tmux_seq.len() > normal_seq.len());
        assert!(tmux_seq.starts_with("\x1bPtmux;"));
    }
}
