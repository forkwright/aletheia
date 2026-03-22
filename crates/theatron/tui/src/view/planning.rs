//! Rendering for the planning dashboard and retrospective views.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::state::planning::{
    CheckpointRisk, DisplayPhaseState, DisplayPlanState, DisplayProjectState, PlanningTab,
    RequirementStatus, RetrospectiveSection,
};
use crate::theme::Theme;

/// Height of the tab bar row.
const TAB_BAR_HEIGHT: u16 = 2;
/// Minimum height for the content area.
const CONTENT_MIN_HEIGHT: u16 = 3;
/// Height of the status/help bar.
const STATUS_BAR_HEIGHT: u16 = 1;

/// Render the planning dashboard (tab bar + tab content + status bar).
#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; indices 0/1/2 match the three constraints"
)]
pub(crate) fn render_dashboard(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(TAB_BAR_HEIGHT),
            Constraint::Min(CONTENT_MIN_HEIGHT),
            Constraint::Length(STATUS_BAR_HEIGHT),
        ])
        .split(area);

    render_tab_bar(app, frame, layout[0], theme);

    if app.layout.planning.loading {
        let loading = Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("Loading planning data...", theme.style_dim()),
        ]));
        frame.render_widget(loading, layout[1]);
    } else if app.layout.planning.project.is_none() {
        let empty = Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "No active project. Use :planning to load project data.",
                theme.style_dim(),
            ),
        ]));
        frame.render_widget(empty, layout[1]);
    } else {
        match app.layout.planning.tab {
            PlanningTab::Overview => render_overview(app, frame, layout[1], theme),
            PlanningTab::Requirements => render_requirements(app, frame, layout[1], theme),
            PlanningTab::Execution => render_execution(app, frame, layout[1], theme),
            PlanningTab::Verification => render_verification(app, frame, layout[1], theme),
            PlanningTab::Tasks => render_tasks(app, frame, layout[1], theme),
            PlanningTab::Timeline => render_timeline(app, frame, layout[1], theme),
            PlanningTab::EditHistory => render_edit_history(app, frame, layout[1], theme),
        }
    }

    render_status_bar(app, frame, layout[2], theme);
}

