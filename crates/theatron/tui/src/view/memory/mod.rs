// Rendering for the memory inspector panel views.

use ratatui::Frame;

/// Height of the tab bar row in the memory inspector layout.
const TAB_BAR_HEIGHT: u16 = 2;
/// Minimum height for the content area in the memory inspector layout.
const CONTENT_MIN_HEIGHT: u16 = 3;
/// Height of the status/help bar at the bottom of the memory inspector.
const STATUS_BAR_HEIGHT: u16 = 1;
/// Column width reserved for confidence, tier, and type columns in the facts table.
const RESERVED_COLUMN_WIDTH: u16 = 28;
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
#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as there are constraints; indices 0/1/2 match the three constraints defined above"
)]
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
        MemoryTab::Drift => render_drift_view(app, frame, layout[1], theme),
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
            &MemoryInspectorState::relative_time(&f.temporal.recorded_at),
        ));
        lines.push(meta_line(
            theme,
            "Last Seen",
            &MemoryInspectorState::relative_time(&f.temporal.last_accessed_at),
        ));
        lines.push(meta_line(
            theme,
            "Accesses",
            &f.temporal.access_count.to_string(),
        ));
        lines.push(meta_line(
            theme,
            "Stability",
            &format!("{:.0}h", f.temporal.stability_hours),
        ));
        if !f.temporal.valid_from.is_empty() {
            lines.push(meta_line(theme, "Valid From", &f.temporal.valid_from));
        }
        if !f.temporal.valid_to.is_empty() && f.temporal.valid_to != "9999-12-31" {
            lines.push(meta_line(theme, "Valid To", &f.temporal.valid_to));
        }
        if f.lifecycle.is_forgotten {
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
        .is_some_and(|d| d.fact.lifecycle.is_forgotten)
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
    let tabs = [
        MemoryTab::Facts,
        MemoryTab::Graph,
        MemoryTab::Drift,
        MemoryTab::Timeline,
    ];
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

#[expect(
    clippy::indexing_slicing,
    reason = "end = min(start + height, len) ensures start..end is always a valid slice range"
)]
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
        let visible_height = usize::from(area.height.saturating_sub(1));
        let content_width = usize::from(area.width.saturating_sub(RESERVED_COLUMN_WIDTH));

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
            let content_style = if fact.lifecycle.is_forgotten {
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

/// Height of the health bar in the graph view.
const HEALTH_BAR_HEIGHT: u16 = 2;

#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; indices match"
)]
fn render_graph_view(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(HEALTH_BAR_HEIGHT), Constraint::Min(3)])
        .split(area);

    render_health_bar(app, frame, layout[0], theme);
    render_entity_list(app, frame, layout[1], theme);
}

fn render_health_bar(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let h = &app.layout.memory.graph.health;
    let spans = vec![
        Span::raw(" "),
        Span::styled(format!("{}", h.total_entities), theme.style_accent()),
        Span::styled(" entities  ", theme.style_dim()),
        Span::styled(format!("{}", h.total_relationships), theme.style_accent()),
        Span::styled(" rels  ", theme.style_dim()),
        Span::styled(
            format!("{}", h.orphan_count),
            if h.orphan_count > 0 {
                theme.style_warning()
            } else {
                theme.style_success()
            },
        ),
        Span::styled(" orphans  ", theme.style_dim()),
        Span::styled(
            format!("{}", h.stale_count),
            if h.stale_count > 0 {
                theme.style_warning()
            } else {
                theme.style_success()
            },
        ),
        Span::styled(" stale  ", theme.style_dim()),
        Span::styled(format!("{:.1}", h.avg_cluster_size), theme.style_accent()),
        Span::styled(" avg/cluster  ", theme.style_dim()),
        Span::styled(format!("{}", h.community_count), theme.style_accent()),
        Span::styled(" communities  ", theme.style_dim()),
        Span::styled(
            format!("{}", h.isolated_cluster_count),
            if h.isolated_cluster_count > 0 {
                theme.style_warning()
            } else {
                theme.style_success()
            },
        ),
        Span::styled(" isolated", theme.style_dim()),
    ];
    let line = Line::from(spans);
    let paragraph = Paragraph::new(vec![line, Line::raw("")]);
    frame.render_widget(paragraph, area);
}

