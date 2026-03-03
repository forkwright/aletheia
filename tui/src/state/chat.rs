#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub name: String,
    pub duration_ms: Option<u64>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SavedScrollState {
    pub(crate) scroll_offset: usize,
    pub(crate) auto_scroll: bool,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub is_streaming: bool,
    pub tool_calls: Vec<ToolCallInfo>,
}
