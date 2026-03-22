mod agent;
mod chat;
mod command;
mod filter;
mod input;
pub mod memory;
pub mod notification;
pub(crate) mod ops;
mod overlay;
pub(crate) mod planning;
pub mod settings;
pub(crate) mod tab;
pub(crate) mod view_stack;
pub(crate) mod virtual_scroll;

pub use agent::{ActiveTool, AgentState, AgentStatus, ToolSummary};
pub(crate) use chat::{ArcVec, MarkdownCache, SavedScrollState};
pub use chat::{ChatMessage, ToolCallInfo};
pub use command::{CommandPaletteState, SelectionContext, SlashCompleteState, SlashSuggestion};
pub use filter::{FilterScope, FilterState};
pub use input::{InputState, TabCompletion};
pub use memory::MemoryInspectorState;
pub use notification::{ErrorBanner, NotificationStore, Toast};
pub use ops::{FocusedPane, OpsState};
pub use overlay::{
    ContextAction, ContextActionsOverlay, DecisionCardOverlay, DecisionField, DecisionOption,
    Overlay, PlanApprovalOverlay, PlanStepApproval, SearchResult, SearchResultKind,
    SessionPickerOverlay, SessionSearchOverlay, SubmittedDecision, ToolApprovalOverlay,
};
pub use planning::{PlanningDashboardState, RetrospectiveState};
pub(crate) use tab::TabBar;
pub use view_stack::{View, ViewStack};
