//! Workspace file-browser handlers.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mime_guess::MimeGuess;
use std::path::{Component, Path, PathBuf};
use tokio::process::Command;

use crate::error::{ApiError, BadRequestSnafu, InternalSnafu, NotFoundSnafu, UserFacingSnafu};
use crate::extract::Claims;
use crate::state::WorkspaceState;

#[path = "workspace_dto.rs"]
mod workspace_dto;
pub use workspace_dto::{
    ContentQuery, DiffQuery, FileEntry, FilesQuery, GitStatusEntry, OpenRequest, OpenResponse,
    SearchQuery, SearchResult, WriteContentRequest, WriteContentResponse,
};

const CONTENT_LIMIT_BYTES: u64 = 2 * 1024 * 1024;
const SEARCH_LIMIT_CAP: usize = 1000;

/// Maximum byte length accepted by the workspace write endpoint.
///
/// WHY: the workspace editor is designed for text notes and small structured
/// files. A generous-but-bounded cap lets operators edit large notes without
/// hitting the limit, while constraining a single request from becoming a
/// memory, disk, or abuse vector through binary drops, scripts, or bulk data.
const WRITE_LIMIT_BYTES: usize = 10 * 1024 * 1024;

/// File extensions the workspace write endpoint is allowed to create or
/// overwrite.
///
/// WHY: the editor surface is a markdown/plain-text vault editor. Restricting
/// writes to known text formats stops the endpoint from being used to drop
/// binaries, scripts, or dotfiles into the vault.
const WRITABLE_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdx", "txt", "text", "rst", "org", "csv", "tsv", "json", "toml", "yaml",
    "yml",
];

