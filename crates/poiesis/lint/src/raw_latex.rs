//! Document portability lint for raw `LaTeX` blocks.

use poiesis_core::{Block, Document};

use crate::{Finding, FindingKind};

/// Export target used by the raw `LaTeX` portability check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportTarget {
    /// A target that can render raw `LaTeX`, such as `.tex` or `LaTeX`-backed PDF.
    Latex,
    /// Any non-`LaTeX` target such as HTML, DOCX, EPUB, or Typst fast-lane PDF.
    NonLatex,
}

/// Check whether raw `LaTeX` content would be non-portable for the export target.
pub fn check_raw_latex_nonportable(doc: &Document, target: ExportTarget) -> Vec<Finding> {
    if matches!(target, ExportTarget::Latex) {
        return Vec::new();
    }

    if has_raw_latex_block(doc) {
        vec![Finding {
            line_start: 1,
            line_end: 1,
            message: "document contains RawBlock(latex), which will not render for non-LaTeX export targets".to_owned(),
            kind: FindingKind::RawLatexNonPortable,
            fix: None,
        }]
    } else {
        Vec::new()
    }
}

fn has_raw_latex_block(doc: &Document) -> bool {
    doc.content.iter().any(|block| {
        matches!(
            block,
            Block::RawBlock { format, .. } if format.eq_ignore_ascii_case("latex")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use poiesis_core::{Block, Document, Metadata, RichText};

    fn sample_doc() -> Document {
        Document {
            metadata: Metadata {
                title: "Raw latex".to_owned(),
                author: None,
                created: None,
            },
            content: vec![
                Block::Paragraph(RichText::from("Hello.")),
                Block::RawBlock {
                    format: "latex".to_owned(),
                    content: "\\alpha".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn non_latex_target_flags_raw_latex() {
        let findings = check_raw_latex_nonportable(&sample_doc(), ExportTarget::NonLatex);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings.first().map(|finding| &finding.kind),
            Some(&FindingKind::RawLatexNonPortable)
        );
    }

    #[test]
    fn latex_target_allows_raw_latex() {
        let findings = check_raw_latex_nonportable(&sample_doc(), ExportTarget::Latex);
        assert!(findings.is_empty());
    }

    #[test]
    fn non_latex_target_ignores_nonlatex_raw_blocks() {
        let doc = Document {
            metadata: Metadata {
                title: "Other raw".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::RawBlock {
                format: "html".to_owned(),
                content: "<span>ok</span>".to_owned(),
            }],
        };

        let findings = check_raw_latex_nonportable(&doc, ExportTarget::NonLatex);
        assert!(findings.is_empty());
    }
}
