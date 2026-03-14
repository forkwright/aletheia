mod agent;
mod chat;
mod command;
mod filter;
mod input;
pub mod memory;
pub(crate) mod ops;
mod overlay;
pub mod settings;
pub(crate) mod tab;
pub(crate) mod view_stack;
pub(crate) mod virtual_scroll;

pub use agent::{ActiveTool, AgentState, AgentStatus};
pub(crate) use chat::{ArcVec, SavedScrollState};
pub use chat::{ChatMessage, ToolCallInfo};
pub use command::{CommandPaletteState, SelectionContext};
pub use filter::{FilterScope, FilterState};
pub use input::{InputState, TabCompletion};
pub use memory::MemoryInspectorState;
pub use ops::{FocusedPane, OpsState};
pub use overlay::{
    ContextAction, ContextActionsOverlay, Overlay, PlanApprovalOverlay, PlanStepApproval,
    SessionPickerOverlay, ToolApprovalOverlay,
};
pub(crate) use tab::TabBar;
pub use view_stack::{View, ViewStack};
