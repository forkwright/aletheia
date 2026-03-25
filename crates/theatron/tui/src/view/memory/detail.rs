// Detail rendering for fact and entity views.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::state::memory::MemoryInspectorState;
use crate::theme::Theme;

use super::{SIMILAR_FACT_TRUNCATE_LEN, confidence_style, meta_line, tier_style, truncate};

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
