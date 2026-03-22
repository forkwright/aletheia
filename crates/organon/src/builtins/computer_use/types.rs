//! Core types for the computer use tool.

use serde::{Deserialize, Serialize};

/// Actions the computer use tool can perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum ComputerAction {
    /// Click at screen coordinates.
    Click {
        /// X coordinate in pixels.
        x: i32,
        /// Y coordinate in pixels.
        y: i32,
        /// Mouse button: 1 = left, 2 = middle, 3 = right.
        #[serde(default = "default_button")]
        button: u8,
    },
    /// Type text via simulated keystrokes.
    TypeText {
        /// The text to type.
        text: String,
    },
    /// Press a key combination.
    Key {
        /// Key combo string (e.g. "ctrl+c", "Return", "alt+Tab").
        combo: String,
    },
    /// Scroll at screen coordinates.
    Scroll {
        /// X coordinate in pixels.
        x: i32,
        /// Y coordinate in pixels.
        y: i32,
        /// Scroll delta: positive = down, negative = up.
        delta: i32,
    },
}

fn default_button() -> u8 {
    1
}

impl std::fmt::Display for ComputerAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Click { x, y, button } => write!(f, "click({x}, {y}, button={button})"),
            Self::TypeText { text } => write!(f, "type_text({text:?})"),
            Self::Key { combo } => write!(f, "key({combo})"),
            Self::Scroll { x, y, delta } => write!(f, "scroll({x}, {y}, delta={delta})"),
        }
    }
}

/// Bounding box for a changed region between two frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DiffRegion {
    /// Left edge in pixels.
    pub(crate) x: u32,
    /// Top edge in pixels.
    pub(crate) y: u32,
    /// Width in pixels.
    pub(crate) width: u32,
    /// Height in pixels.
    pub(crate) height: u32,
}

impl std::fmt::Display for DiffRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}) {}x{}", self.x, self.y, self.width, self.height)
    }
}

/// Structured result from a computer use action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ActionResult {
    /// Whether the action succeeded.
    pub(crate) success: bool,
    /// The action that was performed.
    pub(crate) action: String,
    /// Bounding box of the region that changed between frames.
    pub(crate) diff_region: Option<DiffRegion>,
    /// Human-readable description of what changed.
    pub(crate) change_description: String,
    /// Base64-encoded PNG of the post-action frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_base64: Option<String>,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn action_display_formatting() {
        let click = ComputerAction::Click {
            x: 100,
            y: 200,
            button: 1,
        };
        assert_eq!(click.to_string(), "click(100, 200, button=1)");

        let type_text = ComputerAction::TypeText {
            text: "hello".to_owned(),
        };
        assert_eq!(type_text.to_string(), "type_text(\"hello\")");

        let key = ComputerAction::Key {
            combo: "ctrl+c".to_owned(),
        };
        assert_eq!(key.to_string(), "key(ctrl+c)");

        let scroll = ComputerAction::Scroll {
            x: 50,
            y: 60,
            delta: -3,
        };
        assert_eq!(scroll.to_string(), "scroll(50, 60, delta=-3)");
    }

    #[test]
    fn diff_region_display() {
        let region = DiffRegion {
            x: 10,
            y: 20,
            width: 300,
            height: 400,
        };
        assert_eq!(region.to_string(), "(10, 20) 300x400");
    }

    #[test]
    fn action_result_serialization_roundtrip() {
        let result = ActionResult {
            success: true,
            action: "click(100, 200, button=1)".to_owned(),
            diff_region: Some(DiffRegion {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            }),
            change_description: "Performed left-click at (100, 200). Screen changed.".to_owned(),
            frame_base64: None,
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let roundtrip: ActionResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(roundtrip.success, result.success);
        assert_eq!(roundtrip.action, result.action);
        assert!(
            roundtrip.diff_region.is_some(),
            "diff_region should roundtrip"
        );
    }

    #[test]
    fn computer_action_serde_roundtrip() {
        let actions = vec![
            ComputerAction::Click {
                x: 100,
                y: 200,
                button: 1,
            },
            ComputerAction::TypeText {
                text: "hello world".to_owned(),
            },
            ComputerAction::Key {
                combo: "ctrl+shift+t".to_owned(),
            },
            ComputerAction::Scroll {
                x: 50,
                y: 60,
                delta: -5,
            },
        ];

        for action in &actions {
            let json = serde_json::to_string(action).expect("serialize action");
            let roundtrip: ComputerAction =
                serde_json::from_str(&json).expect("deserialize action");
            assert_eq!(
                action.to_string(),
                roundtrip.to_string(),
                "action should roundtrip"
            );
        }
    }
}
