//! `{{file:path:start:end}}` template interpolation.
//!
//! Resolves at prompt-assembly / tool-dispatch time. Hard failures on missing
//! files or out-of-bounds line ranges — silent empty strings let stale refs
//! appear to work, which violates the no-false-capability discipline.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use snafu::{IntoError as _, Snafu};

/// Errors from file-ref interpolation.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
#[non_exhaustive]
pub enum InterpError {
    /// The requested file does not exist.
    #[snafu(display("file not found: {}", path.display()))]
    FileNotFound { path: PathBuf },

    /// The requested line range exceeds the file's actual line count.
    #[snafu(display(
        "line range {requested_start}..{requested_end} out of bounds; file has {actual_lines} lines: {}",
        path.display()
    ))]
    OutOfBounds {
        path: PathBuf,
        requested_start: usize,
        requested_end: usize,
        actual_lines: usize,
    },

    /// Absolute paths are rejected by default.
    #[snafu(display("absolute path not allowed: {}", path.display()))]
    AbsolutePathRejected { path: PathBuf },

    /// An I/O error occurred while reading the file.
    #[snafu(display("io error reading {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    /// A line number in the template could not be parsed.
    #[snafu(display("invalid line number in template: {value}"))]
    InvalidLineNumber { value: String },
}

/// Expand all `{{file:path:start:end}}` references in `text`.
///
/// Paths are resolved relative to `workspace_root`. Absolute paths are rejected
/// unless the `allow-absolute-file-refs` feature is enabled.
///
/// Line numbers are 1-indexed and inclusive. If `start` is absent the range
/// begins at line 1; if `end` is absent the range runs to the end of the file.
/// Missing files and out-of-bounds ranges produce hard errors.
///
/// # Errors
///
/// Returns [`InterpError`] if a file is missing, a range is out of bounds,
/// an absolute path is supplied (and the feature is off), an I/O error
/// occurs, or a line number is invalid.
#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex pattern cannot fail"
)]
pub fn expand_file_refs(text: &str, workspace_root: &Path) -> Result<String, InterpError> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // INVARIANT: compile-time constant regex pattern cannot fail to compile.
        Regex::new(r"\{\{file:([^:}]+)(?::(\d+))?(?::(\d+))?\}\}")
            .expect("compile-time constant regex pattern is valid")
    });

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;

    for cap in re.captures_iter(text) {
        let Some(m) = cap.get(0) else {
            continue;
        };
        result.push_str(text.get(last_end..m.start()).unwrap_or(""));

        let Some(path_match) = cap.get(1) else {
            continue;
        };
        let path_str = path_match.as_str();

        let start = match cap.get(2) {
            Some(m) => match m.as_str().parse::<usize>() {
                Ok(n) => Some(n),
                Err(_) => {
                    return InvalidLineNumberSnafu {
                        value: m.as_str().to_owned(),
                    }
                    .fail();
                }
            },
            None => None,
        };

        let end = match cap.get(3) {
            Some(m) => match m.as_str().parse::<usize>() {
                Ok(n) => Some(n),
                Err(_) => {
                    return InvalidLineNumberSnafu {
                        value: m.as_str().to_owned(),
                    }
                    .fail();
                }
            },
            None => None,
        };

        let resolved = resolve_file_ref(path_str, start, end, workspace_root)?;
        result.push_str(&resolved);

        last_end = m.end();
    }

    result.push_str(text.get(last_end..).unwrap_or(""));
    Ok(result)
}

