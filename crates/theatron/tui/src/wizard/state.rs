//! Wizard state: step definitions, field types, and collected answers.

use std::path::PathBuf;

/// Number of wizard steps.
pub(crate) const TOTAL_STEPS: usize = 5;

/// Display labels for each step, in order.
pub(crate) const STEP_LABELS: &[&str] = &["Credentials", "Account", "Profile", "Agent", "Ready"];

/// A single option in a [`FieldKind::Select`] field.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SelectOption {
    pub value: &'static str,
    pub label: &'static str,
}

/// How a wizard field is rendered and edited.
#[derive(Debug, Clone)]
pub(crate) enum FieldKind {
    /// Single-line text input. `secret = true` renders the value as `●` characters.
    Text { secret: bool },
    /// Cycle-through selection from a fixed list.
    Select { options: &'static [SelectOption] },
    /// Non-interactive display-only field.
    ReadOnly,
}

/// A single form field in the wizard.
#[derive(Debug, Clone)]
pub(crate) struct WizardField {
    pub label: &'static str,
    pub value: String,
    pub kind: FieldKind,
    /// Short hint shown dimmed after the value.
    pub hint: &'static str,
}

impl WizardField {
    pub(crate) fn text(
        label: &'static str,
        default: impl Into<String>,
        hint: &'static str,
    ) -> Self {
        Self {
            label,
            value: default.into(),
            kind: FieldKind::Text { secret: false },
            hint,
        }
    }

    pub(crate) fn secret(label: &'static str, hint: &'static str) -> Self {
        Self {
            label,
            value: String::new(),
            kind: FieldKind::Text { secret: true },
            hint,
        }
    }

    pub(crate) fn select(
        label: &'static str,
        options: &'static [SelectOption],
        default: &'static str,
        hint: &'static str,
    ) -> Self {
        Self {
            label,
            value: default.to_owned(),
            kind: FieldKind::Select { options },
            hint,
        }
    }

    pub(crate) fn readonly(
        label: &'static str,
        value: impl Into<String>,
        hint: &'static str,
    ) -> Self {
        Self {
            label,
            value: value.into(),
            kind: FieldKind::ReadOnly,
            hint,
        }
    }
}

/// Active text-edit buffer for a [`FieldKind::Text`] field.
#[derive(Debug, Clone)]
pub(crate) struct EditState {
    pub buffer: String,
    /// Byte offset of the edit cursor within `buffer`.
    pub cursor: usize,
}

impl EditState {
    pub(crate) fn new(initial: &str) -> Self {
        Self {
            buffer: initial.to_owned(),
            cursor: initial.len(),
        }
    }

    pub(crate) fn insert(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub(crate) fn delete_before(&mut self) {
        if self.cursor > 0 {
            let prev = self.buffer[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
            self.buffer.remove(prev);
            self.cursor = prev;
        }
    }

    pub(crate) fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.buffer[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
        }
    }

    pub(crate) fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor = self.buffer[self.cursor..]
                .char_indices()
                .nth(1)
                .map_or(self.buffer.len(), |(i, _)| self.cursor + i);
        }
    }

    pub(crate) fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    pub(crate) fn delete_to_end(&mut self) {
        self.buffer.truncate(self.cursor);
    }
}

/// State for a single wizard step: its fields, focused field cursor, and active edit.
#[derive(Debug)]
pub(crate) struct StepState {
    pub fields: Vec<WizardField>,
    pub cursor: usize,
    pub editing: Option<EditState>,
}

impl StepState {
    pub(crate) fn current_field(&self) -> Option<&WizardField> {
        self.fields.get(self.cursor)
    }