/// GET /api/v1/workspace/files
#[utoipa::path(
    get,
    path = "/api/v1/workspace/files",
    params(
        ("path" = Option<String>, Query, description = "Directory path relative to the workspace root"),
    ),
    responses(
        (status = 200, description = "Workspace directory listing", body = [FileEntry]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 400, description = "Invalid workspace path", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_files(
    State(state): State<WorkspaceState>,
    Query(query): Query<FilesQuery>,
) -> Result<Json<Vec<FileEntry>>, ApiError> {
    let dir = resolve_workspace_directory(&state.workspace_root, query.path.as_deref())?;
    let mut entries = Vec::new();

    let read_dir = std::fs::read_dir(&dir).map_err(|e| {
        InternalSnafu {
            message: format!("failed to read workspace directory {}: {e}", dir.display()),
        }
        .build()
    })?;

    for entry in read_dir {
        let entry = entry.map_err(|e| {
            InternalSnafu {
                message: format!("failed to read workspace entry in {}: {e}", dir.display()),
            }
            .build()
        })?;
        let file_type = entry.file_type().map_err(|e| {
            InternalSnafu {
                message: format!(
                    "failed to inspect workspace entry type {}: {e}",
                    entry.path().display()
                ),
            }
            .build()
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = relative_workspace_path(&state.workspace_root, &entry.path())?;
        let metadata = entry.metadata().map_err(|e| {
            InternalSnafu {
                message: format!(
                    "failed to read workspace entry metadata {}: {e}",
                    entry.path().display()
                ),
            }
            .build()
        })?;

        entries.push(FileEntry {
            name,
            path,
            is_dir: file_type.is_dir(),
            size: if file_type.is_dir() {
                0
            } else {
                metadata.len()
            },
        });
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(Json(entries))
}

/// GET /api/v1/workspace/git-status
#[utoipa::path(
    get,
    path = "/api/v1/workspace/git-status",
    responses(
        (status = 200, description = "Normalized git status entries", body = [GitStatusEntry]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn git_status(
    State(state): State<WorkspaceState>,
) -> Result<Json<Vec<GitStatusEntry>>, ApiError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(&state.workspace_root)
        .arg("status")
        .arg("--porcelain")
        .arg("--untracked-files=all")
        .output()
        .await
        .map_err(|e| {
            InternalSnafu {
                message: format!(
                    "failed to run git status in {}: {e}",
                    state.workspace_root.display()
                ),
            }
            .build()
        })?;

    if !output.status.success() {
        if is_not_a_git_repo(&output.stderr) {
            return Ok(Json(Vec::new()));
        }
        return Err(InternalSnafu {
            message: format!(
                "git status failed in {}: {}",
                state.workspace_root.display(),
                String::from_utf8_lossy(&output.stderr)
            ),
        }
        .build());
    }

    let mut entries = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(entry) = parse_git_status_line(line) {
            entries.push(entry);
        }
    }

    Ok(Json(entries))
}

/// GET /api/v1/workspace/files/content
#[expect(
    clippy::disallowed_methods,
    reason = "workspace content endpoints intentionally use synchronous filesystem reads for bounded local file access"
)]
#[utoipa::path(
    get,
    path = "/api/v1/workspace/files/content",
    params(
        ("path" = String, Query, description = "Workspace-relative file path"),
    ),
    responses(
        (status = 200, description = "Raw file content", content_type = "text/plain", body = String),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 400, description = "Invalid workspace path", body = crate::error::ErrorResponse),
        (status = 404, description = "Workspace file not found", body = crate::error::ErrorResponse),
        (status = 413, description = "File too large", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn file_content(
    State(state): State<WorkspaceState>,
    Query(query): Query<ContentQuery>,
) -> Result<Response, ApiError> {
    let path = resolve_workspace_file(&state.workspace_root, &query.path)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|_err| {
        NotFoundSnafu {
            path: query.path.clone(),
        }
        .build()
    })?;

    if metadata.is_dir() {
        return Err(BadRequestSnafu {
            message: format!("path is a directory, not a file: {}", query.path),
        }
        .build());
    }

    if metadata.len() > CONTENT_LIMIT_BYTES {
        return Err(UserFacingSnafu {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: "payload_too_large".to_owned(),
            message: format!(
                "file exceeds the {CONTENT_LIMIT_BYTES} byte response limit: {}",
                query.path
            ),
            retry_after_secs: None,
        }
        .build());
    }

    let bytes = std::fs::read(&path).map_err(|e| {
        InternalSnafu {
            message: format!("failed to read workspace file {}: {e}", path.display()),
        }
        .build()
    })?;
    let content_type = MimeGuess::from_path(&path)
        .first_or_text_plain()
        .to_string();

    Ok(([(header::CONTENT_TYPE, content_type)], bytes).into_response())
}

/// PUT /api/v1/workspace/files/content
///
/// Writes UTF-8 text content back to a file in the workspace vault.
///
/// SAFETY: the write target is the operator's live, restic-backed theke vault.
/// The write is atomic — content lands in a sibling temp file that is fsynced
/// and then renamed over the destination — so a crash or partial write never
/// leaves the vault with a truncated note. The path is validated through the
/// same escape guard as every read endpoint, the extension must be a known
/// text format, the body is size-capped, and an optional `if_match_mtime_ms`
/// guard rejects writes that would clobber a concurrent edit.
///
/// # Cancel safety
///
/// Cancel-safe. If the future is dropped after the temp file is written but
/// before the rename, the temp file is removed on the next failure path or
/// left as inert orphaned content under a hidden name; the destination is
/// never observed in a half-written state.
#[utoipa::path(
    put,
    path = "/api/v1/workspace/files/content",
    request_body = WriteContentRequest,
    responses(
        (status = 200, description = "File written", body = WriteContentResponse),
        (status = 400, description = "Invalid workspace path or extension", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 409, description = "Concurrent modification conflict", body = crate::error::ErrorResponse),
        (status = 413, description = "Content too large", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn write_file_content(
    State(state): State<WorkspaceState>,
    _claims: Claims,
    Json(request): Json<WriteContentRequest>,
) -> Result<Json<WriteContentResponse>, ApiError> {
    if request.content.len() > WRITE_LIMIT_BYTES {
        return Err(UserFacingSnafu {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: "payload_too_large".to_owned(),
            message: format!(
                "content exceeds the {WRITE_LIMIT_BYTES} byte write limit: {}",
                request.path
            ),
            retry_after_secs: None,
        }
        .build());
    }

    let target = resolve_workspace_write_target(&state.workspace_root, &request.path)?;

    // WHY: inspect the existing target before the extension gate so a directory
    // or symlink target is rejected for what it *is*, not incidentally for its
    // (missing) extension. A symlink target could redirect the write outside
    // the guarded tree even though the link itself lives under the root.
    if let Ok(existing) = std::fs::symlink_metadata(&target) {
        if existing.is_dir() {
            return Err(BadRequestSnafu {
                message: format!("path is a directory, not a file: {}", request.path),
            }
            .build());
        }
        if existing.file_type().is_symlink() {
            return Err(BadRequestSnafu {
                message: format!("refusing to write through a symlink: {}", request.path),
            }
            .build());
        }
        if let Some(expected) = request.if_match_mtime_ms {
            let current = file_mtime_ms(&existing)?;
            if current != expected {
                return Err(ApiError::Conflict {
                    message: format!(
                        "file changed since last read (expected mtime {expected}, found {current}): {}",
                        request.path
                    ),
                    location: snafu::location!(),
                });
            }
        }
    } else if request.if_match_mtime_ms.is_some() {
        // WHY: caller expected an existing file to match an mtime, but the file
        // is gone — surface that as a conflict rather than silently creating it.
        return Err(ApiError::Conflict {
            message: format!(
                "file no longer exists but an mtime match was required: {}",
                request.path
            ),
            location: snafu::location!(),
        });
    }

    require_writable_extension(&target, &request.path)?;

    let metadata = atomic_write(&target, request.content.as_bytes())?;

    Ok(Json(WriteContentResponse {
        path: relative_workspace_path(&state.workspace_root, &target)?,
        size: metadata.len(),
        mtime_ms: file_mtime_ms(&metadata)?,
    }))
}

/// POST /api/v1/workspace/open
///
/// Opens a workspace file in the system default application via `xdg-open`
/// (through the `opener` crate). Used for non-markdown files (PDFs, images,
/// canvas, etc.) the in-app viewer does not render.
///
/// SAFETY: the path is resolved through the same escape guard as every read
/// endpoint, so only files that genuinely live inside the workspace root can
/// be opened. The client only ever knows workspace-relative paths.
#[utoipa::path(
    post,
    path = "/api/v1/workspace/open",
    request_body = OpenRequest,
    responses(
        (status = 200, description = "Open dispatched", body = OpenResponse),
        (status = 400, description = "Invalid workspace path", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Workspace file not found", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn open_file(
    State(state): State<WorkspaceState>,
    _claims: Claims,
    Json(request): Json<OpenRequest>,
) -> Result<Json<OpenResponse>, ApiError> {
    let resolved = resolve_workspace_file(&state.workspace_root, &request.path)?;
    let metadata = std::fs::symlink_metadata(&resolved).map_err(|_err| {
        NotFoundSnafu {
            path: request.path.clone(),
        }
        .build()
    })?;
    if metadata.is_dir() {
        return Err(BadRequestSnafu {
            message: format!("path is a directory, not a file: {}", request.path),
        }
        .build());
    }

    let open_path = resolved.clone();
    let relative = relative_workspace_path(&state.workspace_root, &resolved)?;

    // WHY: `opener::open` spawns a blocking subprocess (xdg-open); run it off
    // the async reactor so the handler does not block the executor.
    tokio::task::spawn_blocking(move || opener::open(&open_path))
        .await
        .map_err(|e| {
            InternalSnafu {
                message: format!("open task panicked: {e}"),
            }
            .build()
        })?
        .map_err(|e| {
            InternalSnafu {
                message: format!("failed to open workspace file: {e}"),
            }
            .build()
        })?;

    Ok(Json(OpenResponse {
        ok: true,
        path: relative,
    }))
}

/// GET /api/v1/workspace/diff
#[utoipa::path(
    get,
    path = "/api/v1/workspace/diff",
    params(
        ("path" = String, Query, description = "Workspace-relative path passed to `git diff -- <path>`"),
    ),
    responses(
        (status = 200, description = "Unified diff text", content_type = "text/plain", body = String),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 400, description = "Invalid workspace path", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn file_diff(
    State(state): State<WorkspaceState>,
    Query(query): Query<DiffQuery>,
) -> Result<Response, ApiError> {
    let relative = normalize_relative_path(&query.path)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&state.workspace_root)
        .arg("diff")
        .arg("--")
        .arg(relative.as_os_str())
        .output()
        .await
        .map_err(|e| {
            InternalSnafu {
                message: format!(
                    "failed to run git diff in {}: {e}",
                    state.workspace_root.display()
                ),
            }
            .build()
        })?;

    if !output.status.success() {
        if is_not_a_git_repo(&output.stderr) {
            return Ok(([(header::CONTENT_TYPE, "text/plain")], String::new()).into_response());
        }
        return Err(InternalSnafu {
            message: format!(
                "git diff failed in {}: {}",
                state.workspace_root.display(),
                String::from_utf8_lossy(&output.stderr)
            ),
        }
        .build());
    }

    let diff = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(([(header::CONTENT_TYPE, "text/plain")], diff).into_response())
}

/// GET /api/v1/workspace/search
#[utoipa::path(
    get,
    path = "/api/v1/workspace/search",
    params(
        ("q" = String, Query, description = "Case-insensitive filename/content query"),
        ("limit" = Option<usize>, Query, description = "Maximum results (default: 50)"),
    ),
    responses(
        (status = 200, description = "Workspace search results", body = [SearchResult]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 400, description = "Invalid workspace path", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn search(
    State(state): State<WorkspaceState>,
    Query(mut query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, ApiError> {
    if query.q.trim().is_empty() {
        return Err(BadRequestSnafu {
            message: "search query 'q' must not be empty".to_owned(),
        }
        .build());
    }

    query.limit = query.limit.min(SEARCH_LIMIT_CAP);
    let query_lower = query.q.to_lowercase();
    let mut results = Vec::new();
    search_workspace(
        &state.workspace_root,
        &state.workspace_root,
        &query_lower,
        query.limit,
        &mut results,
    )?;
    Ok(Json(results))
}

fn resolve_workspace_directory(root: &Path, path: Option<&str>) -> Result<PathBuf, ApiError> {
    match path.map(str::trim) {
        None | Some("" | ".") => Ok(root.to_path_buf()),
        Some(path) => {
            let resolved = resolve_workspace_file(root, path)?;
            let metadata = std::fs::metadata(&resolved).map_err(|_err| {
                NotFoundSnafu {
                    path: path.to_owned(),
                }
                .build()
            })?;
            if !metadata.is_dir() {
                return Err(BadRequestSnafu {
                    message: format!("path is not a directory: {path}"),
                }
                .build());
            }
            Ok(resolved)
        }
    }
}

fn resolve_workspace_file(root: &Path, path: &str) -> Result<PathBuf, ApiError> {
    let relative = normalize_relative_path(path)?;
    let joined = root.join(&relative);
    let canonical = std::fs::canonicalize(&joined).map_err(|_err| {
        NotFoundSnafu {
            path: path.to_owned(),
        }
        .build()
    })?;
    if !canonical.starts_with(root) {
        return Err(BadRequestSnafu {
            message: format!("path escapes the workspace root: {path}"),
        }
        .build());
    }
    Ok(canonical)
}

/// Resolve a workspace-relative path to an absolute write destination.
///
/// Unlike [`resolve_workspace_file`], the destination file need not already
/// exist (the editor must be able to create new notes). The escape guard is
/// preserved by canonicalizing the **parent** directory — which must exist and
/// resolves any symlinks in the directory chain — and confirming it stays
/// under the workspace root before re-joining the final filename.
fn resolve_workspace_write_target(root: &Path, path: &str) -> Result<PathBuf, ApiError> {
    let relative = normalize_relative_path(path)?;

    let file_name = relative.file_name().ok_or_else(|| {
        BadRequestSnafu {
            message: format!("workspace path must name a file: {path}"),
        }
        .build()
    })?;

    let joined = root.join(&relative);
    let parent = joined.parent().unwrap_or(root);

    let canonical_parent = std::fs::canonicalize(parent).map_err(|_err| {
        NotFoundSnafu {
            path: path.to_owned(),
        }
        .build()
    })?;
    if !canonical_parent.starts_with(root) {
        return Err(BadRequestSnafu {
            message: format!("path escapes the workspace root: {path}"),
        }
        .build());
    }

    Ok(canonical_parent.join(file_name))
}

/// Reject write paths whose extension is not a known text format.
fn require_writable_extension(absolute: &Path, original: &str) -> Result<(), ApiError> {
    let allowed = absolute
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|ext| WRITABLE_EXTENSIONS.contains(&ext.as_str()));

    if allowed {
        Ok(())
    } else {
        Err(BadRequestSnafu {
            message: format!(
                "workspace writes are limited to text files ({}): {original}",
                WRITABLE_EXTENSIONS.join(", ")
            ),
        }
        .build())
    }
}

/// Atomically write `bytes` to `dest` via a sibling temp file + fsync + rename.
///
/// INVARIANT: `dest` is never observed in a partially-written state. On any
/// failure the temp file is removed so no orphan or partial artifact is left
/// in the vault.
#[expect(
    clippy::disallowed_methods,
    reason = "atomic vault write needs a synchronous open(parent) to fsync the directory entry after rename; tokio::fs offers no parent-dir sync and the cost is one bounded local syscall"
)]
fn atomic_write(dest: &Path, bytes: &[u8]) -> Result<std::fs::Metadata, ApiError> {
    use std::io::Write as _;

    let parent = dest.parent().ok_or_else(|| {
        InternalSnafu {
            message: format!("write target has no parent directory: {}", dest.display()),
        }
        .build()
    })?;

    let file_name = dest.file_name().map_or_else(
        || "file".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let unique = koina::ulid::Ulid::new();
    let tmp_path = parent.join(format!(".{file_name}.{unique}.tmp"));

    // WARNING: every early return below MUST clean up `tmp_path`, or a failed
    // write leaves an orphan dotfile in the operator's vault.
    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        // WHY: fsync the file data before rename so the rename does not expose
        // an empty or short file if the machine loses power mid-write.
        file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(InternalSnafu {
            message: format!(
                "failed to stage workspace write for {}: {e}",
                dest.display()
            ),
        }
        .build());
    }

    if let Err(e) = std::fs::rename(&tmp_path, dest) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(InternalSnafu {
            message: format!(
                "failed to commit workspace write for {}: {e}",
                dest.display()
            ),
        }
        .build());
    }

    // WHY: fsync the directory so the rename itself is durable; a crash after a
    // successful rename could otherwise lose the directory entry update.
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }

    std::fs::metadata(dest).map_err(|e| {
        InternalSnafu {
            message: format!("failed to stat written file {}: {e}", dest.display()),
        }
        .build()
    })
}

/// Extract a file mtime as milliseconds since the Unix epoch.
fn file_mtime_ms(metadata: &std::fs::Metadata) -> Result<i64, ApiError> {
    let modified = metadata.modified().map_err(|e| {
        InternalSnafu {
            message: format!("failed to read file mtime: {e}"),
        }
        .build()
    })?;
    let millis = modified
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    Ok(millis)
}

fn normalize_relative_path(path: &str) -> Result<PathBuf, ApiError> {
    if path.trim().is_empty() {
        return Err(BadRequestSnafu {
            message: "workspace path must not be empty".to_owned(),
        }
        .build());
    }
    if path.contains('\0') {
        return Err(BadRequestSnafu {
            message: "workspace path must not contain NUL bytes".to_owned(),
        }
        .build());
    }

    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(BadRequestSnafu {
            message: format!("workspace path must be relative: {path}"),
        }
        .build());
    }

    for component in candidate.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(BadRequestSnafu {
                    message: format!("workspace path must not contain '..' segments: {path}"),
                }
                .build());
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(BadRequestSnafu {
                    message: format!("workspace path must be relative: {path}"),
                }
                .build());
            }
        }
    }

    Ok(candidate.to_path_buf())
}

fn relative_workspace_path(root: &Path, path: &Path) -> Result<String, ApiError> {
    let relative = path.strip_prefix(root).map_err(|_err| {
        InternalSnafu {
            message: format!(
                "workspace entry {} is not rooted under {}",
                path.display(),
                root.display()
            ),
        }
        .build()
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn parse_git_status_line(line: &str) -> Option<GitStatusEntry> {
    if line.starts_with("?? ") {
        let path = line.get(3..)?.trim();
        return Some(GitStatusEntry {
            path: path.replace('\\', "/"),
            status: "?".to_owned(),
        });
    }

    if line.len() < 3 {
        return None;
    }

    let bytes = line.as_bytes();
    let Some([x, y, ..]) = bytes.get(..2) else {
        return None;
    };
    let x = char::from(*x);
    let y = char::from(*y);
    let status = match (x, y) {
        ('D', _) | (_, 'D') => "D",
        ('A', _) | (_, 'A') => "A",
        ('M' | 'R', _) | (_, 'M' | 'R') => "M",
        ('C', _) | (_, 'C') => "A",
        _ => return None,
    };

    let raw_path = line.get(3..)?.trim();
    let path = raw_path.split(" -> ").last().unwrap_or(raw_path);
    if path.is_empty() {
        return None;
    }

    Some(GitStatusEntry {
        path: path.replace('\\', "/"),
        status: status.to_owned(),
    })
}

fn is_not_a_git_repo(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("not a git repository") || stderr.contains("does not have a commit checked out")
}

fn search_workspace(
    workspace_root: &Path,
    current_dir: &Path,
    query_lower: &str,
    limit: usize,
    results: &mut Vec<SearchResult>,
) -> Result<(), ApiError> {
    if results.len() >= limit {
        return Ok(());
    }

    let entries = std::fs::read_dir(current_dir).map_err(|e| {
        InternalSnafu {
            message: format!(
                "failed to walk workspace directory {}: {e}",
                current_dir.display()
            ),
        }
        .build()
    })?;

    for entry in entries {
        if results.len() >= limit {
            break;
        }

        let entry = entry.map_err(|e| {
            InternalSnafu {
                message: format!(
                    "failed to read workspace entry in {}: {e}",
                    current_dir.display()
                ),
            }
            .build()
        })?;
        let file_type = entry.file_type().map_err(|e| {
            InternalSnafu {
                message: format!(
                    "failed to inspect workspace entry type {}: {e}",
                    entry.path().display()
                ),
            }
            .build()
        })?;

        if file_type.is_symlink() {
            continue;
        }

        let path = entry.path();
        if file_type.is_dir() {
            search_workspace(workspace_root, &path, query_lower, limit, results)?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = relative_workspace_path(workspace_root, &path)?;
        let mut matched = false;
        let mut snippet = String::new();
        let mut line_number = 1usize;

        if name.to_lowercase().contains(query_lower) {
            matched = true;
            snippet.clone_from(&name);
        } else if let Ok(content) = std::fs::read_to_string(&path) {
            for (idx, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(query_lower) {
                    matched = true;
                    line_number = idx + 1;
                    snippet = line.trim().chars().take(200).collect();
                    break;
                }
            }
        }

        if matched {
            results.push(SearchResult {
                path: relative,
                line: line_number,
                snippet,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_status_line_normalizes_porcelain_codes() {
        let Some(added) = parse_git_status_line("A  src/main.rs") else {
            panic!("added");
        };
        assert_eq!(added.status, "A");
        assert_eq!(added.path, "src/main.rs");

        let Some(untracked) = parse_git_status_line("?? notes.txt") else {
            panic!("untracked");
        };
        assert_eq!(untracked.status, "?");
        assert_eq!(untracked.path, "notes.txt");
    }
}