fn render_tab_bar(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for tab in PlanningTab::ALL {
        let is_active = tab == app.layout.planning.tab;
        let style = if is_active {
            theme.style_accent().add_modifier(Modifier::BOLD)
        } else {
            theme.style_dim()
        };
        let marker = if is_active { "▸ " } else { "  " };
        spans.push(Span::styled(format!("{marker}{}", tab.label()), style));
        spans.push(Span::raw("  "));
    }
    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn render_status_bar(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let checkpoint_hint = if app.layout.planning.has_pending_checkpoints() {
        Span::styled("  a", theme.style_accent())
    } else {
        Span::raw("")
    };
    let checkpoint_label = if app.layout.planning.has_pending_checkpoints() {
        Span::styled(" approve  ", theme.style_dim())
    } else {
        Span::raw("")
    };

    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled("Tab", theme.style_accent()),
        Span::styled(" switch  ", theme.style_dim()),
        Span::styled("j/k", theme.style_accent()),
        Span::styled(" navigate  ", theme.style_dim()),
        Span::styled("Enter", theme.style_accent()),
        Span::styled(" expand  ", theme.style_dim()),
        checkpoint_hint,
        checkpoint_label,
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" back", theme.style_dim()),
    ]);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn render_overview(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(ref project) = app.layout.planning.project else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            &project.name,
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));

    if !project.description.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(&project.description, theme.style_dim()),
        ]));
    }

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("State: ", theme.style_dim()),
        Span::styled(project.state.label(), state_style(project.state, theme)),
    ]));

    // Checkpoints section
    if !project.checkpoints.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Checkpoints", theme.style_fg().add_modifier(Modifier::BOLD)),
        ]));
        for (i, cp) in project.checkpoints.iter().enumerate() {
            let icon = if cp.approved { "\u{2713}" } else { "\u{25cb}" };
            let icon_style = if cp.approved {
                theme.style_success()
            } else {
                risk_style(cp.risk, theme)
            };
            let cursor = if i == app.layout.planning.checkpoint_cursor && !cp.approved {
                "▸ "
            } else {
                "  "
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(cursor, theme.style_accent()),
                Span::styled(icon, icon_style),
                Span::raw(" "),
                Span::styled(
                    format!("[{}] ", cp.risk.label()),
                    risk_style(cp.risk, theme),
                ),
                Span::styled(&cp.description, theme.style_fg()),
            ]));
        }
    }

    // Phases summary
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Phases", theme.style_fg().add_modifier(Modifier::BOLD)),
    ]));
    for (i, phase) in project.phases.iter().enumerate() {
        let is_selected = i == app.layout.planning.selected_row;
        let marker = if is_selected { "▸ " } else { "  " };
        let name_style = if is_selected {
            theme.style_fg().add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(marker, theme.style_accent()),
            Span::styled(&phase.name, name_style),
            Span::raw("  "),
            Span::styled(phase.state.label(), phase_state_style(phase.state, theme)),
            Span::raw("  "),
            Span::styled(progress_bar(phase.completion_pct, 20), theme.style_dim()),
            Span::styled(format!(" {}%", phase.completion_pct), theme.style_dim()),
        ]));

        let expanded = app
            .layout
            .planning
            .expanded_phases
            .get(i)
            .copied()
            .unwrap_or(false);
        if expanded {
            for plan in &phase.plans {
                lines.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled(format!("[w{}] ", plan.wave), theme.style_dim()),
                    Span::styled(&plan.title, theme.style_fg()),
                    Span::raw("  "),
                    Span::styled(plan.state.label(), plan_state_style(plan.state, theme)),
                ]));
            }
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_requirements(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(ref project) = app.layout.planning.project else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Requirements",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));

    // Header
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!(
                "{:<10} {:<40} {:<12} {}",
                "ID", "Description", "Status", "Source"
            ),
            theme.style_dim().add_modifier(Modifier::UNDERLINED),
        ),
    ]));

    for (i, req) in project.requirements.iter().enumerate() {
        let is_selected = i == app.layout.planning.selected_row;
        let marker = if is_selected { "▸" } else { " " };
        let row_style = if is_selected {
            theme.style_fg().add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        let desc = truncate_str(&req.description, 40);
        let status_style = requirement_status_style(req.status, theme);

        lines.push(Line::from(vec![
            Span::styled(format!(" {marker}"), theme.style_accent()),
            Span::styled(format!("{:<10} ", req.id), row_style),
            Span::styled(format!("{desc:<40} "), row_style),
            Span::styled(format!("{:<12} ", req.status.label()), status_style),
            Span::styled(&req.source, theme.style_dim()),
        ]));
    }

    if project.requirements.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No requirements defined.", theme.style_dim()),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_execution(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(ref project) = app.layout.planning.project else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Execution — Wave-Based Parallel Phases",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));

    for (i, phase) in project.phases.iter().enumerate() {
        let is_selected = i == app.layout.planning.selected_row;
        let marker = if is_selected { "▸ " } else { "  " };
        let name_style = if is_selected {
            theme.style_fg().add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(marker, theme.style_accent()),
            Span::styled(&phase.name, name_style),
            Span::raw("  "),
            Span::styled(phase.state.label(), phase_state_style(phase.state, theme)),
        ]));

        // Group plans by wave
        let max_wave = phase.plans.iter().map(|p| p.wave).max().unwrap_or(0);
        for wave in 1..=max_wave {
            let wave_plans: Vec<_> = phase.plans.iter().filter(|p| p.wave == wave).collect();
            if wave_plans.is_empty() {
                continue;
            }

            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("Wave {wave}"),
                    theme.style_dim().add_modifier(Modifier::UNDERLINED),
                ),
            ]));

            for plan in wave_plans {
                let deps = if plan.depends_on.is_empty() {
                    String::new()
                } else {
                    format!(" (blocked by: {})", plan.depends_on.join(", "))
                };
                lines.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled(plan.state.label(), plan_state_style(plan.state, theme)),
                    Span::raw("  "),
                    Span::styled(&plan.title, theme.style_fg()),
                    Span::styled(deps, theme.style_dim()),
                ]));
            }
        }

        // Progress bar
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(progress_bar(phase.completion_pct, 30), theme.style_dim()),
            Span::styled(format!(" {}%", phase.completion_pct), theme.style_dim()),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_verification(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(ref project) = app.layout.planning.project else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Verification",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));

    for (i, v) in project.verifications.iter().enumerate() {
        let is_selected = i == app.layout.planning.selected_row;
        let marker = if is_selected { "▸" } else { " " };
        let icon = if v.passed { "\u{2713}" } else { "\u{2717}" };
        let icon_style = if v.passed {
            theme.style_success()
        } else {
            theme.style_error()
        };
        let name_style = if is_selected {
            theme.style_fg().add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), theme.style_accent()),
            Span::styled(icon, icon_style),
            Span::raw("  "),
            Span::styled(&v.name, name_style),
        ]));
        if !v.evidence.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(&v.evidence, theme.style_dim()),
            ]));
        }
    }

    if project.verifications.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No verifications recorded.", theme.style_dim()),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_tasks(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(ref project) = app.layout.planning.project else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Task List",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));

    for (i, task) in project.tasks.iter().enumerate() {
        let is_selected = i == app.layout.planning.selected_row;
        let marker = if is_selected { "▸" } else { " " };
        let indent = "  ".repeat(usize::from(task.depth) + 1);
        let name_style = if is_selected {
            theme.style_fg().add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        let blocked_info = if task.blocked_by.is_empty() {
            String::new()
        } else {
            format!("  [blocked by: {}]", task.blocked_by.join(", "))
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {marker}"), theme.style_accent()),
            Span::raw(indent),
            Span::styled(task.state.label(), plan_state_style(task.state, theme)),
            Span::raw("  "),
            Span::styled(&task.title, name_style),
            Span::styled(blocked_info, theme.style_error()),
        ]));
    }

    if project.tasks.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No tasks defined.", theme.style_dim()),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_timeline(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(ref project) = app.layout.planning.project else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Timeline",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));

    for (i, milestone) in project.milestones.iter().enumerate() {
        let is_selected = i == app.layout.planning.selected_row;
        let marker = if is_selected { "▸" } else { " " };
        let icon = if milestone.completed {
            "\u{25c6}"
        } else {
            "\u{25c7}"
        };
        let icon_style = if milestone.completed {
            theme.style_success()
        } else {
            theme.style_dim()
        };
        let name_style = if is_selected {
            theme.style_fg().add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), theme.style_accent()),
            Span::styled(icon, icon_style),
            Span::raw("  "),
            Span::styled(&milestone.timestamp, theme.style_dim()),
            Span::raw("  "),
            Span::styled(&milestone.label, name_style),
        ]));
    }

    if project.milestones.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No milestones recorded.", theme.style_dim()),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_edit_history(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(ref project) = app.layout.planning.project else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Edit History",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));

    for (i, entry) in project.edit_history.iter().enumerate() {
        let is_selected = i == app.layout.planning.selected_row;
        let marker = if is_selected { "▸" } else { " " };
        let name_style = if is_selected {
            theme.style_fg().add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), theme.style_accent()),
            Span::styled(&entry.timestamp, theme.style_dim()),
            Span::raw("  "),
            Span::styled(&entry.description, name_style),
            Span::raw("  "),
            Span::styled(format!("({})", entry.author), theme.style_dim()),
        ]));
    }

    if project.edit_history.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No edit history.", theme.style_dim()),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Render the retrospective post-mortem view for completed projects.
