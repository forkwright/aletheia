/// Event-to-Msg translation — maps terminal, SSE, and stream events to application messages.
use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind,
};

use crate::api::types::SseEvent;
use crate::app::App;
use crate::events::{Event, StreamEvent};
use crate::msg::{MessageActionKind, Msg, OverlayKind};
use crate::state::Overlay;

impl App {
    pub fn map_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Terminal(term_event) => self.map_terminal(term_event),
            Event::Sse(sse_event) => Some(self.map_sse(sse_event)),
            Event::Stream(stream_event) => Some(self.map_stream(stream_event)),
            Event::Tick => Some(Msg::Tick),
        }
    }

    fn map_terminal(&self, event: TermEvent) -> Option<Msg> {
        match event {
            TermEvent::Key(key) => self.map_key(key),
            TermEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => Some(Msg::ScrollUp),
                MouseEventKind::ScrollDown => Some(Msg::ScrollDown),
                MouseEventKind::Down(MouseButton::Left) => {
                    let sidebar = crate::view::SIDEBAR_RECT.load_rect();
                    if sidebar.width > 0
                        && mouse.column < sidebar.x + sidebar.width
                        && mouse.row >= sidebar.y
                    {
                        let mut y = sidebar.y + 1;
                        for agent in &self.agents {
                            let row_count = if agent.active_tool.is_some()
                                || agent.compaction_stage.is_some()
                            {
                                2u16
                            } else {
                                1
                            };
                            if mouse.row >= y && mouse.row < y + row_count {
                                return Some(Msg::FocusAgent(agent.id.clone()));
                            }
                            y += row_count;
                        }
                    }
                    None
                }
                _ => None,
            },
            TermEvent::Resize(w, h) => Some(Msg::Resize(w, h)),
            _ => None,
        }
    }

    fn map_key(&self, key: KeyEvent) -> Option<Msg> {
        if self.overlay.is_some() {
            return self.map_overlay_key(key);
        }

        if self.command_palette.active {
            return self.map_palette_key(key);
        }

        if self.filter.editing {
            return self.map_filter_editing_key(key);
        }

        if self.filter.active
            && let Some(msg) = self.map_filter_applied_key(key)
        {
            return Some(msg);
        }

        // Selection mode — single-letter keys become actions
        if self.selected_message.is_some() {
            return self.map_selection_key(key);
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(Msg::Quit),

            (KeyModifiers::CONTROL, KeyCode::Char('f')) => Some(Msg::ToggleSidebar),
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => Some(Msg::ToggleThinking),

            (_, KeyCode::F(1)) => Some(Msg::OpenOverlay(OverlayKind::Help)),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                Some(Msg::OpenOverlay(OverlayKind::AgentPicker))
            }
            (KeyModifiers::CONTROL, KeyCode::Char('i')) => {
                Some(Msg::OpenOverlay(OverlayKind::SystemStatus))
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => Some(Msg::NewSession),

            (_, KeyCode::Tab) => {
                if self.input.text.contains('@') {
                    Some(Msg::CharInput('\t'))
                } else {
                    None
                }
            }

            (_, KeyCode::PageUp) => Some(Msg::ScrollPageUp),
            (_, KeyCode::PageDown) => Some(Msg::ScrollPageDown),
            (KeyModifiers::SHIFT, KeyCode::Up) => Some(Msg::ScrollUp),
            (KeyModifiers::SHIFT, KeyCode::Down) => Some(Msg::ScrollDown),

            (_, KeyCode::Enter) => Some(Msg::Submit),
            (_, KeyCode::Backspace) => Some(Msg::Backspace),
            (_, KeyCode::Delete) => Some(Msg::Delete),
            (_, KeyCode::Left) => Some(Msg::CursorLeft),
            (_, KeyCode::Right) => Some(Msg::CursorRight),
            (_, KeyCode::Home) => Some(Msg::CursorHome),
            (_, KeyCode::End) => Some(Msg::CursorEnd),

            // Up/Down with empty input enters selection mode; otherwise history nav
            (_, KeyCode::Up) if self.input.text.is_empty() && !self.messages.is_empty() => {
                Some(Msg::SelectPrev)
            }
            (_, KeyCode::Down) if self.input.text.is_empty() && !self.messages.is_empty() => {
                Some(Msg::SelectNext)
            }
            (_, KeyCode::Up) => Some(Msg::HistoryUp),
            (_, KeyCode::Down) => Some(Msg::HistoryDown),

            (KeyModifiers::CONTROL, KeyCode::Char('w')) => Some(Msg::DeleteWord),
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => Some(Msg::ClearLine),
            (KeyModifiers::CONTROL, KeyCode::Char('y')) => Some(Msg::CopyLastResponse),
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => Some(Msg::ComposeInEditor),

            (KeyModifiers::NONE, KeyCode::Char('?')) if self.input.text.is_empty() => {
                Some(Msg::OpenOverlay(OverlayKind::Help))
            }

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(':'))
                if self.input.text.is_empty() =>
            {
                Some(Msg::CommandPaletteOpen)
            }

            (KeyModifiers::NONE, KeyCode::Char('/')) if self.input.text.is_empty() => {
                Some(Msg::FilterOpen)
            }

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::CharInput(c))
            }

            _ => None,
        }
    }

    fn map_selection_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            // Ctrl combos pass through to global handlers
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(Msg::Quit),
            (KeyModifiers::CONTROL, KeyCode::Char('f')) => Some(Msg::ToggleSidebar),
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => Some(Msg::ToggleThinking),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                Some(Msg::OpenOverlay(OverlayKind::AgentPicker))
            }
            (KeyModifiers::CONTROL, KeyCode::Char('i')) => {
                Some(Msg::OpenOverlay(OverlayKind::SystemStatus))
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => Some(Msg::NewSession),

            // Shift+Up/Down scroll (before bare Up/Down)
            (KeyModifiers::SHIFT, KeyCode::Up) => Some(Msg::ScrollUp),
            (KeyModifiers::SHIFT, KeyCode::Down) => Some(Msg::ScrollDown),

            // Navigation
            (_, KeyCode::Char('j')) | (_, KeyCode::Down) => Some(Msg::SelectNext),
            (_, KeyCode::Char('k')) | (_, KeyCode::Up) => Some(Msg::SelectPrev),
            (_, KeyCode::Esc) => Some(Msg::DeselectMessage),
            (_, KeyCode::Home) => Some(Msg::SelectFirst),
            (_, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
                Some(Msg::SelectLast)
            }

            // Actions
            (KeyModifiers::NONE, KeyCode::Char('c')) => {
                Some(Msg::MessageAction(MessageActionKind::Copy))
            }
            (KeyModifiers::NONE, KeyCode::Char('y')) => {
                Some(Msg::MessageAction(MessageActionKind::YankCodeBlock))
            }
            (KeyModifiers::NONE, KeyCode::Char('e')) => {
                Some(Msg::MessageAction(MessageActionKind::Edit))
            }
            (KeyModifiers::NONE, KeyCode::Char('d')) => {
                Some(Msg::MessageAction(MessageActionKind::Delete))
            }
            (KeyModifiers::NONE, KeyCode::Char('o')) => {
                Some(Msg::MessageAction(MessageActionKind::OpenLinks))
            }
            (KeyModifiers::NONE, KeyCode::Char('i')) => {
                Some(Msg::MessageAction(MessageActionKind::Inspect))
            }

            (_, KeyCode::PageUp) => Some(Msg::ScrollPageUp),
            (_, KeyCode::PageDown) => Some(Msg::ScrollPageDown),
            (_, KeyCode::F(1)) => Some(Msg::OpenOverlay(OverlayKind::Help)),

            // Any other character deselects and inserts into input
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::CharInput(c))
            }

            _ => None,
        }
    }

    fn map_filter_editing_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::Esc) => {
                Some(Msg::FilterClose)
            }
            (_, KeyCode::Enter) => Some(Msg::FilterConfirm),
            (_, KeyCode::Backspace) => {
                if self.filter.text.is_empty() {
                    Some(Msg::FilterClose)
                } else {
                    Some(Msg::FilterBackspace)
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => Some(Msg::FilterClear),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::FilterInput(c))
            }
            _ => None,
        }
    }

    fn map_filter_applied_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => Some(Msg::FilterClose),
            (KeyModifiers::NONE, KeyCode::Char('/')) if self.input.text.is_empty() => {
                Some(Msg::FilterOpen)
            }
            (KeyModifiers::NONE, KeyCode::Char('n')) if self.input.text.is_empty() => {
                Some(Msg::FilterNextMatch)
            }
            (KeyModifiers::SHIFT, KeyCode::Char('N')) => Some(Msg::FilterPrevMatch),
            _ => None,
        }
    }

    fn map_palette_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => Some(Msg::CommandPaletteClose),
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(Msg::CommandPaletteClose),
            (_, KeyCode::Enter) => Some(Msg::CommandPaletteSelect),
            (_, KeyCode::Tab) => Some(Msg::CommandPaletteTab),
            (_, KeyCode::Up) => Some(Msg::CommandPaletteUp),
            (_, KeyCode::Down) => Some(Msg::CommandPaletteDown),
            (_, KeyCode::Backspace) => Some(Msg::CommandPaletteBackspace),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => Some(Msg::CommandPaletteDeleteWord),
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => Some(Msg::CommandPaletteClose),

            (KeyModifiers::NONE, KeyCode::Char('?')) if self.command_palette.input.is_empty() => {
                Some(Msg::OpenOverlay(OverlayKind::Help))
            }

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::CommandPaletteInput(c))
            }
            _ => None,
        }
    }

    fn map_overlay_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => Some(Msg::CloseOverlay),
            (_, KeyCode::Up) => Some(Msg::OverlayUp),
            (_, KeyCode::Down) => Some(Msg::OverlayDown),
            (_, KeyCode::Enter) => Some(Msg::OverlaySelect),

            (_, KeyCode::Char('a' | 'A')) if self.is_tool_approval_overlay() => {
                Some(Msg::OverlaySelect)
            }
            (_, KeyCode::Char('d' | 'D')) if self.is_tool_approval_overlay() => {
                Some(Msg::CloseOverlay)
            }

            (_, KeyCode::Char(' ')) if self.is_plan_approval_overlay() => {
                Some(Msg::OverlaySelect)
            }
            (_, KeyCode::Char('a' | 'A')) if self.is_plan_approval_overlay() => {
                Some(Msg::OverlaySelect)
            }
            (_, KeyCode::Char('c' | 'C')) if self.is_plan_approval_overlay() => {
                Some(Msg::CloseOverlay)
            }

            _ => None,
        }
    }

    pub(crate) fn is_tool_approval_overlay(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::ToolApproval(_)))
    }

    fn is_plan_approval_overlay(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::PlanApproval(_)))
    }

    fn map_sse(&self, event: SseEvent) -> Msg {
        match event {
            SseEvent::Connected => Msg::SseConnected,
            SseEvent::Disconnected => Msg::SseDisconnected,
            SseEvent::Init { active_turns } => Msg::SseInit { active_turns },
            SseEvent::TurnBefore {
                nous_id,
                session_id,
                turn_id,
            } => Msg::SseTurnBefore {
                nous_id,
                session_id,
                turn_id,
            },
            SseEvent::TurnAfter {
                nous_id,
                session_id,
            } => Msg::SseTurnAfter {
                nous_id,
                session_id,
            },
            SseEvent::ToolCalled { nous_id, tool_name } => {
                Msg::SseToolCalled { nous_id, tool_name }
            }
            SseEvent::ToolFailed {
                nous_id,
                tool_name,
                error,
            } => Msg::SseToolFailed {
                nous_id,
                tool_name,
                error,
            },
            SseEvent::StatusUpdate { nous_id, status } => {
                Msg::SseStatusUpdate { nous_id, status }
            }
            SseEvent::SessionCreated {
                nous_id,
                session_id,
            } => Msg::SseSessionCreated {
                nous_id,
                session_id,
            },
            SseEvent::SessionArchived {
                nous_id,
                session_id,
            } => Msg::SseSessionArchived {
                nous_id,
                session_id,
            },
            SseEvent::DistillBefore { nous_id } => Msg::SseDistillBefore { nous_id },
            SseEvent::DistillStage { nous_id, stage } => Msg::SseDistillStage { nous_id, stage },
            SseEvent::DistillAfter { nous_id } => Msg::SseDistillAfter { nous_id },
            SseEvent::Ping => Msg::Tick,
        }
    }

    fn map_stream(&self, event: StreamEvent) -> Msg {
        match event {
            StreamEvent::TurnStart {
                session_id,
                nous_id,
                turn_id,
            } => Msg::StreamTurnStart {
                session_id,
                nous_id,
                turn_id,
            },
            StreamEvent::TextDelta(text) => Msg::StreamTextDelta(text),
            StreamEvent::ThinkingDelta(text) => Msg::StreamThinkingDelta(text),
            StreamEvent::ToolStart { tool_name, tool_id } => {
                Msg::StreamToolStart { tool_name, tool_id }
            }
            StreamEvent::ToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
            } => Msg::StreamToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
            },
            StreamEvent::ToolApprovalRequired {
                turn_id,
                tool_name,
                tool_id,
                input,
                risk,
                reason,
            } => Msg::StreamToolApprovalRequired {
                turn_id,
                tool_name,
                tool_id,
                input,
                risk,
                reason,
            },
            StreamEvent::ToolApprovalResolved { tool_id, decision } => {
                Msg::StreamToolApprovalResolved { tool_id, decision }
            }
            StreamEvent::PlanProposed { plan } => Msg::StreamPlanProposed { plan },
            StreamEvent::PlanStepStart { plan_id, step_id } => {
                Msg::StreamPlanStepStart { plan_id, step_id }
            }
            StreamEvent::PlanStepComplete {
                plan_id,
                step_id,
                status,
            } => Msg::StreamPlanStepComplete {
                plan_id,
                step_id,
                status,
            },
            StreamEvent::PlanComplete { plan_id, status } => {
                Msg::StreamPlanComplete { plan_id, status }
            }
            StreamEvent::TurnComplete { outcome } => Msg::StreamTurnComplete { outcome },
            StreamEvent::TurnAbort { reason } => Msg::StreamTurnAbort { reason },
            StreamEvent::Error(msg) => Msg::StreamError(msg),
        }
    }
}
