//! Rendering for the memory inspector panel views.

use ratatui::Frame;

/// Height of the tab bar row in the memory inspector layout.
const TAB_BAR_HEIGHT: u16 = 2;
/// Minimum height for the content area in the memory inspector layout.
const CONTENT_MIN_HEIGHT: u16 = 3;
/// Height of the status/help bar at the bottom of the memory inspector.
const STATUS_BAR_HEIGHT: u16 = 1;
/// Column width reserved for confidence, tier, and type columns in the facts table.
const RESERVED_COLUMN_WIDTH: u16 = 28;
/// Maximum number of relationships shown in the graph view before truncating.
const RELATIONSHIP_DISPLAY_LIMIT: usize = 20;
/// Maximum content length for similar-fact snippets shown in the detail view.
const SIMILAR_FACT_TRUNCATE_LEN: usize = 60;
/// Column width for the fact type field in the facts table.
const FACT_TYPE_COLUMN_WIDTH: usize = 10;
/// Confidence threshold above which a fact is considered high-confidence.
const CONFIDENCE_HIGH_THRESHOLD: f64 = 0.8;
/// Confidence threshold above which a fact is considered medium-confidence.
const CONFIDENCE_MED_THRESHOLD: f64 = 0.5;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::state::memory::{MemoryInspectorState, MemoryTab};
use crate::theme::Theme;

/// Render the main memory inspector view (fact browser, graph, timeline tabs).
pub(crate) fn render_inspector(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(TAB_BAR_HEIGHT),
            Constraint::Min(CONTENT_MIN_HEIGHT),
            Constraint::Length(STATUS_BAR_HEIGHT),
        ])
        .split(area);

    render_tab_bar(app, frame, layout[0], theme);

    match app.layout.memory.tab {
        MemoryTab::Facts => render_facts_table(app, frame, layout[1], theme),
        MemoryTab::Graph => render_graph_view(app, frame, layout[1], theme),
        MemoryTab::Timeline => render_timeline_view(app, frame, layout[1], theme),
    }

    render_memory_status(app, frame, layout[2], theme);
}

