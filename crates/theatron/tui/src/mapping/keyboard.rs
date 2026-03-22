//! Keyboard event mapping methods.
/// Event-to-Msg translation: maps terminal, SSE, and stream events to application messages.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::msg::{MessageActionKind, Msg, OverlayKind};
use crate::state::Overlay;

impl crate::app::App {
    pub(super) fn map_key(&self, key: KeyEvent) -> Option<Msg> {
        // WHY: Ctrl+C during an active turn cancels it immediately rather than quitting.
        // This is the highest-priority check so overlays and selection mode cannot
        // intercept it — the user's intent is unambiguous when a turn is running.
        if self.connection.active_turn_id.is_some()
            && key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('c')
        {
            return Some(Msg::CancelTurn);
        }

        if self.layout.overlay.is_some() {
            return self.map_overlay_key(key);
        }

        if self.interaction.command_palette.active {
            return self.map_palette_key(key);
        }

        if self.interaction.slash_complete.active {
            return self.map_slash_complete_key(key);
        }

        if self.interaction.filter.editing {
            return self.map_filter_editing_key(key);
        }

        if self.interaction.filter.active
            && let Some(msg) = self.map_filter_applied_key(key)
        {
            return Some(msg);
        }

        // WHY: g-prefix must intercept before normal key routing so gt/gT are
        // treated as two-key sequences; after 'g' is consumed, the second char falls through.
        if self.layout.pending_g {
            return match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('t')) => Some(Msg::TabNext),
                (KeyModifiers::SHIFT, KeyCode::Char('T')) => Some(Msg::TabPrev),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    Some(Msg::CharInput(c))
                }
                _ => None,
            };
        }

        if self.is_editor_view() {
            return self.map_editor_key(key);
        }

        if self.is_memory_view() {
            return self.map_memory_key(key);
        }

        // WHY: ViewPopBack takes priority over DeselectMessage so Esc always unwinds
        // the view stack before affecting selection state.
        if !self.layout.view_stack.is_home()
            && matches!((key.modifiers, key.code), (_, KeyCode::Esc))
        {
            return Some(Msg::ViewPopBack);
        }

        if self.interaction.selected_message.is_some() {
            return self.map_selection_key(key);
        }

        if self.layout.ops.visible
            && self.layout.ops.focused_pane == crate::state::FocusedPane::Operations
        {
            return self.map_ops_pane_key(key);
        }

        // WHY: Esc during an active turn (no modal, home view, ops chat-focused) cancels it.
        // Checked after filter/selection/ops so those modal contexts handle Esc first.
        if self.connection.active_turn_id.is_some()
            && matches!(key.code, KeyCode::Esc)
            && self.layout.view_stack.is_home()
        {
            return Some(Msg::CancelTurn);
        }

        if let Some(action) = self.interaction.keymap.lookup(key.modifiers, key.code) {
            return Some(action.to_msg());
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('w'))
                if self.interaction.input.text.is_empty() =>
            {
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
                let n = usize::try_from(u32::from(c) - u32::from('1')).unwrap_or(0);
                Some(Msg::TabJump(n))
            }

            (_, KeyCode::Tab) => {
                if self.interaction.input.text.contains('@') {
                    Some(Msg::CharInput('\t'))
                } else if self.layout.ops.visible {
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
            (_, KeyCode::End) if self.interaction.input.text.is_empty() => {
                Some(Msg::ScrollToBottom)
            }
            (_, KeyCode::End) => Some(Msg::CursorEnd),

            // WHY: Up/Down with empty input enters selection mode rather than history nav,
            // matching the modal editing convention where arrow keys in read state navigate messages.
            (_, KeyCode::Up)
                if self.interaction.input.text.is_empty()
                    && !self.dashboard.messages.is_empty() =>
            {
                Some(Msg::SelectPrev)
            }
            (_, KeyCode::Down)
                if self.interaction.input.text.is_empty()
                    && !self.dashboard.messages.is_empty() =>
            {
                Some(Msg::SelectNext)
            }
            (_, KeyCode::Up) => Some(Msg::HistoryUp),
            (_, KeyCode::Down) => Some(Msg::HistoryDown),

            (KeyModifiers::CONTROL, KeyCode::Char('b'))
                if self.interaction.input.text.is_empty() =>
            {
                Some(Msg::OpenOverlay(OverlayKind::ContextBudget))
            }

            (KeyModifiers::NONE, KeyCode::Char('?')) if self.interaction.input.text.is_empty() => {
                Some(Msg::OpenOverlay(OverlayKind::Help))
            }

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(':'))
                if self.interaction.input.text.is_empty() =>
            {
                Some(Msg::CommandPaletteOpen)
            }

            (KeyModifiers::NONE, KeyCode::Char('/')) if self.interaction.input.text.is_empty() => {
                Some(Msg::SlashCompleteOpen)
            }

            (KeyModifiers::NONE, KeyCode::Char('v'))
                if self.interaction.input.text.is_empty()
                    && !self.dashboard.messages.is_empty() =>
            {
                Some(Msg::SelectPrev)
            }

            (KeyModifiers::NONE, KeyCode::Char('g'))
                if self.interaction.input.text.is_empty() && self.layout.tab_bar.len() > 1 =>
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
            (_, KeyCode::Char('s')) => Some(Msg::OpsToggleShowAll),
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

            (KeyModifiers::SHIFT, KeyCode::Up) => Some(Msg::ScrollLineUp),
            (KeyModifiers::SHIFT, KeyCode::Down) => Some(Msg::ScrollLineDown),

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

    #[expect(
        clippy::unused_self,
        reason = "consistent method signature with other map_ methods"
    )]
    fn map_slash_complete_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                Some(Msg::SlashCompleteClose)
            }
            (_, KeyCode::Enter) => Some(Msg::SlashCompleteSelect),
            (_, KeyCode::Up) => Some(Msg::SlashCompleteUp),
            (_, KeyCode::Down) => Some(Msg::SlashCompleteDown),
            (_, KeyCode::Backspace) => Some(Msg::SlashCompleteBackspace),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::SlashCompleteInput(c))
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
                if self.interaction.filter.text.is_empty() {
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
            (KeyModifiers::NONE, KeyCode::Char('/')) if self.interaction.input.text.is_empty() => {
                Some(Msg::FilterOpen)
            }
            (KeyModifiers::NONE, KeyCode::Char('n')) if self.interaction.input.text.is_empty() => {
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

            (KeyModifiers::NONE, KeyCode::Char('?'))
                if self.interaction.command_palette.input.is_empty() =>
            {
                Some(Msg::OpenOverlay(OverlayKind::Help))
            }

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::CommandPaletteInput(c))
            }
            _ => None,
        }
    }

    fn is_editor_view(&self) -> bool {
        matches!(
            self.layout.view_stack.current(),
            crate::state::view_stack::View::FileEditor
        )
    }

    fn map_editor_key(&self, key: KeyEvent) -> Option<Msg> {
        let editor = &self.layout.editor;

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => return Some(Msg::Quit),
            _ => {}
        }

        // WHY: Modal inputs (rename, new file, confirm delete) intercept all keys.
        if editor.confirm_delete.is_some() {
            return match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => Some(Msg::EditorConfirmDelete(true)),
                (_, KeyCode::Char('n')) | (_, KeyCode::Esc) => {
                    Some(Msg::EditorConfirmDelete(false))
                }
                _ => None,
            };
        }

        if editor.rename_input.is_some() || editor.new_file_input.is_some() {
            return match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => Some(Msg::EditorModalCancel),
                (_, KeyCode::Enter) => Some(Msg::EditorNewline),
                (_, KeyCode::Backspace) => Some(Msg::EditorBackspace),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    Some(Msg::EditorCharInput(c))
                }
                _ => None,
            };
        }

        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => Some(Msg::EditorClose),
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => Some(Msg::EditorSave),
            (KeyModifiers::CONTROL, KeyCode::Char('x')) => Some(Msg::EditorCut),
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => Some(Msg::EditorCopy),
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => Some(Msg::EditorPaste),
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => Some(Msg::EditorTreeToggle),
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => Some(Msg::EditorNewFileStart),
            (_, KeyCode::Tab) => Some(Msg::EditorFocusToggle),
            (_, KeyCode::F(2)) => Some(Msg::EditorRenameStart),
            (_, KeyCode::F(5)) => Some(Msg::EditorRefreshTree),
            (_, KeyCode::F(8)) => Some(Msg::EditorDeleteStart),
            (_, KeyCode::Enter) => Some(Msg::EditorNewline),
            (_, KeyCode::Backspace) => Some(Msg::EditorBackspace),
            (_, KeyCode::Delete) => Some(Msg::EditorDelete),
            (KeyModifiers::ALT, KeyCode::Right) => Some(Msg::EditorTabNext),
            (KeyModifiers::ALT, KeyCode::Left) => Some(Msg::EditorTabPrev),
            (_, KeyCode::Up) => Some(Msg::EditorCursorUp),
            (_, KeyCode::Down) => Some(Msg::EditorCursorDown),
            (_, KeyCode::Left) => Some(Msg::EditorCursorLeft),
            (_, KeyCode::Right) => Some(Msg::EditorCursorRight),
            (_, KeyCode::Home) => Some(Msg::EditorCursorHome),
            (_, KeyCode::End) => Some(Msg::EditorCursorEnd),
            (_, KeyCode::PageUp) => Some(Msg::EditorPageUp),
            (_, KeyCode::PageDown) => Some(Msg::EditorPageDown),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => Some(Msg::EditorTabClose),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::EditorCharInput(c))
            }
            _ => None,
        }
    }

    fn is_memory_view(&self) -> bool {
        matches!(
            self.layout.view_stack.current(),
            crate::state::view_stack::View::MemoryInspector
                | crate::state::view_stack::View::FactDetail { .. }
        )
    }

    fn map_memory_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => return Some(Msg::Quit),
            (KeyModifiers::CONTROL, KeyCode::Char('f')) => return Some(Msg::ToggleSidebar),
            _ => {
                // NOTE: unhandled key combinations produce no action
            }
        }

        if self.layout.memory.fact_list.editing_confidence {
            return match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => Some(Msg::MemoryConfidenceSubmit),
                (_, KeyCode::Esc) => Some(Msg::MemoryConfidenceCancel),
                (_, KeyCode::Backspace) => Some(Msg::MemoryConfidenceBackspace),
                (KeyModifiers::NONE, KeyCode::Char(c)) => Some(Msg::MemoryConfidenceInput(c)),
                _ => None,
            };
        }

        if self.layout.memory.search.search_active {
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

        if self.layout.memory.filters.filter_editing {
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
            self.layout.view_stack.current(),
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
        matches!(&self.layout.overlay, Some(Overlay::ContextActions(_)))
    }

    fn is_session_picker_overlay(&self) -> bool {
        matches!(&self.layout.overlay, Some(Overlay::SessionPicker(_)))
    }

    fn is_diff_view_overlay(&self) -> bool {
        matches!(&self.layout.overlay, Some(Overlay::DiffView(_)))
    }

    fn map_overlay_key(&self, key: KeyEvent) -> Option<Msg> {
        if matches!(&self.layout.overlay, Some(Overlay::Settings(_))) {
            return self.map_settings_overlay_key(key);
        }

        if matches!(&self.layout.overlay, Some(Overlay::SessionSearch(_))) {
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

        // WHY: `?` toggles help overlay: pressing it again closes it.
        if matches!(&self.layout.overlay, Some(Overlay::Help))
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
            (_, KeyCode::Char('l' | 'L')) if self.is_tool_approval_overlay() => {
                Some(Msg::ToolApprovalAlwaysAllow)
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
        matches!(&self.layout.overlay, Some(Overlay::ToolApproval(_)))
    }

    fn is_plan_approval_overlay(&self) -> bool {
        matches!(&self.layout.overlay, Some(Overlay::PlanApproval(_)))
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
            &self.layout.overlay,
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
}