pub(crate) fn render_retrospective(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let state = &app.layout.retrospective;

    if state.loading {
        let loading = Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("Loading retrospective...", theme.style_dim()),
        ]));
        frame.render_widget(loading, area);
        return;
    }

    let Some(ref entry) = state.entry else {
        let empty = Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "No retrospective data. Only completed projects have retrospectives.",
                theme.style_dim(),
            ),
        ]));
        frame.render_widget(empty, area);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("Retrospective — {}", entry.project_name),
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));

    // Section navigation
    lines.push(Line::from(vec![
        Span::raw("  "),
        section_span(
            "Successes",
            RetrospectiveSection::Successes,
            state.selected_section,
            theme,
        ),
        Span::raw("  "),
        section_span(
            "Blockers",
            RetrospectiveSection::Blockers,
            state.selected_section,
            theme,
        ),
        Span::raw("  "),
        section_span(
            "Lessons",
            RetrospectiveSection::Lessons,
            state.selected_section,
            theme,
        ),
        Span::raw("  "),
        section_span(
            "Decisions",
            RetrospectiveSection::Decisions,
            state.selected_section,
            theme,
        ),
    ]));
    lines.push(Line::raw(""));

    match state.selected_section {
        RetrospectiveSection::Successes => {
            for s in &entry.successes {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("\u{2713} ", theme.style_success()),
                    Span::styled(s.as_str(), theme.style_fg()),
                ]));
            }
            if entry.successes.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("No successes recorded.", theme.style_dim()),
                ]));
            }
        }
        RetrospectiveSection::Blockers => {
            for b in &entry.blockers {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("\u{2717} ", theme.style_error()),
                    Span::styled(b.as_str(), theme.style_fg()),
                ]));
            }
            if entry.blockers.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("No blockers recorded.", theme.style_dim()),
                ]));
            }
        }
        RetrospectiveSection::Lessons => {
            for l in &entry.lessons {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("\u{25b8} ", theme.style_accent()),
                    Span::styled(l.as_str(), theme.style_fg()),
                ]));
            }
            if entry.lessons.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("No lessons recorded.", theme.style_dim()),
                ]));
            }
        }
        RetrospectiveSection::Decisions => {
            for d in &entry.decisions {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(&d.timestamp, theme.style_dim()),
                    Span::raw("  "),
                    Span::styled(&d.question, theme.style_fg().add_modifier(Modifier::BOLD)),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("    Choice: "),
                    Span::styled(&d.choice, theme.style_accent()),
                ]));
                if !d.rationale.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("    Why: "),
                        Span::styled(&d.rationale, theme.style_dim()),
                    ]));
                }
                lines.push(Line::raw(""));
            }
            if entry.decisions.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("No decisions recorded.", theme.style_dim()),
                ]));
            }
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Tab", theme.style_accent()),
        Span::styled(" section  ", theme.style_dim()),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" back", theme.style_dim()),
    ]));

    let scroll = u16::try_from(state.scroll_offset).unwrap_or(u16::MAX);
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

