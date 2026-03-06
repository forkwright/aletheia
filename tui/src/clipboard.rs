/// Copy text to the system clipboard.
/// Tries arboard (native) first, falls back to OSC52 escape sequence.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    // Try native clipboard via arboard
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

/// OSC52 clipboard escape sequence — works over SSH, inside tmux/screen.
/// Supported by: iTerm2, Kitty, WezTerm, Alacritty, GNOME Terminal (VTE 0.76+).
fn copy_osc52(text: &str) -> Result<(), String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use std::io::Write;

    let encoded = STANDARD.encode(text.as_bytes());

    // Check if we're in tmux — need passthrough wrapper
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
