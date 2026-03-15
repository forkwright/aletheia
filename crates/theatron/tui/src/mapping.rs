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

        // WHY: g-prefix must intercept before normal key routing so gt/gT are
        // treated as two-key sequences; after 'g' is consumed, the second char falls through.
        if self.pending_g {
            return match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('t')) => Some(Msg::TabNext),
                (KeyModifiers::SHIFT, KeyCode::Char('T')) => Some(Msg::TabPrev),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    Some(Msg::CharInput(c))
                }
                _ => None,
            };
        }

        if self.is_memory_view() {
            return self.map_memory_key(key);
        }

        // WHY: ViewPopBack takes priority over DeselectMessage so Esc always unwinds
        // the view stack before affecting selection state.
        if !self.view_stack.is_home() && matches!((key.modifiers, key.code), (_, KeyCode::Esc)) {
            return Some(Msg::ViewPopBack);
        }

        if self.selected_message.is_some() {
            return self.map_selection_key(key);
        }

        if self.ops.visible && self.ops.focused_pane == crate::state::FocusedPane::Operations {
            return self.map_ops_pane_key(key);
        }

        // Configurable keymap — covers global Ctrl+key shortcuts.
        if let Some(action) = self.keymap.lookup(key.modifiers, key.code) {
            return Some(action.to_msg());
        }

        match (key.modifiers, key.code) {
            // Context-dependent Ctrl+W: TabClose when input is empty, DeleteWord otherwise.
            (KeyModifiers::CONTROL, KeyCode::Char('w')) if self.input.text.is_empty() => {
                Some(Msg::TabClose)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => Some(Msg::DeleteWord),

            (_, KeyCode::Tab) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Msg::TabNext)
            }
            (_, KeyCode::BackTab) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Msg::TabPrev)
            }

            (KeyModifiers::ALT, KeyCode::Char(c @ '1'..='9')) => {
                let n = (c as usize) - ('1' as usize);
                Some(Msg::TabJump(n))
            }

            (_, KeyCode::Tab) => {
                if self.input.text.contains('@') {
                    Some(Msg::CharInput('\t'))
                } else if self.ops.visible {
                    Some(Msg::OpsFocusSwitch)
                } else {
                    Some(Msg::NextAgent)
                }
            }
            (_, KeyCode::BackTab) => Some(Msg::PrevAgent),

            (_, KeyCode::Enter) => Some(Msg::Submit),
            (_, KeyCode::Backspace) => Some(Msg::Backspace),
            (_, KeyCode::Delete) => Some(Msg::Delete),
            (_, KeyCode::Left) => Some(Msg::CursorLeft),
            (_, KeyCode::Right) => Some(Msg::CursorRight),
            (_, KeyCode::Home) => Some(Msg::CursorHome),
            (_, KeyCode::End) if self.input.text.is_empty() => Some(Msg::ScrollToBottom),
            (_, KeyCode::End) => Some(Msg::CursorEnd),

            // WHY: Up/Down with empty input enters selection mode rather than history nav,
            // matching the modal editing convention where arrow keys in read state navigate messages.
            (_, KeyCode::Up) if self.input.text.is_empty() && !self.messages.is_empty() => {
                Some(Msg::SelectPrev)
            }
            (_, KeyCode::Down) if self.input.text.is_empty() && !self.messages.is_empty() => {
                Some(Msg::SelectNext)
            }
            (_, KeyCode::Up) => Some(Msg::HistoryUp),
            (_, KeyCode::Down) => Some(Msg::HistoryDown),

            (KeyModifiers::NONE, KeyCode::Char('?')) if self.input.text.is_empty() => {
                Some(Msg::OpenOverlay(OverlayKind::Help))
            }

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(':'))
                if self.input.text.is_empty() =>
            {
                Some(Msg::CommandPaletteOpen)
            }

            (KeyModifiers::NONE, KeyCode::Char('/')) if self.input.text.is_empty() => {
                Some(Msg::SessionSearchOpen)
            }

            (KeyModifiers::NONE, KeyCode::Char('v'))
                if self.input.text.is_empty() && !self.messages.is_empty() =>
            {
                Some(Msg::SelectPrev)
            }

            (KeyModifiers::NONE, KeyCode::Char('g'))
                if self.input.text.is_empty() && self.tab_bar.len() > 1 =>
            {
                Some(Msg::GPrefix)
            }

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => Some(Msg::CharInput(c)),

            _ => None,
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "method on App for consistency with other map_ methods"
    )]
    fn map_ops_pane_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(Msg::Quit),
            (KeyModifiers::CONTROL, KeyCode::Char('o')) => Some(Msg::ToggleOpsPane),
            (_, KeyCode::Tab) => Some(Msg::OpsFocusSwitch),
            (_, KeyCode::Esc) => Some(Msg::OpsFocusSwitch),
            (_, KeyCode::PageUp) => Some(Msg::OpsScrollUp),
            (_, KeyCode::PageDown) => Some(Msg::OpsScrollDown),
            (_, KeyCode::Char('j')) | (_, KeyCode::Down) => Some(Msg::OpsSelectNext),
            (_, KeyCode::Char('k')) | (_, KeyCode::Up) => Some(Msg::OpsSelectPrev),
            (_, KeyCode::Enter) => Some(Msg::OpsToggleExpand),
            _ => None,
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "consistent method signature; self needed for future key binding personalisation"
    )]
    fn map_selection_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(Msg::Quit),
            (KeyModifiers::CONTROL, KeyCode::Char('f')) => Some(Msg::ToggleSidebar),
            (KeyModifiers::CONTROL, KeyCode::Char('b')) => Some(Msg::ToggleThinking),
            (KeyModifiers::CONTROL, KeyCode::Char('o')) => Some(Msg::ToggleOpsPane),
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => Some(Msg::TabNew),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => Some(Msg::TabClose),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                Some(Msg::OpenOverlay(OverlayKind::AgentPicker))
            }
            (KeyModifiers::CONTROL, KeyCode::Char('i')) => {
                Some(Msg::OpenOverlay(OverlayKind::SystemStatus))
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => Some(Msg::NewSession),
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                Some(Msg::OpenOverlay(OverlayKind::SessionPicker))
            }

            (KeyModifiers::SHIFT, KeyCode::Up) => Some(Msg::ScrollUp),
            (KeyModifiers::SHIFT, KeyCode::Down) => Some(Msg::ScrollDown),

            (_, KeyCode::Char('j')) | (_, KeyCode::Down) => Some(Msg::SelectNext),
            (_, KeyCode::Char('k')) | (_, KeyCode::Up) => Some(Msg::SelectPrev),
            (_, KeyCode::Esc) => Some(Msg::DeselectMessage),
            (_, KeyCode::Enter) => Some(Msg::ViewDrillIn),
            (_, KeyCode::Home) => Some(Msg::SelectFirst),
            (_, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => Some(Msg::SelectLast),

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

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => Some(Msg::CharInput(c)),

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

    fn is_memory_view(&self) -> bool {
        matches!(
            self.view_stack.current(),
            crate::state::view_stack::View::MemoryInspector
                | crate::state::view_stack::View::FactDetail { .. }
        )
    }

    fn map_memory_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => return Some(Msg::Quit),
            (KeyModifiers::CONTROL, KeyCode::Char('f')) => return Some(Msg::ToggleSidebar),
            _ => {}
        }

        if self.memory.editing_confidence {
            return match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => Some(Msg::MemoryConfidenceSubmit),
                (_, KeyCode::Esc) => Some(Msg::MemoryConfidenceCancel),
                (_, KeyCode::Backspace) => Some(Msg::MemoryConfidenceBackspace),
                (KeyModifiers::NONE, KeyCode::Char(c)) => Some(Msg::MemoryConfidenceInput(c)),
                _ => None,
            };
        }

        if self.memory.search_active {
            return match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => Some(Msg::MemorySearchSubmit),
                (_, KeyCode::Esc) => Some(Msg::MemorySearchClose),
                (_, KeyCode::Backspace) => Some(Msg::MemorySearchBackspace),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    Some(Msg::MemorySearchInput(c))
                }
                _ => None,
            };
        }

        if self.memory.filter_editing {
            return match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => Some(Msg::MemoryFilterClose),
                (_, KeyCode::Enter) => Some(Msg::MemoryFilterClose),
                (_, KeyCode::Backspace) => Some(Msg::MemoryFilterBackspace),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    Some(Msg::MemoryFilterInput(c))
                }
                _ => None,
            };
        }

        if matches!(
            self.view_stack.current(),
            crate::state::view_stack::View::FactDetail { .. }
        ) {
            return match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => Some(Msg::MemoryPopBack),
                (KeyModifiers::NONE, KeyCode::Char('e')) => Some(Msg::MemoryEditConfidence),
                (KeyModifiers::NONE, KeyCode::Char('d')) => Some(Msg::MemoryForget),
                (KeyModifiers::NONE, KeyCode::Char('r')) => Some(Msg::MemoryRestore),
                _ => None,
            };
        }

        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => Some(Msg::MemoryClose),
            (_, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                Some(Msg::MemorySelectUp)
            }
            (_, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                Some(Msg::MemorySelectDown)
            }
            (_, KeyCode::Home) => Some(Msg::MemorySelectFirst),
            (_, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
                Some(Msg::MemorySelectLast)
            }
            (_, KeyCode::PageUp) => Some(Msg::MemoryPageUp),
            (_, KeyCode::PageDown) => Some(Msg::MemoryPageDown),
            (_, KeyCode::Enter) => Some(Msg::MemoryDrillIn),
            (KeyModifiers::NONE, KeyCode::Char('s')) => Some(Msg::MemorySortCycle),
            (KeyModifiers::NONE, KeyCode::Char('f')) => Some(Msg::MemoryFilterOpen),
            (KeyModifiers::NONE, KeyCode::Char('/')) => Some(Msg::MemorySearchOpen),
            (KeyModifiers::NONE, KeyCode::Char('d')) => Some(Msg::MemoryForget),
            (KeyModifiers::NONE, KeyCode::Char('r')) => Some(Msg::MemoryRestore),
            (KeyModifiers::NONE, KeyCode::Char('e')) => Some(Msg::MemoryEditConfidence),
            (_, KeyCode::Tab) => Some(Msg::MemoryTabNext),
            (KeyModifiers::SHIFT, KeyCode::BackTab) => Some(Msg::MemoryTabPrev),
            _ => None,
        }
    }

    fn is_context_actions_overlay(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::ContextActions(_)))
    }

    fn is_session_picker_overlay(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::SessionPicker(_)))
    }

    fn is_diff_view_overlay(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::DiffView(_)))
    }

    fn map_overlay_key(&self, key: KeyEvent) -> Option<Msg> {
        if matches!(&self.overlay, Some(Overlay::Settings(_))) {
            return self.map_settings_overlay_key(key);
        }

        if matches!(&self.overlay, Some(Overlay::SessionSearch(_))) {
            return self.map_session_search_key(key);
        }

        if self.is_diff_view_overlay() {
            return match (key.modifiers, key.code) {
                (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                    Some(Msg::DiffClose)
                }
                (_, KeyCode::Char('m')) => Some(Msg::DiffCycleMode),
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) => Some(Msg::DiffScrollUp),
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => Some(Msg::DiffScrollDown),
                (_, KeyCode::PageUp) => Some(Msg::DiffPageUp),
                (_, KeyCode::PageDown) => Some(Msg::DiffPageDown),
                (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(Msg::Quit),
                _ => None,
            };
        }

        // WHY: `?` toggles help overlay — pressing it again closes it.
        if matches!(&self.overlay, Some(Overlay::Help))
            && matches!(
                (key.modifiers, key.code),
                (KeyModifiers::NONE, KeyCode::Char('?'))
            )
        {
            return Some(Msg::CloseOverlay);
        }

        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => Some(Msg::CloseOverlay),
            (_, KeyCode::Up) => Some(Msg::OverlayUp),
            (_, KeyCode::Down) => Some(Msg::OverlayDown),
            (_, KeyCode::Enter) => Some(Msg::OverlaySelect),

            (_, KeyCode::Char('j')) if self.is_context_actions_overlay() => Some(Msg::OverlayDown),
            (_, KeyCode::Char('k')) if self.is_context_actions_overlay() => Some(Msg::OverlayUp),

            (_, KeyCode::Char('n')) if self.is_session_picker_overlay() => {
                Some(Msg::SessionPickerNewSession)
            }
            (_, KeyCode::Char('d')) if self.is_session_picker_overlay() => {
                Some(Msg::SessionPickerArchive)
            }

            (_, KeyCode::Char('a' | 'A')) if self.is_tool_approval_overlay() => {
                Some(Msg::OverlaySelect)
            }
            (_, KeyCode::Char('d' | 'D')) if self.is_tool_approval_overlay() => {
                Some(Msg::CloseOverlay)
            }

            (_, KeyCode::Char(' ')) if self.is_plan_approval_overlay() => Some(Msg::OverlaySelect),
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

    #[expect(
        clippy::unused_self,
        reason = "consistent method signature; self needed for future key binding personalisation"
    )]
    fn map_session_search_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                Some(Msg::SessionSearchClose)
            }
            (_, KeyCode::Enter) => Some(Msg::SessionSearchSelect),
            (_, KeyCode::Up) => Some(Msg::SessionSearchUp),
            (_, KeyCode::Down) => Some(Msg::SessionSearchDown),
            (_, KeyCode::Backspace) => Some(Msg::SessionSearchBackspace),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::SessionSearchInput(c))
            }
            _ => None,
        }
    }

    fn map_settings_overlay_key(&self, key: KeyEvent) -> Option<Msg> {
        let editing = matches!(
            &self.overlay,
            Some(Overlay::Settings(s)) if s.editing.is_some()
        );

        if editing {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => Some(Msg::CloseOverlay),
                (_, KeyCode::Enter) => Some(Msg::OverlaySelect),
                (_, KeyCode::Backspace) => Some(Msg::OverlayFilterBackspace),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    Some(Msg::OverlayFilter(c))
                }
                _ => None,
            }
        } else {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => Some(Msg::CloseOverlay),
                (_, KeyCode::Up) => Some(Msg::OverlayUp),
                (_, KeyCode::Down) => Some(Msg::OverlayDown),
                (_, KeyCode::Enter) => Some(Msg::OverlaySelect),
                (_, KeyCode::Char('s' | 'S')) => Some(Msg::OverlayFilter('s')),
                (_, KeyCode::Char('r' | 'R')) => Some(Msg::OverlayFilter('r')),
                _ => None,
            }
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "consistent method signature for event mapping interface"
    )]
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
            SseEvent::StatusUpdate { nous_id, status } => Msg::SseStatusUpdate { nous_id, status },
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

    #[expect(
        clippy::unused_self,
        reason = "consistent method signature for event mapping interface"
    )]
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
            StreamEvent::ToolStart {
                tool_name,
                tool_id,
                input,
            } => Msg::StreamToolStart {
                tool_name,
                tool_id,
                input,
            },
            StreamEvent::ToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
                result,
            } => Msg::StreamToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
                result,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> crossterm::event::Event {
        crossterm::event::Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn key_mod(code: KeyCode, mods: KeyModifiers) -> crossterm::event::Event {
        crossterm::event::Event::Key(KeyEvent::new(code, mods))
    }

    #[test]
    fn tick_event_maps_to_tick() {
        let app = test_app();
        let msg = app.map_event(Event::Tick);
        assert!(matches!(msg, Some(Msg::Tick)));
    }

    #[test]
    fn ctrl_c_maps_to_quit() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Quit)));
    }

    #[test]
    fn ctrl_q_maps_to_quit() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Char('q'), KeyModifiers::CONTROL));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Quit)));
    }

    #[test]
    fn f1_maps_to_help() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::F(1)));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OpenOverlay(OverlayKind::Help))));
    }

    #[test]
    fn ctrl_f_toggles_sidebar() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Char('f'), KeyModifiers::CONTROL));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ToggleSidebar)));
    }

    #[test]
    fn enter_maps_to_submit() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Enter));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Submit)));
    }

    #[test]
    fn question_mark_on_empty_input_opens_help() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Char('?')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OpenOverlay(OverlayKind::Help))));
    }

    #[test]
    fn question_mark_with_text_is_char_input() {
        let mut app = test_app();
        app.input.text = "hello".to_string();
        app.input.cursor = 5;
        let event = Event::Terminal(key(KeyCode::Char('?')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CharInput('?'))));
    }

    #[test]
    fn colon_on_empty_input_opens_command_palette() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Char(':'), KeyModifiers::SHIFT));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CommandPaletteOpen)));
    }

    #[test]
    fn slash_on_empty_input_opens_session_search() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Char('/')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SessionSearchOpen)));
    }

    #[test]
    fn up_with_empty_input_and_messages_selects() {
        let mut app = test_app();
        app.messages.push(crate::state::ChatMessage {
            role: "user".to_string(),
            text: "hi".to_string(),
            text_lower: "hi".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });
        let event = Event::Terminal(key(KeyCode::Up));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SelectPrev)));
    }

    #[test]
    fn up_with_text_navigates_history() {
        let mut app = test_app();
        app.input.text = "some text".to_string();
        app.input.cursor = 9;
        let event = Event::Terminal(key(KeyCode::Up));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::HistoryUp)));
    }

    #[test]
    fn resize_event_maps_to_resize() {
        let app = test_app();
        let event = Event::Terminal(crossterm::event::Event::Resize(80, 24));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Resize(80, 24))));
    }

    // --- Selection mode tests ---

    #[test]
    fn selection_mode_j_moves_next() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Char('j')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SelectNext)));
    }

    #[test]
    fn selection_mode_k_moves_prev() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.selected_message = Some(1);
        let event = Event::Terminal(key(KeyCode::Char('k')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SelectPrev)));
    }

    #[test]
    fn selection_mode_esc_deselects() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::DeselectMessage)));
    }

    #[test]
    fn selection_mode_c_copies() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Char('c')));
        let msg = app.map_event(event);
        assert!(matches!(
            msg,
            Some(Msg::MessageAction(MessageActionKind::Copy))
        ));
    }

    // --- Command palette mode tests ---

    #[test]
    fn palette_esc_closes() {
        let mut app = test_app();
        app.command_palette.active = true;
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CommandPaletteClose)));
    }

    #[test]
    fn palette_enter_selects() {
        let mut app = test_app();
        app.command_palette.active = true;
        let event = Event::Terminal(key(KeyCode::Enter));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CommandPaletteSelect)));
    }

    #[test]
    fn palette_char_inputs() {
        let mut app = test_app();
        app.command_palette.active = true;
        let event = Event::Terminal(key(KeyCode::Char('a')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CommandPaletteInput('a'))));
    }

    // --- Filter mode tests ---

    #[test]
    fn filter_editing_esc_closes() {
        let mut app = test_app();
        app.filter.active = true;
        app.filter.editing = true;
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::FilterClose)));
    }

    #[test]
    fn filter_editing_enter_confirms() {
        let mut app = test_app();
        app.filter.active = true;
        app.filter.editing = true;
        let event = Event::Terminal(key(KeyCode::Enter));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::FilterConfirm)));
    }

    #[test]
    fn filter_editing_char_inputs() {
        let mut app = test_app();
        app.filter.active = true;
        app.filter.editing = true;
        let event = Event::Terminal(key(KeyCode::Char('x')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::FilterInput('x'))));
    }

    #[test]
    fn filter_applied_n_next_match() {
        let mut app = test_app();
        app.filter.active = true;
        app.filter.editing = false;
        app.filter.text = "search".to_string();
        let event = Event::Terminal(key(KeyCode::Char('n')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::FilterNextMatch)));
    }

    // --- Overlay tests ---

    #[test]
    fn overlay_esc_closes() {
        let mut app = test_app();
        app.overlay = Some(Overlay::Help);
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CloseOverlay)));
    }

    #[test]
    fn overlay_up_navigates() {
        let mut app = test_app();
        app.overlay = Some(Overlay::AgentPicker { cursor: 0 });
        let event = Event::Terminal(key(KeyCode::Up));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayUp)));
    }

    // --- SSE event mapping ---

    #[test]
    fn sse_connected_maps() {
        let app = test_app();
        let event = Event::Sse(SseEvent::Connected);
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SseConnected)));
    }

    #[test]
    fn sse_ping_maps_to_tick() {
        let app = test_app();
        let event = Event::Sse(SseEvent::Ping);
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Tick)));
    }

    // --- Stream event mapping ---

    #[test]
    fn stream_text_delta_maps() {
        let app = test_app();
        let event = Event::Stream(StreamEvent::TextDelta("hello".to_string()));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::StreamTextDelta(_))));
    }

    #[test]
    fn stream_error_maps() {
        let app = test_app();
        let event = Event::Stream(StreamEvent::Error("oops".to_string()));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::StreamError(_))));
    }

    // --- Scroll keys ---

    #[test]
    fn page_up_maps() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::PageUp));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ScrollPageUp)));
    }

    #[test]
    fn shift_up_scrolls() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Up, KeyModifiers::SHIFT));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ScrollUp)));
    }

    // --- Settings overlay key mapping ---

    #[test]
    fn settings_overlay_up_down() {
        let mut app = test_app();
        let settings = crate::state::settings::SettingsOverlay::from_config(&serde_json::json!({}));
        app.overlay = Some(Overlay::Settings(settings));
        let event = Event::Terminal(key(KeyCode::Up));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayUp)));
    }

    #[test]
    fn settings_overlay_s_key_saves() {
        let mut app = test_app();
        let settings = crate::state::settings::SettingsOverlay::from_config(&serde_json::json!({}));
        app.overlay = Some(Overlay::Settings(settings));
        let event = Event::Terminal(key(KeyCode::Char('s')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayFilter('s'))));
    }

    // --- v key enters selection ---

    #[test]
    fn v_on_empty_input_with_messages_enters_selection() {
        let app = test_app_with_messages(vec![("user", "hello")]);
        let event = Event::Terminal(key(KeyCode::Char('v')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SelectPrev)));
    }

    #[test]
    fn v_on_empty_input_no_messages_is_char_input() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Char('v')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CharInput('v'))));
    }

    #[test]
    fn v_with_text_is_char_input() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.input.text = "typing".to_string();
        app.input.cursor = 6;
        let event = Event::Terminal(key(KeyCode::Char('v')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CharInput('v'))));
    }

    // --- Enter in selection mode drills into detail view ---

    #[test]
    fn selection_mode_enter_drills_in() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Enter));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ViewDrillIn)));
    }

    // --- Context actions overlay j/k navigation ---

    #[test]
    fn context_actions_overlay_j_moves_down() {
        let mut app = test_app();
        app.overlay = Some(Overlay::ContextActions(
            crate::state::ContextActionsOverlay {
                actions: vec![crate::state::ContextAction {
                    label: "Copy",
                    kind: MessageActionKind::Copy,
                }],
                cursor: 0,
            },
        ));
        let event = Event::Terminal(key(KeyCode::Char('j')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayDown)));
    }

    #[test]
    fn context_actions_overlay_k_moves_up() {
        let mut app = test_app();
        app.overlay = Some(Overlay::ContextActions(
            crate::state::ContextActionsOverlay {
                actions: vec![crate::state::ContextAction {
                    label: "Copy",
                    kind: MessageActionKind::Copy,
                }],
                cursor: 0,
            },
        ));
        let event = Event::Terminal(key(KeyCode::Char('k')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayUp)));
    }

    // --- View stack navigation key tests ---

    #[test]
    fn esc_at_non_home_view_pops_back() {
        let mut app = test_app();
        app.view_stack.push(crate::state::View::Sessions {
            agent_id: "syn".into(),
        });
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ViewPopBack)));
    }

    #[test]
    fn esc_at_home_with_selection_deselects() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(0);
        // view_stack is Home by default
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::DeselectMessage)));
    }

    #[test]
    fn esc_at_non_home_with_selection_still_pops() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(0);
        app.view_stack
            .push(crate::state::View::MessageDetail { message_index: 0 });
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ViewPopBack)));
    }

    // --- ScrollToBottom keybinding tests ---

    #[test]
    fn end_key_on_empty_input_scrolls_to_bottom() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::End));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ScrollToBottom)));
    }

    #[test]
    fn end_key_with_text_moves_cursor_to_end() {
        let mut app = test_app();
        app.input.text = "hello".to_string();
        app.input.cursor = 0;
        let event = Event::Terminal(key(KeyCode::End));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CursorEnd)));
    }

    // --- NextAgent/PrevAgent keybinding tests ---

    #[test]
    fn tab_on_empty_input_with_no_ops_cycles_next_agent() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Tab));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::NextAgent)));
    }

    #[test]
    fn tab_with_at_mention_does_completion_not_agent_cycle() {
        let mut app = test_app();
        app.input.text = "@al".to_string();
        app.input.cursor = 3;
        let event = Event::Terminal(key(KeyCode::Tab));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CharInput('\t'))));
    }

    #[test]
    fn shift_tab_cycles_prev_agent() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::BackTab));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::PrevAgent)));
    }

    #[test]
    fn ctrl_backtab_switches_tab_prev_not_agent() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::BackTab, KeyModifiers::CONTROL));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::TabPrev)));
    }
}
