mod agent;
mod chat;
mod command;
mod filter;
mod input;
pub(crate) mod ops;
mod overlay;
pub mod settings;
pub(crate) mod view_stack;
pub(crate) mod virtual_scroll;

pub use agent::{AgentState, AgentStatus};
pub(crate) use chat::SavedScrollState;
pub use chat::{ChatMessage, ToolCallInfo};
pub use command::{CommandPaletteState, SelectionContext};
pub use filter::{FilterScope, FilterState};
pub use input::{InputState, TabCompletion};
pub use ops::{FocusedPane, OpsState};
pub use overlay::{
    ContextAction, ContextActionsOverlay, Overlay, PlanApprovalOverlay, PlanStepApproval,
    SessionPickerOverlay, ToolApprovalOverlay,
};
pub use view_stack::{View, ViewStack};