    pub(crate) fn nav_up(&mut self) {
        if self.editing.is_none() {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    pub(crate) fn nav_down(&mut self) {
        if self.editing.is_none() && self.cursor + 1 < self.fields.len() {
            self.cursor += 1;
        }
    }

    /// Begin editing the currently focused text field.
    pub(crate) fn begin_edit(&mut self) {
        if let Some(field) = self.fields.get(self.cursor)
            && matches!(field.kind, FieldKind::Text { .. })
        {
            self.editing = Some(EditState::new(&field.value));
        }
    }

    /// Commit the edit buffer to the field value and exit edit mode.
    pub(crate) fn commit_edit(&mut self) {
        if let Some(edit) = self.editing.take()
            && let Some(field) = self.fields.get_mut(self.cursor)
        {
            field.value = edit.buffer;
        }
    }

    /// Discard the current edit without changing the field value.
    pub(crate) fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Advance a select field to the next option.
    pub(crate) fn cycle_select_next(&mut self) {
        if let Some(field) = self.fields.get_mut(self.cursor)
            && let FieldKind::Select { options } = field.kind
        {
            let pos = options
                .iter()
                .position(|o| o.value == field.value)
                .unwrap_or(0);
            let next = (pos + 1) % options.len();
            if let Some(opt) = options.get(next) {
                field.value = opt.value.to_owned();
            }
        }
    }

    /// Rewind a select field to the previous option.
    pub(crate) fn cycle_select_prev(&mut self) {
        if let Some(field) = self.fields.get_mut(self.cursor)
            && let FieldKind::Select { options } = field.kind
        {
            let pos = options
                .iter()
                .position(|o| o.value == field.value)
                .unwrap_or(0);
            let prev = if pos == 0 {
                options.len().saturating_sub(1)
            } else {
                pos - 1
            };
            if let Some(opt) = options.get(prev) {
                field.value = opt.value.to_owned();
            }
        }
    }
}

// ─── Static option lists ────────────────────────────────────────────────────

pub(crate) static PROVIDER_OPTIONS: &[SelectOption] = &[
    SelectOption {
        value: "anthropic",
        label: "Anthropic (Claude)",
    },
    SelectOption {
        value: "openai",
        label: "OpenAI (GPT)",
    },
];

pub(crate) static BIND_OPTIONS: &[SelectOption] = &[
    SelectOption {
        value: "localhost",
        label: "localhost — this machine only",
    },
    SelectOption {
        value: "lan",
        label: "lan — network / Tailscale",
    },
];

pub(crate) static AUTH_OPTIONS: &[SelectOption] = &[
    SelectOption {
        value: "none",
        label: "none — single user, no auth",
    },
    SelectOption {
        value: "token",
        label: "token — API key required",
    },
];

pub(crate) static MODEL_OPTIONS: &[SelectOption] = &[
    SelectOption {
        value: "claude-sonnet-4-6",
        label: "claude-sonnet-4-6 (recommended)",
    },
    SelectOption {
        value: "claude-opus-4-6",
        label: "claude-opus-4-6",
    },
    SelectOption {
        value: "claude-haiku-4-5",
        label: "claude-haiku-4-5",
    },
];

// ─── Step factories ──────────────────────────────────────────────────────────

fn detected_provider() -> &'static str {
    // WHY: prefer Anthropic unless only OpenAI key is present
    if std::env::var("OPENAI_API_KEY").is_ok() && std::env::var("ANTHROPIC_API_KEY").is_err() {
        "openai"
    } else {
        "anthropic"
    }
}

fn credential_status() -> String {
    let a = std::env::var("ANTHROPIC_API_KEY").is_ok();
    let o = std::env::var("OPENAI_API_KEY").is_ok();
    match (a, o) {
        (true, true) => "ANTHROPIC_API_KEY ✓  OPENAI_API_KEY ✓".to_owned(),
        (true, false) => "ANTHROPIC_API_KEY ✓".to_owned(),
        (false, true) => "OPENAI_API_KEY ✓".to_owned(),
        (false, false) => "no API key detected in environment".to_owned(),
    }
}

fn make_credentials_step() -> StepState {
    StepState {
        fields: vec![
            WizardField::select("Provider", PROVIDER_OPTIONS, detected_provider(), ""),
            WizardField::secret("API Key", "leave blank to auto-detect from env"),
            WizardField::readonly("Detected", credential_status(), ""),
        ],
        cursor: 0,
        editing: None,
    }
}

fn make_account_step(root: &str) -> StepState {
    StepState {
        fields: vec![
            WizardField::text("Instance path", root, "config and data root directory"),
            WizardField::select("Gateway bind", BIND_OPTIONS, "localhost", ""),
            WizardField::select("Auth mode", AUTH_OPTIONS, "none", ""),
        ],
        cursor: 0,
        editing: None,
    }
}

fn detect_timezone() -> String {
    jiff::tz::TimeZone::system()
        .iana_name()
        .map_or_else(|| "UTC".to_owned(), ToOwned::to_owned)
}

fn make_profile_step() -> StepState {
    StepState {
        fields: vec![
            WizardField::text("Your name", "", "written to agent USER.md"),
            WizardField::text("Your role", "", "e.g. engineer, researcher, designer"),
            WizardField::text(
                "Timezone",
                detect_timezone(),
                "IANA tz, e.g. America/New_York",
            ),
            WizardField::text(
                "Notes",
                "",
                "anything else the agent should know (optional)",
            ),
        ],
        cursor: 0,
        editing: None,
    }
}

fn make_agent_step() -> StepState {
    StepState {
        fields: vec![
            WizardField::text("Agent name", "Pronoea", "display name shown in the TUI"),
            WizardField::text(
                "Agent ID",
                "pronoea",
                "alphanumeric + hyphens, used in paths",
            ),
            WizardField::select("Model", MODEL_OPTIONS, "claude-sonnet-4-6", ""),
        ],
        cursor: 0,
        editing: None,
    }
}

fn make_ready_step() -> StepState {
    // NOTE: populated via refresh_ready_step() before the step is shown
    StepState {
        fields: Vec::new(),
        cursor: 0,
        editing: None,
    }
}

// ─── WizardAnswers ────────────────────────────────────────────────────────────

/// Configuration answers collected by the setup wizard.
#[derive(Debug, Clone)]
pub struct WizardAnswers {
    /// Instance root directory.
    pub root: PathBuf,
    /// API provider (`"anthropic"` or `"openai"`).
    pub api_provider: String,
    /// Raw API key string pasted by the user; `None` means use the environment.
    pub api_key: Option<String>,
    /// Credential resolution source (`"api-key"` or `"auto"`).
    pub credential_source: String,
    /// Gateway bind target (`"localhost"` or `"lan"`).
    pub bind: String,
    /// Gateway auth mode (`"none"` or `"token"`).
    pub auth_mode: String,
    /// IANA timezone identifier (e.g., `"America/New_York"`).
    pub timezone: String,
    /// Operator display name for `USER.md`.
    pub user_name: String,
    /// Operator role description for `USER.md`.
    pub user_role: String,
    /// Agent identifier (alphanumeric + hyphens/underscores).
    pub agent_id: String,
    /// Agent display name.
    pub agent_name: String,
    /// Primary model identifier.
    pub model: String,
}

// ─── WizardState ─────────────────────────────────────────────────────────────

/// Top-level wizard state machine.
#[derive(Debug)]
pub(crate) struct WizardState {
    /// Current step index (0 = Credentials … 4 = Ready).
    pub step: usize,
    /// One [`StepState`] per step.
    pub steps: Vec<StepState>,
    /// `true` once the user confirms on the Ready step.
    pub completed: bool,
    /// `true` signals the event loop to exit.
    pub should_quit: bool,
}

impl WizardState {
    pub(crate) fn new(root: Option<PathBuf>, preset_key: Option<String>) -> Self {
        let root_str = root.map_or_else(
            || "./instance".to_owned(),
            |p| p.to_string_lossy().into_owned(),
        );

        let mut steps = vec![
            make_credentials_step(),
            make_account_step(&root_str),
            make_profile_step(),
            make_agent_step(),
            make_ready_step(),
        ];

        // Pre-fill API key field from caller if provided
        if let Some(key) = preset_key
            && let Some(step) = steps.get_mut(0)
            && let Some(field) = step.fields.get_mut(1)
        {
            field.value = key;
        }

        Self {
            step: 0,
            steps,
            completed: false,
            should_quit: false,
        }
    }

