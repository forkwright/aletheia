//! view_file tool: images, PDFs, and text with multimodal support.

use std::fs::File;
use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::pin::Pin;

use indexmap::IndexMap;
use koina::base64;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    DocumentSource, ImageSource, InputSchema, PropertyDef, PropertyType, Reversibility,
    RollbackSupport, ToolCapabilityMetadata, ToolCategory, ToolContext, ToolDef, ToolGroupId,
    ToolInput, ToolResult, ToolResultBlock, ToolStability, ToolTag,
};

use super::workspace::{extract_opt_u64, extract_str, validate_prepared_path};

/// WHY: Full filesystem paths in error messages leak instance directory
/// structure to the LLM. Show workspace-relative path instead.
fn relativize_path(path: &Path, workspace: &Path) -> String {
    path.strip_prefix(workspace)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Fallback default; runtime reads `ctx.tool_config.max_image_bytes`.
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// Fallback default; runtime reads `ctx.tool_config.max_pdf_bytes`.
pub const MAX_PDF_BYTES: u64 = 32 * 1024 * 1024;

enum MediaKind {
    Image(&'static str),
    Pdf,
    Text,
}

fn detect_media_kind(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some(MediaKind::Image("image/png")),
        "jpg" | "jpeg" => Some(MediaKind::Image("image/jpeg")),
        "gif" => Some(MediaKind::Image("image/gif")),
        "webp" => Some(MediaKind::Image("image/webp")),
        "pdf" => Some(MediaKind::Pdf),
        "svg" | "txt" | "md" | "rs" | "py" | "ts" | "js" | "toml" | "yaml" | "yml" | "json"
        | "css" | "html" | "sh" | "bash" | "fish" | "sql" | "go" | "java" | "c" | "cpp" | "h"
        | "hpp" | "rb" | "lua" | "conf" | "cfg" | "ini" | "env" | "log" | "csv" | "xml" | "jsx"
        | "tsx" | "vue" | "svelte" | "lock" | "makefile" | "dockerfile" => Some(MediaKind::Text),
        _ => None,
    }
}

/// Read through one opened handle. A metadata check followed by `fs::read`
/// lets a concurrent replacement turn an approved small PDF into an unbounded
/// allocation; this helper observes and caps the same handle instead.
fn read_file_bounded(file: &mut File, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds configured byte limit",
        ));
    }
    let max = usize::try_from(max_bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "configured byte limit is unsupported",
        )
    })?;
    let mut bytes = Vec::with_capacity(max.min(64 * 1024));
    let mut chunk = [0_u8; 8192];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            return Ok(bytes);
        }
        let next = bytes.len().checked_add(count).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "file exceeds configured byte limit",
            )
        })?;
        if next > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "file exceeds configured byte limit",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

/// Open the target exactly once, relative to a pinned allowed-root handle.
///
/// Linux `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` makes containment
/// and opening one kernel operation. A concurrent replacement can therefore
/// neither redirect an intermediate component outside the root nor swap in a
/// symlink between authorization and open. Other platforms fail closed until
/// they have an equivalent handle-relative primitive.
#[cfg(target_os = "linux")]
fn open_validated_file(path: &Path, ctx: &ToolContext) -> std::io::Result<File> {
    use rustix::fs::{Mode, OFlags, ResolveFlags};

    let mut saw_candidate_root = false;
    for root in &ctx.allowed_roots {
        let Ok(root) = std::fs::canonicalize(root) else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(&root) else {
            continue;
        };
        saw_candidate_root = true;
        let root_fd = match rustix::fs::open(
            &root,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(_) => continue,
        };
        let relative = if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        };
        let fd = rustix::fs::openat2(
            &root_fd,
            relative,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS | ResolveFlags::NO_SYMLINKS,
        )?;
        return Ok(fd.into());
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        if saw_candidate_root {
            "allowed root could not be pinned for file access"
        } else {
            "opened file is outside allowed roots"
        },
    ))
}

#[cfg(not(target_os = "linux"))]
fn open_validated_file(_path: &Path, _ctx: &ToolContext) -> std::io::Result<File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "view_file requires handle-native path validation on this platform",
    ))
}

struct ViewFileExecutor;