#[expect(
    clippy::indexing_slicing,
    reason = "end = min(start + height, len) ensures valid slice range"
)]
fn render_entity_list(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if app.layout.memory.graph.entity_stats.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No entities loaded.", theme.style_dim()),
        ]));
    } else {
        let header_style = theme.style_dim().add_modifier(Modifier::BOLD);
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("Name              ", header_style),
            Span::styled("Type       ", header_style),
            Span::styled("Rels  ", header_style),
            Span::styled("PR     ", header_style),
            Span::styled("Community", header_style),
        ]));

        let visible_height = usize::from(area.height.saturating_sub(1));
        let start = app.layout.memory.graph.entity_scroll_offset;
        let end = (start + visible_height).min(app.layout.memory.graph.entity_stats.len());

        for (offset, stat) in app.layout.memory.graph.entity_stats[start..end]
            .iter()
            .enumerate()
        {
            let idx = start + offset;
            let is_selected = idx == app.layout.memory.graph.selected_entity;
            let marker = if is_selected { "▸ " } else { "  " };
            let marker_style = if is_selected {
                Style::default().fg(theme.borders.selected)
            } else {
                Style::default()
            };

            let name = truncate(&stat.entity.name, 16);
            let name_style = if is_selected {
                theme.style_accent().add_modifier(Modifier::BOLD)
            } else {
                theme.style_accent()
            };

            let entity_type = truncate(&stat.entity.entity_type, 9);
            let community_label = stat
                .community_id
                .map(|c| format!("#{c}"))
                .unwrap_or_else(|| "---".into());

            let rel_style = if stat.relationship_count == 0 {
                theme.style_error()
            } else {
                theme.style_fg()
            };

            lines.push(Line::from(vec![
                Span::styled(marker, marker_style),
                Span::styled(format!("{name:<16}  "), name_style),
                Span::styled(format!("{entity_type:<9}  "), theme.style_dim()),
                Span::styled(format!("{:<4}  ", stat.relationship_count), rel_style),
                Span::styled(format!("{:<5.2}  ", stat.pagerank), theme.style_fg()),
                Span::styled(community_label, theme.style_muted()),
            ]));
        }
    }

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_drift_view(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    // WHY: drift sub-tab bar
    let drift_tabs = [
        crate::state::memory::DriftTab::Suggestions,
        crate::state::memory::DriftTab::Orphans,
        crate::state::memory::DriftTab::Stale,
        crate::state::memory::DriftTab::Isolated,
    ];
    let mut tab_spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, tab) in drift_tabs.iter().enumerate() {
        if i > 0 {
            tab_spans.push(Span::styled(" │ ", theme.style_dim()));
        }
        let style = if *tab == app.layout.memory.graph.drift_tab {
            theme.style_accent().add_modifier(Modifier::BOLD)
        } else {
            theme.style_dim()
        };
        tab_spans.push(Span::styled(tab.label(), style));
    }
    tab_spans.push(Span::styled("   [/] cycle", theme.style_muted()));
    lines.push(Line::from(tab_spans));
    lines.push(Line::raw(""));

    match app.layout.memory.graph.drift_tab {
        crate::state::memory::DriftTab::Suggestions => {
            if app.layout.memory.graph.drift_suggestions.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("No drift suggestions.", theme.style_dim()),
                ]));
            } else {
                for (i, s) in app.layout.memory.graph.drift_suggestions.iter().enumerate() {
                    let is_selected = i == app.layout.memory.graph.drift_selected;
                    let marker = if is_selected { "▸ " } else { "  " };
                    let action_style = match s.action.as_str() {
                        "delete" => theme.style_error(),
                        "merge" => theme.style_warning(),
                        _ => theme.style_accent(),
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            marker,
                            if is_selected {
                                Style::default().fg(theme.borders.selected)
                            } else {
                                Style::default()
                            },
                        ),
                        Span::styled(format!("{:<7}", s.action), action_style),
                        Span::styled(&s.entity_name, theme.style_fg()),
                        Span::styled(format!(" — {}", s.reason), theme.style_dim()),
                    ]));
                }
            }
        }
        crate::state::memory::DriftTab::Orphans => {
            if app.layout.memory.graph.orphaned_entities.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("No orphaned entities.", theme.style_success()),
                ]));
            } else {
                for (i, name) in app.layout.memory.graph.orphaned_entities.iter().enumerate() {
                    let is_selected = i == app.layout.memory.graph.drift_selected;
                    let marker = if is_selected { "▸ " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(
                            marker,
                            if is_selected {
                                Style::default().fg(theme.borders.selected)
                            } else {
                                Style::default()
                            },
                        ),
                        Span::styled(name.as_str(), theme.style_warning()),
                        Span::styled("  0 rels", theme.style_dim()),
                    ]));
                }
            }
        }
        crate::state::memory::DriftTab::Stale => {
            if app.layout.memory.graph.stale_entities.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("No stale entities.", theme.style_success()),
                ]));
            } else {
                for (i, name) in app.layout.memory.graph.stale_entities.iter().enumerate() {
                    let is_selected = i == app.layout.memory.graph.drift_selected;
                    let marker = if is_selected { "▸ " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(
                            marker,
                            if is_selected {
                                Style::default().fg(theme.borders.selected)
                            } else {
                                Style::default()
                            },
                        ),
                        Span::styled(name.as_str(), theme.style_warning()),
                        Span::styled("  >30d since update", theme.style_dim()),
                    ]));
                }
            }
        }
        crate::state::memory::DriftTab::Isolated => {
            if app.layout.memory.graph.isolated_clusters.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("No isolated clusters.", theme.style_success()),
                ]));
            } else {
                for (i, cluster) in app.layout.memory.graph.isolated_clusters.iter().enumerate() {
                    let is_selected = i == app.layout.memory.graph.drift_selected;
                    let marker = if is_selected { "▸ " } else { "  " };
                    let members = cluster.entity_names.join(", ");
                    lines.push(Line::from(vec![
                        Span::styled(
                            marker,
                            if is_selected {
                                Style::default().fg(theme.borders.selected)
                            } else {
                                Style::default()
                            },
                        ),
                        Span::styled(format!("{{{members}}}"), theme.style_warning()),
                        Span::styled(format!("  {} entities", cluster.size), theme.style_dim()),
                    ]));
                }
            }
        }
    }

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Render the entity detail view (node card).
pub(crate) fn render_entity_detail(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if let Some(ref card) = app.layout.memory.graph.node_card {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Entity Detail",
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::raw(""));

        lines.push(meta_line(theme, "Name", &card.entity.name));
        lines.push(meta_line(theme, "Type", &card.entity.entity_type));
        if !card.entity.aliases.is_empty() {
            lines.push(meta_line(theme, "Aliases", &card.entity.aliases.join(", ")));
        }
        lines.push(meta_line(
            theme,
            "Created",
            &MemoryInspectorState::relative_time(&card.entity.created_at),
        ));
        lines.push(meta_line(
            theme,
            "Updated",
            &MemoryInspectorState::relative_time(&card.entity.updated_at),
        ));
        lines.push(meta_line(
            theme,
            "PageRank",
            &format!("{:.4}", card.pagerank),
        ));
        lines.push(meta_line(
            theme,
            "Community",
            &card
                .community_id
                .map(|c| format!("#{c}"))
                .unwrap_or_else(|| "none".into()),
        ));

        if !card.relationships_grouped.is_empty() {
            lines.push(Line::raw(""));
            let total_rels: usize = card
                .relationships_grouped
                .iter()
                .map(|(_, v)| v.len())
                .sum();
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("Relationships ({total_rels}):"),
                    theme.style_fg().add_modifier(Modifier::BOLD),
                ),
            ]));

            for (rel_type, rels) in &card.relationships_grouped {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(rel_type.as_str(), theme.style_accent()),
                    Span::styled(format!(" ({})", rels.len()), theme.style_dim()),
                ]));
                for rel in rels.iter().take(5) {
                    let other = if rel.src == card.entity.id {
                        &rel.dst
                    } else {
                        &rel.src
                    };
                    let direction = if rel.src == card.entity.id {
                        " ─▸ "
                    } else {
                        " ◂─ "
                    };
                    lines.push(Line::from(vec![
                        Span::raw("      "),
                        Span::styled(direction, theme.style_dim()),
                        Span::styled(other.as_str(), theme.style_fg()),
                    ]));
                }
                if rels.len() > 5 {
                    lines.push(Line::from(vec![
                        Span::raw("      "),
                        Span::styled(
                            format!("  ... and {} more", rels.len() - 5),
                            theme.style_muted(),
                        ),
                    ]));
                }
            }
        }

        if !card.related_facts.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("Related Facts ({}):", card.related_facts.len()),
                    theme.style_fg().add_modifier(Modifier::BOLD),
                ),
            ]));
            for fact in &card.related_facts {
                let tier_abbr = MemoryInspectorState::tier_abbrev(&fact.tier);
                let content = truncate(&fact.content.replace('\n', " "), SIMILAR_FACT_TRUNCATE_LEN);
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        format!("{:.0}%", fact.confidence * 100.0),
                        confidence_style(theme, fact.confidence),
                    ),
                    Span::raw("  "),
                    Span::styled(tier_abbr, tier_style(theme, &fact.tier)),
                    Span::raw("  "),
                    Span::styled(content, theme.style_fg()),
                ]));
            }
        }
    } else if app.layout.memory.loading {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Loading entity detail...", theme.style_dim()),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No entity data available.", theme.style_dim()),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" back", theme.style_dim()),
    ]));

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
        format!("{}…", s.get(..max_len.saturating_sub(1)).unwrap_or(s))
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