    /// Return an immutable reference to the current step.
    pub(crate) fn current_step(&self) -> Option<&StepState> {
        self.steps.get(self.step)
    }

    /// Return a mutable reference to the current step.
    pub(crate) fn current_step_mut(&mut self) -> Option<&mut StepState> {
        self.steps.get_mut(self.step)
    }

    /// Advance to the next step.  Returns `false` when already on the last step.
    pub(crate) fn next_step(&mut self) -> bool {
        if self.step + 1 < TOTAL_STEPS {
            self.step += 1;
            if self.step == TOTAL_STEPS - 1 {
                self.refresh_ready_step();
            }
            true
        } else {
            false
        }
    }

    /// Go back to the previous step.  Returns `false` on the first step.
    pub(crate) fn back_step(&mut self) -> bool {
        if self.step > 0 {
            self.step -= 1;
            true
        } else {
            false
        }
    }

    /// Rebuild the Ready step summary from the other steps' collected values.
    pub(crate) fn refresh_ready_step(&mut self) {
        // Collect strings (owned) before mutably borrowing steps
        let provider = self.field_value(0, 0);
        let key_set = !self.field_value(0, 1).is_empty();
        let root = self.field_value(1, 0);
        let bind = self.field_value(1, 1);
        let auth = self.field_value(1, 2);
        let user_name = self.field_value(2, 0);
        let user_role = self.field_value(2, 1);
        let tz = self.field_value(2, 2);
        let agent_name = self.field_value(3, 0);
        let agent_id = self.field_value(3, 1);
        let model = self.field_value(3, 2);

        let cred = if key_set {
            "API key (pasted)"
        } else {
            "auto (env var)"
        };

        let fields = vec![
            WizardField::readonly("Instance path", root, ""),
            WizardField::readonly("Provider", provider, ""),
            WizardField::readonly("Credentials", cred, ""),
            WizardField::readonly("Gateway bind", bind, ""),
            WizardField::readonly("Auth mode", auth, ""),
            WizardField::readonly("Your name", user_name, ""),
            WizardField::readonly("Your role", user_role, ""),
            WizardField::readonly("Timezone", tz, ""),
            WizardField::readonly("Agent name", agent_name, ""),
            WizardField::readonly("Agent ID", agent_id, ""),
            WizardField::readonly("Model", model, ""),
        ];

        if let Some(ready) = self.steps.get_mut(TOTAL_STEPS - 1) {
            ready.fields = fields;
            ready.cursor = 0;
        }
    }