/// Recursively expand file refs in every JSON string value.
///
/// Objects and arrays are traversed depth-first. Non-string values are cloned
/// unchanged.
///
/// # Errors
///
/// Returns [`InterpError`] on the first file-ref that fails to resolve.
pub fn expand_file_refs_in_json(
    value: &serde_json::Value,
    workspace_root: &Path,
) -> Result<serde_json::Value, InterpError> {
    match value {
        serde_json::Value::String(s) => {
            let expanded = expand_file_refs(s, workspace_root)?;
            Ok(serde_json::Value::String(expanded))
        }
        serde_json::Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                out.push(expand_file_refs_in_json(v, workspace_root)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), expand_file_refs_in_json(v, workspace_root)?);
            }
            Ok(serde_json::Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn resolve_file_ref(
    path_str: &str,
    start: Option<usize>,
    end: Option<usize>,
    workspace_root: &Path,
) -> Result<String, InterpError> {
    let path = Path::new(path_str);

    #[cfg(not(feature = "allow-absolute-file-refs"))]
    if path.is_absolute() {
        return AbsolutePathRejectedSnafu {
            path: path.to_path_buf(),
        }
        .fail();
    }

    let full_path = workspace_root.join(path);
    if !full_path.exists() {
        return FileNotFoundSnafu { path: full_path }.fail();
    }

    let content = std::fs::read_to_string(&full_path).map_err(|e| {
        IoSnafu {
            path: full_path.clone(),
        }
        .into_error(e)
    })?;

    let lines: Vec<&str> = content.lines().collect();
    let line_count = lines.len();

    let start_idx = start.unwrap_or(1);
    let end_idx = end.unwrap_or(line_count);

    if start_idx == 0
        || end_idx == 0
        || start_idx > line_count
        || end_idx > line_count
        || start_idx > end_idx
    {
        return OutOfBoundsSnafu {
            path: full_path,
            requested_start: start_idx,
            requested_end: end_idx,
            actual_lines: line_count,
        }
        .fail();
    }

    let slice = lines.get(start_idx - 1..end_idx).unwrap_or(&[]);
    Ok(slice.join("\n"))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::io::Write;

    use super::*;

    fn make_temp_file(dir: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).expect("create temp file");
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                file.write_all(b"\n").expect("write newline");
            }
            file.write_all(line.as_bytes()).expect("write line");
        }
        path
    }

    #[test]
    fn expand_basic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        make_temp_file(
            tmp.path(),
            "foo.txt",
            &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"],
        );

        let text = "{{file:foo.txt:3:5}}";
        let result = expand_file_refs(text, tmp.path()).expect("expand");
        assert_eq!(result, "c\nd\ne");
    }

    #[test]
    fn expand_full_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        make_temp_file(tmp.path(), "foo.txt", &["a", "b", "c"]);

        let text = "{{file:foo.txt}}";
        let result = expand_file_refs(text, tmp.path()).expect("expand");
        assert_eq!(result, "a\nb\nc");
    }

    #[test]
    fn expand_start_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        make_temp_file(tmp.path(), "foo.txt", &["a", "b", "c", "d"]);

        let text = "{{file:foo.txt:3}}";
        let result = expand_file_refs(text, tmp.path()).expect("expand");
        assert_eq!(result, "c\nd");
    }

    #[test]
    fn missing_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let text = "{{file:nonexistent.txt}}";
        let err = expand_file_refs(text, tmp.path()).expect_err("should fail");
        assert!(
            matches!(err, InterpError::FileNotFound { .. }),
            "expected FileNotFound, got {err:?}"
        );
    }

    #[test]
    fn out_of_bounds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        make_temp_file(tmp.path(), "foo.txt", &["a", "b", "c"]);

        let text = "{{file:foo.txt:1:1000}}";
        let err = expand_file_refs(text, tmp.path()).expect_err("should fail");
        assert!(
            matches!(
                err,
                InterpError::OutOfBounds {
                    requested_start: 1,
                    requested_end: 1000,
                    actual_lines: 3,
                    ..
                }
            ),
            "expected OutOfBounds, got {err:?}"
        );
    }

    #[test]
    #[cfg(not(feature = "allow-absolute-file-refs"))]
    fn absolute_path_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let text = "{{file:/etc/passwd:1:1}}";
        let err = expand_file_refs(text, tmp.path()).expect_err("should fail");
        assert!(
            matches!(err, InterpError::AbsolutePathRejected { .. }),
            "expected AbsolutePathRejected, got {err:?}"
        );
    }

    #[test]
    fn multiple_refs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        make_temp_file(tmp.path(), "a.txt", &["one", "two"]);
        make_temp_file(tmp.path(), "b.txt", &["three", "four"]);

        let text = "A: {{file:a.txt:1:1}} B: {{file:b.txt:2:2}}";
        let result = expand_file_refs(text, tmp.path()).expect("expand");
        assert_eq!(result, "A: one B: four");
    }

    #[test]
    fn no_match() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let text = "hello world";
        let result = expand_file_refs(text, tmp.path()).expect("expand");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn expand_json_object() {
        let tmp = tempfile::tempdir().expect("tempdir");
        make_temp_file(tmp.path(), "data.txt", &["line1", "line2"]);

        let value = serde_json::json!({
            "path": "{{file:data.txt:1:1}}",
            "nested": {
                "content": "{{file:data.txt:2:2}}"
            },
            "arr": ["{{file:data.txt}}"],
            "num": 42
        });

        let result = expand_file_refs_in_json(&value, tmp.path()).expect("expand");
        let expected = serde_json::json!({
            "path": "line1",
            "nested": {
                "content": "line2"
            },
            "arr": ["line1\nline2"],
            "num": 42
        });
        assert_eq!(result, expected);
    }
}