impl ToolExecutor for ViewFileExecutor {
    fn path_arguments(&self) -> &'static [&'static str] {
        &["path"]
    }

    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let max_lines = extract_opt_u64(&input.arguments, "maxLines");
            let path = validate_prepared_path(path_str, ctx, &input.name)?;
            let Some(kind) = detect_media_kind(&path) else {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("unknown");
                return Ok(ToolResult::error(format!(
                    "unsupported file type: {ext}. Supported: png, jpg, gif, webp, pdf, and text files"
                )));
            };

            let mut file = match open_validated_file(&path, ctx) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(ToolResult::error(format!(
                        "file not found: {}",
                        relativize_path(&path, &ctx.workspace)
                    )));
                }
                Err(e) => {
                    return Ok(ToolResult::error(format!("file access refused: {e}")));
                }
            };
            let metadata = match file.metadata() {
                Ok(metadata) => metadata,
                Err(error) => return Ok(ToolResult::error(format!("metadata failed: {error}"))),
            };

            if !metadata.is_file() {
                return Ok(ToolResult::error(format!(
                    "not a file: {}",
                    relativize_path(&path, &ctx.workspace)
                )));
            }

            Ok(execute_by_kind(
                &kind,
                &mut file,
                &path,
                &metadata,
                max_lines,
                &ctx.workspace,
                &ctx.tool_config,
            ))
        })
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "single cohesive function handling three media kinds; splitting would fragment the config plumbing"
)]
fn execute_by_kind(
    kind: &MediaKind,
    file: &mut File,
    path: &std::path::Path,
    metadata: &std::fs::Metadata,
    max_lines: Option<u64>,
    workspace: &std::path::Path,
    tool_config: &taxis::config::ToolLimitsConfig,
) -> ToolResult {
    match kind {
        MediaKind::Image(media_type) => {
            let max_image = tool_config.max_image_bytes;
            if metadata.len() > max_image {
                return ToolResult::error(format!(
                    "image too large: {} bytes (max {} MB)",
                    metadata.len(),
                    max_image / (1024 * 1024)
                ));
            }
            let bytes = match read_file_bounded(file, max_image) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    return ToolResult::error(format!(
                        "image too large: exceeds max {} MB",
                        max_image / (1024 * 1024)
                    ));
                }
                Err(e) => return ToolResult::error(format!("read failed: {e}")),
            };
            let encoded = base64::encode(&bytes);
            ToolResult::blocks(vec![
                ToolResultBlock::Image {
                    source: ImageSource {
                        source_type: "base64".to_owned(),
                        media_type: (*media_type).to_owned(),
                        data: encoded,
                    },
                },
                ToolResultBlock::Text {
                    text: format!(
                        "{} ({} bytes)",
                        relativize_path(path, workspace),
                        bytes.len()
                    ),
                },
            ])
        }
        MediaKind::Pdf => {
            let max_pdf = tool_config.max_pdf_bytes;
            let bytes = match read_file_bounded(file, max_pdf) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    return ToolResult::error(format!(
                        "PDF too large: exceeds max {} MB",
                        max_pdf / (1024 * 1024)
                    ));
                }
                Err(e) => return ToolResult::error(format!("read failed: {e}")),
            };
            let encoded = base64::encode(&bytes);
            ToolResult::blocks(vec![
                ToolResultBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_owned(),
                        media_type: "application/pdf".to_owned(),
                        data: encoded,
                    },
                },
                ToolResultBlock::Text {
                    text: format!(
                        "{} ({} bytes)",
                        relativize_path(path, workspace),
                        bytes.len()
                    ),
                },
            ])
        }
        MediaKind::Text => {
            let mut content = String::new();
            if let Err(e) = file.read_to_string(&mut content) {
                return if e.kind() == std::io::ErrorKind::InvalidData {
                    ToolResult::error(format!("file is not valid UTF-8 text: {}", path.display()))
                } else {
                    ToolResult::error(format!("read failed: {e}"))
                };
            }
            let output = match max_lines {
                Some(n) => {
                    let n = usize::try_from(n).unwrap_or(usize::MAX);
                    content
                        .lines()
                        .take(n)
                        .fold(String::new(), |mut acc, line| {
                            if !acc.is_empty() {
                                acc.push('\n');
                            }
                            acc.push_str(line);
                            acc
                        })
                }
                None => content,
            };
            ToolResult::text(output)
        }
    }
}

/// Register the `view_file` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(view_file_def(), Box::new(ViewFileExecutor))?;
    registry.declare_capability(
        koina::id::ToolName::from_static("view_file"), // kanon:ignore RUST/expect
        ToolCapabilityMetadata {
            owner: "organon::builtins::view_file".to_owned(),
            stability: ToolStability::Stable,
            rollback: RollbackSupport::Supported,
            ..ToolCapabilityMetadata::default()
        },
    )?;
    Ok(())
}

fn view_file_def() -> crate::types::ToolDef {
    use koina::id::ToolName;
    ToolDef {
        name: ToolName::from_static("view_file"), // kanon:ignore RUST/expect
        description: "View a file — images, PDFs, and text. For images and PDFs, the content is sent directly to the model for visual analysis.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "maxLines".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "For text files: maximum lines to return".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: index 0 is valid after asserting len >= 1"
)]
mod tests {
    use koina::id::ToolName;

    use super::*;
    use crate::types::ToolResultContent;

    fn mock_ctx(dir: &Path) -> ToolContext {
        crate::testing::make_test_context_at(dir)
    }