/// Render the fact detail view.
pub(crate) fn render_fact_detail(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if let Some(ref detail) = app.layout.memory.fact_list.detail {
        let f = &detail.fact;

        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Fact Detail",
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::raw(""));

        let tier_label = MemoryInspectorState::tier_abbrev(&f.tier);
        lines.push(meta_line(theme, "ID", &f.id));
        lines.push(meta_line(
            theme,
            "Tier",
            &format!("{tier_label} ({})", f.tier),
        ));
        lines.push(meta_line(
            theme,
            "Confidence",
            &format!("{:.0}%", f.confidence * 100.0),
        ));
        lines.push(meta_line(theme, "Type", &f.fact_type));
        lines.push(meta_line(
            theme,
            "Created",
            &MemoryInspectorState::relative_time(&f.recorded_at),
        ));
        lines.push(meta_line(
            theme,
            "Last Seen",
            &MemoryInspectorState::relative_time(&f.last_accessed_at),
        ));
        lines.push(meta_line(theme, "Accesses", &f.access_count.to_string()));
        lines.push(meta_line(
            theme,
            "Stability",
            &format!("{:.0}h", f.stability_hours),
        ));
        if !f.valid_from.is_empty() {
            lines.push(meta_line(theme, "Valid From", &f.valid_from));
        }
        if !f.valid_to.is_empty() && f.valid_to != "9999-12-31" {
            lines.push(meta_line(theme, "Valid To", &f.valid_to));
        }
        if f.is_forgotten {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Status: ", theme.style_dim()),
                Span::styled("FORGOTTEN", theme.style_error()),
            ]));
        }

        lines.push(Line::raw(""));

        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Content:", theme.style_fg().add_modifier(Modifier::BOLD)),
        ]));
        for content_line in f.content.lines() {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(content_line.to_string(), theme.style_fg()),
            ]));
        }

        if !detail.relationships.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("Relationships ({}):", detail.relationships.len()),
                    theme.style_fg().add_modifier(Modifier::BOLD),
                ),
            ]));
            for rel in &detail.relationships {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(&rel.src, theme.style_accent()),
                    Span::styled(format!(" ─{}─▸ ", rel.relation), theme.style_dim()),
                    Span::styled(&rel.dst, theme.style_accent()),
                ]));
            }
        }

        if !detail.similar.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("Similar Facts ({}):", detail.similar.len()),
                    theme.style_fg().add_modifier(Modifier::BOLD),
                ),
            ]));
            for sim in &detail.similar {
                let truncated = truncate(&sim.content, SIMILAR_FACT_TRUNCATE_LEN);
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        format!("{:.0}% ", sim.similarity * 100.0),
                        theme.style_warning(),
                    ),
                    Span::styled(truncated, theme.style_fg()),
                ]));
            }
        }
    } else if app.layout.memory.loading {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Loading fact detail...", theme.style_dim()),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No fact data available.", theme.style_dim()),
        ]));
    }

    if app.layout.memory.fact_list.editing_confidence {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Set confidence (0.0–1.0): ",
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &app.layout.memory.fact_list.confidence_buffer,
                theme.style_fg().add_modifier(Modifier::UNDERLINED),
            ),
            Span::styled("▌", theme.style_accent()),
        ]));
    }

    lines.push(Line::raw(""));
    let mut help = vec![
        Span::raw("  "),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" back  ", theme.style_dim()),
        Span::styled("e", theme.style_accent()),
        Span::styled(" edit confidence  ", theme.style_dim()),
    ];
    if app
        .layout
        .memory
        .fact_list
        .detail
        .as_ref()
        .is_some_and(|d| d.fact.is_forgotten)
    {
        help.push(Span::styled("r", theme.style_accent()));
        help.push(Span::styled(" restore", theme.style_dim()));
    } else {
        help.push(Span::styled("d", theme.style_accent()));
        help.push(Span::styled(" forget", theme.style_dim()));
    }
    lines.push(Line::from(help));

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_tab_bar(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let tabs = [MemoryTab::Facts, MemoryTab::Graph, MemoryTab::Timeline];
    let mut spans: Vec<Span> = vec![Span::raw(" ")];

    for (i, tab) in tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", theme.style_dim()));
        }
        let style = if *tab == app.layout.memory.tab {
            theme.style_accent().add_modifier(Modifier::BOLD)
        } else {
            theme.style_dim()
        };
        spans.push(Span::styled(tab.label(), style));
    }

    spans.push(Span::raw("  "));
    let arrow = if app.layout.memory.fact_list.sort_asc {
        "↑"
    } else {
        "↓"
    };
    spans.push(Span::styled(
        format!(
            "[s]ort: {} {arrow}",
            app.layout.memory.fact_list.sort.label()
        ),
        theme.style_dim(),
    ));

    if app.layout.memory.search.search_active {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("search: {}", app.layout.memory.search.search_query),
            theme.style_warning(),
        ));
        spans.push(Span::styled("▌", theme.style_accent()));
    }

    if app.layout.memory.filters.filter_editing {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("filter: {}", app.layout.memory.filters.filter_text),
            theme.style_warning(),
        ));
        spans.push(Span::styled("▌", theme.style_accent()));
    } else if !app.layout.memory.filters.filter_text.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("filter: {}", app.layout.memory.filters.filter_text),
            theme.style_muted(),
        ));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(vec![line, Line::raw("")]);
    frame.render_widget(paragraph, area);
}

