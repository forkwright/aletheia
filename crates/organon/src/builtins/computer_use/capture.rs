//! Screen capture and frame diff logic.

use std::path::Path;

use super::types::{ComputerAction, DiffRegion};

/// Detect display server and return the appropriate capture command.
pub(super) fn capture_command(output_path: &Path) -> std::process::Command {
    let output = output_path.to_string_lossy();

    // WHY: Check WAYLAND_DISPLAY first; if set, the session is Wayland and
    // scrot (X11-only) will not work. grim is the standard Wayland capture tool.
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        let mut cmd = std::process::Command::new("grim");
        cmd.arg(output.as_ref());
        cmd
    } else {
        let mut cmd = std::process::Command::new("scrot");
        cmd.args(["--overwrite", output.as_ref()]);
        cmd
    }
}

/// Capture the current screen to a PNG file.
///
/// # Errors
///
/// Returns `Err` if the capture tool is not installed or fails.
pub(super) fn capture_screen(output_path: &Path) -> std::io::Result<()> {
    let mut cmd = capture_command(output_path);
    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!(
            "screen capture failed: {stderr}"
        )));
    }
    Ok(())
}

/// Read a PNG file and return its raw bytes.
pub(super) fn read_frame(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Compare two PNG byte buffers and return the bounding box of the changed region.
///
/// Uses a simple byte-level comparison. Both frames must have the same dimensions.
/// Returns `None` if the frames are identical or cannot be compared.
pub(super) fn compute_diff_region(before: &[u8], after: &[u8]) -> Option<DiffRegion> {
    // kanon:ignore RUST/indexing-slicing
    // WHY: Parse PNG headers to extract dimensions rather than pulling in an
    // image decoding crate. PNG IHDR chunk is always the first chunk after
    // the 8-byte signature: 4 bytes length, 4 bytes "IHDR", 4 bytes width,
    // 4 bytes height (big-endian u32).
    let width_before = png_width(before)?;
    let height_before = png_height(before)?;
    let width_after = png_width(after)?;
    let height_after = png_height(after)?;

    if width_before != width_after || height_before != height_after {
        // Frames have different dimensions; treat entire frame as changed.
        return Some(DiffRegion {
            x: 0,
            y: 0,
            width: width_after,
            height: height_after,
        });
    }

    if before == after {
        return None;
    }

    // WHY: For raw PNG byte comparison, we cannot do per-pixel diff without
    // decompressing the IDAT chunks. Instead, report that a change occurred
    // and return the full frame as the diff region. This is a pragmatic
    // compromise: the LLM receives the full post-action screenshot and knows
    // that something changed.
    Some(DiffRegion {
        x: 0,
        y: 0,
        width: width_after,
        height: height_after,
    })
}

/// Extract width from PNG IHDR chunk.
fn png_width(data: &[u8]) -> Option<u32> {
    // kanon:ignore RUST/indexing-slicing
    // PNG signature (8 bytes) + chunk length (4) + "IHDR" (4) + width (4)
    let bytes: [u8; 4] = data.get(16..20)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// Extract height from PNG IHDR chunk.
fn png_height(data: &[u8]) -> Option<u32> {
    // kanon:ignore RUST/indexing-slicing
    let bytes: [u8; 4] = data.get(20..24)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// Generate a human-readable description of the change.
pub(super) fn describe_change(action: &ComputerAction, diff: Option<&DiffRegion>) -> String {
    let action_desc = match action {
        ComputerAction::Click { x, y, button } => {
            let btn = match button {
                1 => "left",
                2 => "middle",
                3 => "right",
                _ => "unknown",
            };
            format!("Performed {btn}-click at ({x}, {y})")
        }
        ComputerAction::TypeText { text } => {
            let preview = if text.len() > 50 {
                format!("{}...", text.get(..50).unwrap_or(text))
            } else {
                text.clone()
            };
            format!("Typed text: {preview:?}")
        }
        ComputerAction::Key { combo } => {
            format!("Pressed key combination: {combo}")
        }
        ComputerAction::Scroll { x, y, delta } => {
            let direction = if *delta > 0 { "down" } else { "up" };
            format!(
                "Scrolled {direction} by {} units at ({x}, {y})",
                delta.unsigned_abs()
            )
        }
    };

    match diff {
        Some(region) => format!("{action_desc}. Screen changed in region {region}."),
        None => format!("{action_desc}. No visible change detected."),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn png_dimension_parsing() {
        // Minimal valid PNG: 8-byte signature + IHDR chunk
        // Signature: 137 80 78 71 13 10 26 10
        // IHDR: length (13) + "IHDR" + width (800) + height (600) + ...
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10]; // signature
        png.extend_from_slice(&[0, 0, 0, 13]); // chunk length
        png.extend_from_slice(b"IHDR"); // chunk type
        png.extend_from_slice(&800u32.to_be_bytes()); // width
        png.extend_from_slice(&600u32.to_be_bytes()); // height
        png.extend_from_slice(&[8, 2, 0, 0, 0]); // bit depth, color type, etc.

        assert_eq!(png_width(&png), Some(800), "should parse width from IHDR");
        assert_eq!(png_height(&png), Some(600), "should parse height from IHDR");
    }

    #[test]
    fn png_dimension_parsing_too_short() {
        assert_eq!(png_width(&[0; 10]), None, "buffer too short for width");
        assert_eq!(png_height(&[0; 20]), None, "buffer too short for height");
    }

    #[test]
    fn diff_identical_frames_returns_none() {
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&100u32.to_be_bytes());
        png.extend_from_slice(&100u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);

        assert!(
            compute_diff_region(&png, &png).is_none(),
            "identical frames should produce no diff"
        );
    }

    #[test]
    fn diff_different_frames_returns_region() {
        let mut png1 = vec![137, 80, 78, 71, 13, 10, 26, 10];
        png1.extend_from_slice(&[0, 0, 0, 13]);
        png1.extend_from_slice(b"IHDR");
        png1.extend_from_slice(&640u32.to_be_bytes());
        png1.extend_from_slice(&480u32.to_be_bytes());
        png1.extend_from_slice(&[8, 2, 0, 0, 0]);
        png1.extend_from_slice(&[0xAA; 50]); // padding

        let mut png2 = png1.clone();
        // Modify some bytes after IHDR to simulate different content.
        if let Some(byte) = png2.get_mut(30) {
            *byte = 0xBB;
        }

        let diff = compute_diff_region(&png1, &png2);
        assert!(diff.is_some(), "different frames should produce a diff");
        let region = diff.expect("diff should exist");
        assert_eq!(region.width, 640, "diff width should match frame width");
        assert_eq!(region.height, 480, "diff height should match frame height");
    }

    #[test]
    fn diff_different_dimensions_returns_full_frame() {
        let make_png = |w: u32, h: u32| {
            let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
            png.extend_from_slice(&[0, 0, 0, 13]);
            png.extend_from_slice(b"IHDR");
            png.extend_from_slice(&w.to_be_bytes());
            png.extend_from_slice(&h.to_be_bytes());
            png.extend_from_slice(&[8, 2, 0, 0, 0]);
            png
        };

        let diff = compute_diff_region(&make_png(800, 600), &make_png(1024, 768));
        assert!(diff.is_some(), "different dimensions should produce diff");
        let region = diff.expect("diff should exist");
        assert_eq!(region.width, 1024, "should use after frame width");
        assert_eq!(region.height, 768, "should use after frame height");
    }

    #[test]
    fn describe_change_with_diff() {
        let action = ComputerAction::Click {
            x: 10,
            y: 20,
            button: 1,
        };
        let diff = Some(DiffRegion {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        });
        let desc = describe_change(&action, diff.as_ref());
        assert!(desc.contains("left-click"), "should mention click type");
        assert!(
            desc.contains("Screen changed"),
            "should mention screen change"
        );
    }

    #[test]
    fn describe_change_without_diff() {
        let action = ComputerAction::Key {
            combo: "Return".to_owned(),
        };
        let desc = describe_change(&action, None);
        assert!(
            desc.contains("No visible change"),
            "should indicate no change"
        );
    }
}