// -- Style helpers --

fn state_style(state: DisplayProjectState, theme: &Theme) -> Style {
    match state {
        DisplayProjectState::Complete => theme.style_success(),
        DisplayProjectState::Failed => theme.style_error(),
        DisplayProjectState::Executing => theme.style_accent(),
        DisplayProjectState::Paused => Style::default().fg(theme.status.warning),
        _ => theme.style_dim(),
    }
}

fn phase_state_style(state: DisplayPhaseState, theme: &Theme) -> Style {
    match state {
        DisplayPhaseState::Complete => theme.style_success(),
        DisplayPhaseState::Failed => theme.style_error(),
        DisplayPhaseState::Active | DisplayPhaseState::Executing => theme.style_accent(),
        DisplayPhaseState::Verifying => Style::default().fg(theme.status.warning),
        DisplayPhaseState::Pending => theme.style_dim(),
    }
}

fn plan_state_style(state: DisplayPlanState, theme: &Theme) -> Style {
    match state {
        DisplayPlanState::Complete => theme.style_success(),
        DisplayPlanState::Failed | DisplayPlanState::Stuck => theme.style_error(),
        DisplayPlanState::Executing => theme.style_accent(),
        DisplayPlanState::Ready => Style::default().fg(theme.status.warning),
        DisplayPlanState::Pending | DisplayPlanState::Skipped => theme.style_dim(),
    }
}

fn risk_style(risk: CheckpointRisk, theme: &Theme) -> Style {
    match risk {
        CheckpointRisk::Low => theme.style_success(),
        CheckpointRisk::Medium => Style::default().fg(theme.status.warning),
        CheckpointRisk::High | CheckpointRisk::Critical => theme.style_error(),
    }
}

fn requirement_status_style(status: RequirementStatus, theme: &Theme) -> Style {
    match status {
        RequirementStatus::Met => theme.style_success(),
        RequirementStatus::Failed => theme.style_error(),
        RequirementStatus::InProgress => theme.style_accent(),
        RequirementStatus::Pending => theme.style_dim(),
        RequirementStatus::Deferred => Style::default().fg(theme.status.warning),
    }
}

fn section_span<'a>(
    label: &'a str,
    section: RetrospectiveSection,
    current: RetrospectiveSection,
    theme: &Theme,
) -> Span<'a> {
    if section == current {
        Span::styled(
            format!("[{label}]"),
            theme.style_accent().add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(format!(" {label} "), theme.style_dim())
    }
}

/// Render a text-based progress bar.
pub(crate) fn progress_bar(pct: u8, width: usize) -> String {
    let filled = (usize::from(pct) * width) / 100;
    let empty = width.saturating_sub(filled);
    format!(
        "[{}{}]",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(empty)
    )
}

/// Truncate a string to a maximum length, appending "..." if truncated.
pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = max.saturating_sub(3);
    let mut boundary = end;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let prefix = s.get(..boundary).unwrap_or(s);
    format!("{prefix}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_bar_zero() {
        let bar = progress_bar(0, 10);
        assert_eq!(
            bar,
            "[\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}]"
        );
    }

    #[test]
    fn progress_bar_full() {
        let bar = progress_bar(100, 10);
        assert_eq!(
            bar,
            "[\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}]"
        );
    }

    #[test]
    fn progress_bar_half() {
        let bar = progress_bar(50, 10);
        assert_eq!(
            bar,
            "[\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}]"
        );
    }

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_str_long() {
        let result = truncate_str("hello world", 8);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn truncate_str_multibyte() {
        let s = "hello\u{00e9}world";
        let result = truncate_str(s, 8);
        assert!(result.is_char_boundary(result.len()));
        assert!(result.ends_with("..."));
    }
}
