//! view_file tool — images, PDFs, and text with multimodal support.
#![expect(clippy::expect_used, reason = "ToolName::new() with static string literals is infallible — name validation would only fail on invalid chars which these names don't contain")]

use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use base64::Engine as _;
use indexmap::IndexMap;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    DocumentSource, ImageSource, InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext,
    ToolDef, ToolInput, ToolResult, ToolResultBlock,
};

use super::workspace::{extract_opt_u64, extract_str, validate_path};

const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 32 * 1024 * 1024;

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

struct ViewFileExecutor;

impl ToolExecutor for ViewFileExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let max_lines = extract_opt_u64(&input.arguments, "maxLines");
            let path = validate_path(path_str, ctx, &input.name)?;

            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(ToolResult::error(format!(
                        "file not found: {}",
                        path.display()
                    )));
                }
                Err(e) => {
                    return Ok(ToolResult::error(format!("metadata failed: {e}")));
                }
            };

            if !metadata.is_file() {
                return Ok(ToolResult::error(format!("not a file: {}", path.display())));
            }

            let Some(kind) = detect_media_kind(&path) else {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("unknown");
                return Ok(ToolResult::error(format!(
                    "unsupported file type: {ext}. Supported: png, jpg, gif, webp, pdf, and text files"
                )));
            };

            Ok(execute_by_kind(&kind, &path, &metadata, max_lines))
        })
    }
}

fn execute_by_kind(
    kind: &MediaKind,
    path: &std::path::Path,
    metadata: &std::fs::Metadata,
    max_lines: Option<u64>,
) -> ToolResult {
    match kind {
        MediaKind::Image(media_type) => {
            if metadata.len() > MAX_IMAGE_BYTES {
                return ToolResult::error(format!(
                    "image too large: {} bytes (max {} MB)",
                    metadata.len(),
                    MAX_IMAGE_BYTES / (1024 * 1024)
                ));
            }
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => return ToolResult::error(format!("read failed: {e}")),
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            ToolResult::blocks(vec![
                ToolResultBlock::Image {
                    source: ImageSource {
                        source_type: "base64".to_owned(),
                        media_type: (*media_type).to_owned(),
                        data: encoded,
                    },
                },
                ToolResultBlock::Text {
                    text: format!("{} ({} bytes)", path.display(), bytes.len()),
                },
            ])
        }
        MediaKind::Pdf => {
            if metadata.len() > MAX_PDF_BYTES {
                return ToolResult::error(format!(
                    "PDF too large: {} bytes (max {} MB)",
                    metadata.len(),
                    MAX_PDF_BYTES / (1024 * 1024)
                ));
            }
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => return ToolResult::error(format!("read failed: {e}")),
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            ToolResult::blocks(vec![
                ToolResultBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_owned(),
                        media_type: "application/pdf".to_owned(),
                        data: encoded,
                    },
                },
                ToolResultBlock::Text {
                    text: format!("{} ({} bytes)", path.display(), bytes.len()),
                },
            ])
        }
        MediaKind::Text => {
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    return ToolResult::error(format!(
                        "file is not valid UTF-8 text: {}",
                        path.display()
                    ));
                }
                Err(e) => return ToolResult::error(format!("read failed: {e}")),
            };
            let output = match max_lines {
                Some(n) => {
                    let n = usize::try_from(n).unwrap_or(usize::MAX);
                    content.lines().take(n).collect::<Vec<_>>().join("\n")
                }
                None => content,
            };
            ToolResult::text(output)
        }
    }
}

/// Register the `view_file` tool.
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(view_file_def(), Box::new(ViewFileExecutor))?;
    Ok(())
}

fn view_file_def() -> crate::types::ToolDef {
    use aletheia_koina::id::ToolName;
    ToolDef {
        name: ToolName::new("view_file").expect("valid tool name"),
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
                    },
                ),
                (
                    "maxLines".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "For text files: maximum lines to return".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use super::*;
    use crate::types::ToolResultContent;

    fn test_ctx(dir: &Path) -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: dir.to_path_buf(),
            allowed_roots: vec![dir.to_path_buf()],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn tool_input(args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: ToolName::new("view_file").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn view_text_file() {
        let dir = tempfile::tempdir().expect("tmpdir");
        std::fs::write(dir.path().join("hello.txt"), "hello world").expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "hello.txt" }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert!(!result.is_error);
        assert_eq!(result.content.text_summary(), "hello world");
    }

    #[tokio::test]
    async fn view_text_file_max_lines() {
        let dir = tempfile::tempdir().expect("tmpdir");
        std::fs::write(dir.path().join("lines.txt"), "a\nb\nc\nd\ne").expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "lines.txt", "maxLines": 2 }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert_eq!(result.content.text_summary(), "a\nb");
    }

    #[tokio::test]
    async fn view_png_returns_image_block() {
        let dir = tempfile::tempdir().expect("tmpdir");
        // Minimal valid 1x1 white PNG
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
        std::fs::write(dir.path().join("test.png"), &png_bytes).expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "test.png" }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert!(!result.is_error);
        match &result.content {
            ToolResultContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2);
                match &blocks[0] {
                    ToolResultBlock::Image { source } => {
                        assert_eq!(source.media_type, "image/png");
                        assert_eq!(source.source_type, "base64");
                        assert!(!source.data.is_empty());
                    }
                    other => panic!("expected Image block, got {other:?}"),
                }
            }
            other @ ToolResultContent::Text(_) => panic!("expected Blocks, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn view_unknown_extension_errors() {
        let dir = tempfile::tempdir().expect("tmpdir");
        std::fs::write(dir.path().join("data.bin"), b"\x00\x01\x02").expect("write");
        let ctx = test_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "data.bin" }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert!(result.is_error);
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
        let ctx = test_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "nope.txt" }));
        let result = ViewFileExecutor.execute(&input, &ctx).await.expect("exec");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("file not found"));
    }

    #[tokio::test]
    async fn view_path_traversal_blocked() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let ctx = test_ctx(dir.path());
        let input = tool_input(serde_json::json!({ "path": "../../etc/passwd" }));
        let err = ViewFileExecutor
            .execute(&input, &ctx)
            .await
            .expect_err("should reject traversal");
        assert!(err.to_string().contains("outside allowed roots"));
    }

    #[tokio::test]
    async fn view_file_registered() {
        let mut reg = crate::registry::ToolRegistry::new();
        register(&mut reg).expect("register");
        let name = ToolName::new("view_file").expect("valid");
        assert!(reg.get_def(&name).is_some());
    }
}