fn render_facts_table(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    let header_style = theme.style_dim().add_modifier(Modifier::BOLD);
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("Conf  ", header_style),
        Span::styled("Tier ", header_style),
        Span::styled("Type       ", header_style),
        Span::styled("Content", header_style),
    ]));

    if app.layout.memory.fact_list.facts.is_empty() {
        if app.layout.memory.loading {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Loading facts...", theme.style_dim()),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("No facts found.", theme.style_dim()),
            ]));
        }
    } else {
        let filter = app.layout.memory.filters.filter_text.to_lowercase();
        let visible_height = area.height.saturating_sub(1) as usize;
        let content_width = area.width.saturating_sub(RESERVED_COLUMN_WIDTH) as usize;

        let type_filter = app.layout.memory.filters.type_filter.as_deref();
        let tier_filter = app.layout.memory.filters.tier_filter.as_deref();

        let filtered_facts: Vec<(usize, &crate::state::memory::MemoryFact)> = app
            .layout
            .memory
            .fact_list
            .facts
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                if let Some(tf) = type_filter
                    && f.fact_type != tf
                {
                    return false;
                }
                if let Some(tf) = tier_filter
                    && f.tier != tf
                {
                    return false;
                }
                if filter.is_empty() {
                    return true;
                }
                f.content.to_lowercase().contains(&filter)
                    || f.tier.to_lowercase().contains(&filter)
                    || f.fact_type.to_lowercase().contains(&filter)
            })
            .collect();

        let start = app.layout.memory.fact_list.scroll_offset;
        let end = (start + visible_height).min(filtered_facts.len());

        for &(original_idx, fact) in &filtered_facts[start..end] {
            let is_selected = original_idx == app.layout.memory.fact_list.selected;
            let marker = if is_selected { "▸ " } else { "  " };
            let marker_style = if is_selected {
                Style::default().fg(theme.borders.selected)
            } else {
                Style::default()
            };

            let conf_str = format!("{:.0}%   ", fact.confidence * 100.0);
            let conf_style = confidence_style(theme, fact.confidence);

            let tier_str = format!("{} ", MemoryInspectorState::tier_abbrev(&fact.tier));
            let tier_style = tier_style(theme, &fact.tier);

            let type_str = format!(
                "{:<width$} ",
                truncate(&fact.fact_type, FACT_TYPE_COLUMN_WIDTH),
                width = FACT_TYPE_COLUMN_WIDTH
            );

            let content = truncate(&fact.content.replace('\n', " "), content_width);
            let content_style = if fact.is_forgotten {
                theme.style_dim().add_modifier(Modifier::CROSSED_OUT)
            } else if is_selected {
                theme.style_fg().add_modifier(Modifier::BOLD)
            } else {
                theme.style_fg()
            };

            lines.push(Line::from(vec![
                Span::styled(marker, marker_style),
                Span::styled(conf_str, conf_style),
                Span::styled(tier_str, tier_style),
                Span::styled(type_str, theme.style_dim()),
                Span::styled(content, content_style),
            ]));
        }
    }

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_graph_view(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if app.layout.memory.graph.entities.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Entity Graph",
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No entities loaded.", theme.style_dim()),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Entities are loaded from the knowledge API.",
                theme.style_dim(),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!(
                    "Entity Graph ({} entities)",
                    app.layout.memory.graph.entities.len()
                ),
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::raw(""));

        for (i, entity) in app.layout.memory.graph.entities.iter().enumerate() {
            let is_selected = i == app.layout.memory.fact_list.selected;
            let marker = if is_selected { "▸ " } else { "  " };

            let rel_count = app
                .layout
                .memory
                .graph
                .relationships
                .iter()
                .filter(|r| r.src == entity.id || r.dst == entity.id)
                .count();

            lines.push(Line::from(vec![
                Span::styled(
                    marker,
                    if is_selected {
                        Style::default().fg(theme.borders.selected)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(&entity.name, theme.style_accent()),
                Span::styled(format!(" ({})", entity.entity_type), theme.style_dim()),
                Span::styled(format!("  {rel_count} rels"), theme.style_muted()),
            ]));
        }

        if !app.layout.memory.graph.relationships.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Relationships:",
                    theme.style_fg().add_modifier(Modifier::BOLD),
                ),
            ]));
            for rel in app
                .layout
                .memory
                .graph
                .relationships
                .iter()
                .take(RELATIONSHIP_DISPLAY_LIMIT)
            {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(&rel.src, theme.style_accent()),
                    Span::styled(format!(" ─{}─▸ ", rel.relation), theme.style_dim()),
                    Span::styled(&rel.dst, theme.style_accent()),
                ]));
            }
        }
    }

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_timeline_view(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Timeline",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::raw(""));

    if app.layout.memory.graph.timeline_events.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No timeline events.", theme.style_dim()),
        ]));
    } else {
        for (i, event) in app.layout.memory.graph.timeline_events.iter().enumerate() {
            let is_selected = i == app.layout.memory.fact_list.selected;
            let marker = if is_selected { "▸ " } else { "  " };

            let time = MemoryInspectorState::relative_time(&event.timestamp);
            let event_icon = match event.event_type.as_str() {
                "created" => "+",
                "modified" => "~",
                "forgotten" => "×",
                "restored" => "↺",
                "accessed" => "○",
                _ => "·",
            };

            let conf_str = event
                .confidence
                .map(|c| format!(" {:.0}%", c * 100.0))
                .unwrap_or_default();

            lines.push(Line::from(vec![
                Span::styled(
                    marker,
                    if is_selected {
                        Style::default().fg(theme.borders.selected)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(time, theme.style_dim()),
                Span::raw(" "),
                Span::styled(event_icon, event_type_style(theme, &event.event_type)),
                Span::raw(" "),
                Span::styled(&event.description, theme.style_fg()),
                Span::styled(conf_str, theme.style_muted()),
            ]));
        }
    }

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_memory_status(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let total = app.layout.memory.fact_list.total_facts;
    let visible = app.layout.memory.fact_list.facts.len();
    let sel = app.layout.memory.fact_list.selected + 1;

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(format!("{sel}/{visible}"), theme.style_fg()),
    ];

    if total > visible {
        spans.push(Span::styled(format!(" of {total}"), theme.style_dim()));
    }

    spans.push(Span::raw("  "));
    spans.push(Span::styled("Tab", theme.style_accent()));
    spans.push(Span::styled(" tabs  ", theme.style_dim()));
    spans.push(Span::styled("j/k", theme.style_accent()));
    spans.push(Span::styled(" nav  ", theme.style_dim()));
    spans.push(Span::styled("Enter", theme.style_accent()));
    spans.push(Span::styled(" detail  ", theme.style_dim()));
    spans.push(Span::styled("/", theme.style_accent()));
    spans.push(Span::styled(" search  ", theme.style_dim()));
    spans.push(Span::styled("f", theme.style_accent()));
    spans.push(Span::styled(" filter  ", theme.style_dim()));
    spans.push(Span::styled("Esc", theme.style_accent()));
    spans.push(Span::styled(" close", theme.style_dim()));

    let line = Line::from(spans);
    let paragraph = Paragraph::new(vec![line]);
    frame.render_widget(paragraph, area);
}

fn meta_line<'a>(theme: &'a Theme, label: &'a str, value: &str) -> Line<'a> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{label}: "), theme.style_dim()),
        Span::styled(value.to_string(), theme.style_fg()),
    ])
}

