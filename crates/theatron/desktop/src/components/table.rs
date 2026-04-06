//! Markdown table rendering component.

use dioxus::prelude::*;
use pulldown_cmark::Alignment;

/// Render a markdown table with column alignment and alternating row backgrounds.
#[component]
pub(crate) fn MdTable(
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    alignments: Vec<Alignment>,
) -> Element {
    rsx! {
        div {
            style: "
                overflow-x: auto;
                margin: var(--space-2) 0;
                border: 1px solid var(--border);
                border-radius: var(--radius-lg);
            ",
            table {
                style: "
                    width: 100%;
                    border-collapse: collapse;
                    font-size: var(--text-sm);
                    font-family: var(--font-body);
                ",
                thead {
                    tr {
                        style: "
                            background: var(--bg-surface-dim);
                            border-bottom: 2px solid var(--border);
                        ",
                        for (i , header) in headers.iter().enumerate() {
                            th {
                                key: "{i}",
                                style: "
                                    padding: var(--space-2) var(--space-3);
                                    text-align: {alignment_css(alignments.get(i).copied())};
                                    color: var(--text-primary);
                                    font-weight: var(--weight-semibold);
                                ",
                                "{header}"
                            }
                        }
                    }
                }
                tbody {
                    for (row_idx , row) in rows.iter().enumerate() {
                        tr {
                            key: "{row_idx}",
                            style: "
                                background: {row_bg(row_idx)};
                                border-bottom: 1px solid var(--border-separator);
                            ",
                            for (col_idx , cell) in row.iter().enumerate() {
                                td {
                                    key: "{col_idx}",
                                    style: "
                                        padding: var(--space-2) var(--space-3);
                                        text-align: {alignment_css(alignments.get(col_idx).copied())};
                                        color: var(--text-secondary);
                                    ",
                                    "{cell}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn alignment_css(align: Option<Alignment>) -> &'static str {
    match align {
        Some(Alignment::Left) | Some(Alignment::None) | None => "left",
        Some(Alignment::Center) => "center",
        Some(Alignment::Right) => "right",
    }
}

fn row_bg(idx: usize) -> &'static str {
    if idx % 2 == 0 {
        "var(--bg-surface)"
    } else {
        "var(--bg)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_css_values() {
        assert_eq!(alignment_css(None), "left");
        assert_eq!(alignment_css(Some(Alignment::Left)), "left");
        assert_eq!(alignment_css(Some(Alignment::None)), "left");
        assert_eq!(alignment_css(Some(Alignment::Center)), "center");
        assert_eq!(alignment_css(Some(Alignment::Right)), "right");
    }

    #[test]
    fn row_bg_alternates() {
        assert_eq!(row_bg(0), "var(--bg-surface)");
        assert_eq!(row_bg(1), "var(--bg)");
        assert_eq!(row_bg(2), "var(--bg-surface)");
        assert_eq!(row_bg(3), "var(--bg)");
    }
}