    /// Assemble final [`WizardAnswers`] from collected step values.
    pub(crate) fn collect_answers(&self) -> WizardAnswers {
        let api_key_str = self.field_value(0, 1);
        let api_key = if api_key_str.is_empty() {
            None
        } else {
            Some(api_key_str)
        };
        let credential_source = if api_key.is_some() { "api-key" } else { "auto" };

        WizardAnswers {
            root: PathBuf::from(self.field_value(1, 0)),
            api_provider: self.field_value(0, 0),
            api_key,
            credential_source: credential_source.to_owned(),
            bind: self.field_value(1, 1),
            auth_mode: self.field_value(1, 2),
            timezone: self.field_value(2, 2),
            user_name: self.field_value(2, 0),
            user_role: self.field_value(2, 1),
            agent_id: self.field_value(3, 1),
            agent_name: self.field_value(3, 0),
            model: self.field_value(3, 2),
        }
    }

    fn field_value(&self, step: usize, field: usize) -> String {
        self.steps
            .get(step)
            .and_then(|s| s.fields.get(field))
            .map(|f| f.value.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn wizard_state_initializes_with_five_steps() {
        let state = WizardState::new(None, None);
        assert_eq!(state.steps.len(), TOTAL_STEPS);
        assert_eq!(state.step, 0);
        assert!(!state.completed);
        assert!(!state.should_quit);
    }

    #[test]
    fn wizard_state_preset_key_fills_credentials_field() {
        let state = WizardState::new(None, Some("sk-ant-test".to_owned()));
        let key_field = state.steps.first().unwrap().fields.get(1).unwrap();
        assert_eq!(key_field.value, "sk-ant-test");
    }

    #[test]
    fn next_step_increments_step_index() {
        let mut state = WizardState::new(None, None);
        assert!(state.next_step());
        assert_eq!(state.step, 1);
    }

    #[test]
    fn back_step_returns_false_on_first_step() {
        let mut state = WizardState::new(None, None);
        assert!(!state.back_step());
        assert_eq!(state.step, 0);
    }

    #[test]
    fn next_step_returns_false_after_last_step() {
        let mut state = WizardState::new(None, None);
        for _ in 0..(TOTAL_STEPS - 1) {
            assert!(state.next_step());
        }
        assert!(!state.next_step());
    }

    #[test]
    fn collect_answers_defaults() {
        let state = WizardState::new(None, None);
        let answers = state.collect_answers();
        assert_eq!(answers.api_provider, "anthropic");
        assert_eq!(answers.credential_source, "auto");
        assert_eq!(answers.bind, "localhost");
        assert_eq!(answers.auth_mode, "none");
        assert_eq!(answers.agent_name, "Pronoea");
        assert_eq!(answers.agent_id, "pronoea");
        assert_eq!(answers.model, "claude-sonnet-4-6");
        assert!(answers.api_key.is_none());
    }

    #[test]
    fn collect_answers_with_pasted_key() {
        let state = WizardState::new(None, Some("sk-ant-key123".to_owned()));
        let answers = state.collect_answers();
        assert_eq!(answers.api_key, Some("sk-ant-key123".to_owned()));
        assert_eq!(answers.credential_source, "api-key");
    }

    #[test]
    fn step_state_nav_up_clamps_at_zero() {
        let mut step = make_credentials_step();
        step.nav_up();
        assert_eq!(step.cursor, 0);
    }

    #[test]
    fn step_state_nav_down_moves_cursor() {
        let mut step = make_credentials_step();
        step.nav_down();
        assert_eq!(step.cursor, 1);
    }

    #[test]
    fn cycle_select_next_wraps() {
        let mut step = make_credentials_step();
        // Provider starts at anthropic (index 0), cycle twice to wrap back
        step.cycle_select_next();
        assert_eq!(step.fields.first().unwrap().value, "openai");
        step.cycle_select_next();
        assert_eq!(step.fields.first().unwrap().value, "anthropic");
    }

    #[test]
    fn edit_state_insert_and_delete() {
        let mut edit = EditState::new("abc");
        edit.insert('d');
        assert_eq!(edit.buffer, "abcd");
        edit.delete_before();
        assert_eq!(edit.buffer, "abc");
    }

    #[test]
    fn edit_state_move_home_and_end() {
        let mut edit = EditState::new("hello");
        edit.move_home();
        assert_eq!(edit.cursor, 0);
        edit.move_end();
        assert_eq!(edit.cursor, 5);
    }

    #[test]
    fn refresh_ready_step_populates_fields() {
        let mut state = WizardState::new(None, None);
        // Advance to ready step
        for _ in 0..(TOTAL_STEPS - 1) {
            state.next_step();
        }
        let ready = state.steps.get(TOTAL_STEPS - 1).unwrap();
        assert!(
            !ready.fields.is_empty(),
            "ready step should have summary fields"
        );
    }

    #[test]
    fn step_labels_count_matches_total() {
        assert_eq!(STEP_LABELS.len(), TOTAL_STEPS);
    }

    #[test]
    fn begin_edit_on_text_field_sets_editing() {
        let mut step = make_account_step("./instance");
        step.begin_edit();
        assert!(step.editing.is_some());
    }

    #[test]
    fn commit_edit_updates_field_value() {
        let mut step = make_account_step("./instance");
        step.begin_edit();
        if let Some(ref mut edit) = step.editing {
            edit.buffer = "/custom/path".to_owned();
        }
        step.commit_edit();
        assert_eq!(step.fields.first().unwrap().value, "/custom/path");
    }

    #[test]
    fn cancel_edit_discards_changes() {
        let mut step = make_account_step("./instance");
        step.begin_edit();
        if let Some(ref mut edit) = step.editing {
            edit.buffer = "/discarded".to_owned();
        }
        step.cancel_edit();
        assert!(step.editing.is_none());
        assert_eq!(step.fields.first().unwrap().value, "./instance");
    }
}
