//! Project-template scaffolder for poiesis report projects.
//!
//! Given a slug, description, format, and confidentiality flag, returns an
//! in-memory set of files suitable for a new report project.

use std::path::PathBuf;

use poiesis_core::Renderer;
use snafu::Snafu;

/// Errors returned by the scaffold pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// The provided slug was empty.
    #[snafu(display("slug must not be empty"))]
    EmptySlug,

    /// Failed to generate the XLSX scaffold.
    #[snafu(display("failed to generate xlsx scaffold: {source}"))]
    XlsxRender {
        /// Underlying renderer error.
        source: poiesis_sheet::xlsx::XlsxRendererError,
    },
}

/// Output format for the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// Typst-only scaffold.
    Typst,
    /// XLSX-only scaffold.
    Xlsx,
    /// Both Typst and XLSX scaffolds.
    Both,
}

/// A single file produced by the scaffolder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScaffoldFile {
    /// Relative path within the project directory.
    pub path: PathBuf,
    /// File contents.
    pub contents: Vec<u8>,
}

impl ScaffoldFile {
    /// Build a [`ScaffoldFile`] from any path and raw byte contents.
    ///
    /// Required to construct the type outside of `poiesis-scaffold`
    /// because the struct is `#[non_exhaustive]` (kanon #108 discipline).
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            contents: contents.into(),
        }
    }

    #[must_use]
    fn from_str(path: impl Into<PathBuf>, contents: &str) -> Self {
        Self::new(path, contents.as_bytes().to_vec())
    }
}

/// Generate a report project scaffold.
///
/// # Errors
///
/// Returns an error if the slug is empty or the XLSX renderer fails.
#[tracing::instrument(level = "debug", skip(slug, description))]
pub fn scaffold_report(
    slug: &str,
    description: &str,
    format: Format,
    confidential: bool,
) -> Result<Vec<ScaffoldFile>, Error> {
    if slug.is_empty() {
        return Err(Error::EmptySlug);
    }

    let mut files: Vec<ScaffoldFile> = Vec::new();

    match format {
        Format::Typst => files.extend(typst_files(slug, description, confidential)),
        Format::Xlsx => files.extend(xlsx_files(slug, description, confidential)?),
        Format::Both => {
            files.extend(typst_files(slug, description, confidential));
            files.extend(xlsx_files(slug, description, confidential)?);
        }
    }

    Ok(files)
}

// ── Typst scaffold ───────────────────────────────────────────────────────────

fn typst_files(slug: &str, description: &str, confidential: bool) -> Vec<ScaffoldFile> {
    let mut report = TYPST_TEMPLATE.replace("{{slug}}", slug);
    report = report.replace("{{description}}", description);

    if confidential {
        report = report.replace("{{confidential_header}}", CONFIDENTIAL_HEADER);
        report = report.replace("{{confidential_footer}}", CONFIDENTIAL_FOOTER);
    } else {
        report = report.replace("{{confidential_header}}\n", "");
        report = report.replace("{{confidential_footer}}\n", "");
    }

    let data = TYPST_DATA_STUB
        .replace("{{slug}}", slug)
        .replace("{{description}}", description);

    vec![
        ScaffoldFile::from_str("report.typ", &report),
        ScaffoldFile::from_str("data.json", &data),
    ]
}

const TYPST_TEMPLATE: &str = include_str!("templates/report.typ");
const TYPST_DATA_STUB: &str = include_str!("templates/data.json");
const CONFIDENTIAL_HEADER: &str = r#"#align(center)[#text(12pt, weight: "bold", fill: red)[CONFIDENTIAL]]
"#;
const CONFIDENTIAL_FOOTER: &str = r"#align(center)[#text(9pt, fill: red)[CONFIDENTIAL — DO NOT DISTRIBUTE]]
";

// ── XLSX scaffold ───────────────────────────────────────────────────────────

fn xlsx_files(
    slug: &str,
    description: &str,
    confidential: bool,
) -> Result<Vec<ScaffoldFile>, Error> {
    use poiesis_core::{Block, Document, Metadata, RichText, Span};

    let mut content: Vec<Block> = Vec::new();

    if confidential {
        content.push(Block::Heading {
            level: 1,
            text: RichText {
                spans: vec![Span::Plain("CONFIDENTIAL".to_owned())],
            },
        });
    }

    content.push(Block::Heading {
        level: 1,
        text: RichText {
            spans: vec![Span::Plain(slug.to_owned())],
        },
    });

    content.push(Block::Paragraph(RichText {
        spans: vec![Span::Plain(description.to_owned())],
    }));

    let doc = Document {
        metadata: Metadata {
            title: slug.to_owned(),
            author: None,
            created: None,
        },
        content,
    };

    let renderer = poiesis_sheet::XlsxRenderer::new();
    let bytes = renderer
        .render(&doc)
        .map_err(|e| Error::XlsxRender { source: e })?;

    Ok(vec![ScaffoldFile {
        path: PathBuf::from("report.xlsx"),
        contents: bytes,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "test assertions on synthetic data"
    )]
    fn scaffold_typst_returns_expected_files() {
        let files = scaffold_report("q1-review", "Quarterly review", Format::Typst, false)
            .expect("scaffold must succeed");
        let paths: Vec<_> = files.iter().map(|f| f.path.to_str().unwrap()).collect();
        assert!(paths.contains(&"report.typ"));
        assert!(paths.contains(&"data.json"));
    }

    #[test]
    #[expect(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "test assertions on synthetic data"
    )]
    fn scaffold_xlsx_returns_expected_files() {
        let files = scaffold_report("q1-review", "Quarterly review", Format::Xlsx, false)
            .expect("scaffold must succeed");
        let paths: Vec<_> = files.iter().map(|f| f.path.to_str().unwrap()).collect();
        assert!(paths.contains(&"report.xlsx"));
    }

    #[test]
    #[expect(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "test assertions on synthetic data"
    )]
    fn scaffold_both_combines_formats() {
        let files = scaffold_report("q1-review", "Quarterly review", Format::Both, false)
            .expect("scaffold must succeed");
        let paths: Vec<_> = files.iter().map(|f| f.path.to_str().unwrap()).collect();
        assert!(paths.contains(&"report.typ"));
        assert!(paths.contains(&"data.json"));
        assert!(paths.contains(&"report.xlsx"));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn scaffold_with_confidential_injects_header() {
        let files = scaffold_report("secret", "Top secret", Format::Typst, true)
            .expect("scaffold must succeed");
        let typst = files
            .iter()
            .find(|f| f.path == std::path::Path::new("report.typ"))
            .expect("report.typ must be present");
        let text = std::str::from_utf8(&typst.contents).expect("report.typ must be valid utf-8");
        assert!(text.contains("CONFIDENTIAL"));
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test assertion")]
    fn scaffold_with_empty_slug_errors() {
        let err = scaffold_report("", "desc", Format::Typst, false).unwrap_err();
        assert!(matches!(err, Error::EmptySlug));
    }
}