    fn tool_input(args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: ToolName::from_static("view_file"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn view_text_file() {
        let dir = tempfile::tempdir().expect("tmpdir");
        #[expect(
            clippy::disallowed_methods,
            reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
        )]
        std::fs::write(dir.path().join("hello.txt"), "hello world").expect("write");
        let ctx = mock_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "hello.txt" }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert!(!result.is_error, "expected result.is_error to be false");
        assert_eq!(
            result.content.text_summary(),
            "hello world",
            "expected result.content.text_summary() to equal \"hello world\""
        );
    }

    #[tokio::test]
    async fn view_text_file_max_lines() {
        let dir = tempfile::tempdir().expect("tmpdir");
        #[expect(
            clippy::disallowed_methods,
            reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
        )]
        std::fs::write(dir.path().join("lines.txt"), "a\nb\nc\nd\ne").expect("write");
        let ctx = mock_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "lines.txt", "maxLines": 2 }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert_eq!(
            result.content.text_summary(),
            "a\nb",
            "expected result.content.text_summary() to equal \"a\nb\""
        );
    }

    #[tokio::test]
    async fn view_png_returns_image_block() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let png_bytes: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
            0x77, 0x53, 0xDE, // IHDR data + CRC
            0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, // IDAT chunk
            0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21,
            0xBC, 0x33, // IDAT data + CRC
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND chunk
            0xAE, 0x42, 0x60, 0x82, // IEND CRC
        ];
        #[expect(
            clippy::disallowed_methods,
            reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
        )]
        std::fs::write(dir.path().join("test.png"), &png_bytes).expect("write");
        let ctx = mock_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "test.png" }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert!(!result.is_error, "expected result.is_error to be false");
        match &result.content {
            ToolResultContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2, "expected blocks.len() to equal 2");
                match &blocks[0] {
                    ToolResultBlock::Image { source } => {
                        assert_eq!(
                            source.media_type, "image/png",
                            "expected source.media_type to equal \"image/png\""
                        );
                        assert_eq!(
                            source.source_type, "base64",
                            "expected source.source_type to equal \"base64\""
                        );
                        assert!(
                            !source.data.is_empty(),
                            "expected source.data.is_empty() to be false"
                        );
                    }
                    other => panic!("expected Image block, got {other:?}"),
                }
            }
            other => panic!("expected Blocks, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn view_unknown_extension_errors() {
        let dir = tempfile::tempdir().expect("tmpdir");
        #[expect(
            clippy::disallowed_methods,
            reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
        )]
        std::fs::write(dir.path().join("data.bin"), b"\x00\x01\x02").expect("write");
        let ctx = mock_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "data.bin" }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert!(result.is_error, "expected result.is_error to be true");
        assert!(
            result
                .content
                .text_summary()
                .contains("unsupported file type")
        );
    }

    #[tokio::test]
    async fn view_missing_file_errors() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let ctx = mock_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "nope.txt" }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert!(result.is_error, "expected result.is_error to be true");
        assert!(
            result.content.text_summary().contains("file not found"),
            "expected result.content.text_summary().contains(\"file not found\") to be true"
        );
    }

    #[tokio::test]
    async fn view_path_traversal_blocked() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let ctx = mock_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "../../etc/passwd" }));
        let err = ViewFileExecutor
            .execute(&input, &ctx)
            .await
            .expect_err("should reject traversal");
        assert!(
            err.to_string().contains("outside allowed roots"),
            "expected err.to_string().contains(\"outside allowed roots\") to be true"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "hermetic race regression controls temporary filesystem identities directly"
    )]
    fn handle_relative_open_refuses_symlink_replacement_and_retains_open_file() {
        use std::os::unix::fs::symlink;

        let allowed = tempfile::tempdir().expect("allowed tmpdir");
        let outside = tempfile::tempdir().expect("outside tmpdir");
        let directory = allowed.path().join("nested");
        std::fs::create_dir(&directory).expect("create allowed directory");
        let path = directory.join("race.pdf");
        let outside_path = outside.path().join("race.pdf");
        std::fs::write(&path, b"inside").expect("write allowed file");
        std::fs::write(&outside_path, b"outside").expect("write outside file");
        let ctx = mock_ctx(allowed.path());
        let tool_name = ToolName::from_static("view_file");
        let prepared =
            validate_prepared_path("nested/race.pdf", &ctx, &tool_name).expect("prepare path");

        let mut opened = open_validated_file(&prepared, &ctx).expect("open approved object");
        std::fs::rename(&directory, allowed.path().join("nested-original"))
            .expect("move approved directory");
        symlink(outside.path(), &directory).expect("install outside directory replacement");
        let bytes = read_file_bounded(&mut opened, 64).expect("read original handle");
        assert_eq!(
            bytes, b"inside",
            "reads must stay bound to the approved handle"
        );

        let _error = open_validated_file(&prepared, &ctx)
            .expect_err("an outside replacement opened during the race must be refused");
    }

    #[tokio::test]
    async fn view_file_registered() {
        let mut reg = crate::registry::ToolRegistry::new();
        register(&mut reg).expect("register");
        let name = ToolName::from_static("view_file");
        assert!(
            reg.get_def(&name).is_some(),
            "expected reg.get_def(&name).is_some() to be true"
        );
    }
}