fn confidence_style(theme: &Theme, conf: f64) -> Style {
    if conf >= CONFIDENCE_HIGH_THRESHOLD {
        theme.style_success()
    } else if conf >= CONFIDENCE_MED_THRESHOLD {
        theme.style_warning()
    } else {
        theme.style_error()
    }
}

fn tier_style(theme: &Theme, tier: &str) -> Style {
    match tier {
        "verified" => theme.style_success(),
        "inferred" => theme.style_warning(),
        "assumed" => theme.style_muted(),
        _ => theme.style_dim(),
    }
}

fn event_type_style(theme: &Theme, event_type: &str) -> Style {
    match event_type {
        "created" => theme.style_success(),
        "modified" => theme.style_warning(),
        "forgotten" => theme.style_error(),
        "restored" => theme.style_accent(),
        _ => theme.style_dim(),
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let result = truncate("this is a very long string", 10);
        assert!(result.len() <= 12); // 9 chars + ellipsis (multi-byte)
        assert!(result.ends_with('…'));
    }

    #[test]
    fn confidence_style_high() {
        let theme = Theme::detect();
        let style = confidence_style(&theme, 0.95);
        assert_eq!(style, theme.style_success());
    }

    #[test]
    fn confidence_style_medium() {
        let theme = Theme::detect();
        let style = confidence_style(&theme, 0.6);
        assert_eq!(style, theme.style_warning());
    }

    #[test]
    fn confidence_style_low() {
        let theme = Theme::detect();
        let style = confidence_style(&theme, 0.3);
        assert_eq!(style, theme.style_error());
    }

    #[test]
    fn tier_style_verified() {
        let theme = Theme::detect();
        assert_eq!(tier_style(&theme, "verified"), theme.style_success());
    }

    #[test]
    fn tier_style_inferred() {
        let theme = Theme::detect();
        assert_eq!(tier_style(&theme, "inferred"), theme.style_warning());
    }

    #[test]
    fn tier_style_unknown() {
        let theme = Theme::detect();
        assert_eq!(tier_style(&theme, "other"), theme.style_dim());
    }

    #[test]
    fn event_type_style_created() {
        let theme = Theme::detect();
        assert_eq!(event_type_style(&theme, "created"), theme.style_success());
    }

    #[test]
    fn event_type_style_forgotten() {
        let theme = Theme::detect();
        assert_eq!(event_type_style(&theme, "forgotten"), theme.style_error());
    }
}
