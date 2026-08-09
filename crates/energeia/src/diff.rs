//! Shared unified-diff parsing helpers used by both the QA gate and the
//! steward pipeline.

use std::path::Path;

/// Parse the new-file start line from a unified diff hunk header.
///
/// Format: `@@ -old_start,old_count +new_start,new_count @@`
#[must_use]
pub(crate) fn parse_hunk_new_start(hunk_line: &str) -> Option<u32> {
    let plus_idx = hunk_line.find('+')?;
    let after_plus = hunk_line.get(plus_idx + 1..)?;
    let end = after_plus.find(|c: char| !c.is_ascii_digit())?;
    after_plus.get(..end)?.parse().ok()
}

/// Whether `child` lies under directory `parent`, compared component-wise.
///
/// Component-based matching (rather than string prefix matching) prevents
/// path tricks: `src/lib` as a parent does not match `src/library/mod.rs`
/// because `library` is a different path component than `lib`.
#[must_use]
pub(crate) fn path_is_under(child: &Path, parent: &Path) -> bool {
    let parent_components: Vec<_> = parent.components().collect();
    let child_components: Vec<_> = child.components().collect();

    if parent_components.len() > child_components.len() {
        return false;
    }

    parent_components
        .iter()
        .zip(child_components.iter())
        .all(|(p, c)| p == c)
}

/// Whether every file in `changed_files` falls within `blast_radius`.
///
/// An empty `blast_radius` allows all files (no declared scope means no
/// restriction). Each entry ending in `/` is a directory scope -- anything
/// under it is allowed; other entries must match a changed file exactly.
///
/// WHY: shared between the QA mechanical gate (`qa::mechanical`, which
/// reports every out-of-scope file as a `MechanicalIssue`) and the steward
/// pipeline (which only needs the yes/no verdict) so the path-matching rules
/// -- and their tests -- live in exactly one place.
#[must_use]
pub(crate) fn all_files_within_blast_radius(changed_files: &[String], blast_radius: &[String]) -> bool {
    if blast_radius.is_empty() {
        return true;
    }

    changed_files.iter().all(|file| {
        let file_path = Path::new(file);
        blast_radius.iter().any(|allowed| {
            if let Some(dir) = allowed.strip_suffix('/') {
                path_is_under(file_path, Path::new(dir))
            } else {
                file_path == Path::new(allowed)
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hunk_new_start_normal() {
        assert_eq!(parse_hunk_new_start("@@ -1,3 +1,5 @@"), Some(1));
        assert_eq!(parse_hunk_new_start("@@ -10,2 +42,7 @@"), Some(42));
    }

    #[test]
    fn parse_hunk_new_start_single_line_old() {
        assert_eq!(parse_hunk_new_start("@@ -1 +1,2 @@"), Some(1));
    }

    #[test]
    fn parse_hunk_new_start_single_line_new() {
        // WHY: no comma in the `+` section is the edge case that motivated
        // deduplicating this parser -- both call sites must agree on it.
        assert_eq!(parse_hunk_new_start("@@ -0,0 +1 @@"), Some(1));
    }

    #[test]
    fn parse_hunk_new_start_invalid() {
        assert_eq!(parse_hunk_new_start("not a hunk header"), None);
    }

    #[test]
    fn blast_radius_empty_allows_all() {
        let files = vec!["anything/at/all.rs".to_owned()];
        assert!(all_files_within_blast_radius(&files, &[]));
    }

    #[test]
    fn blast_radius_directory_scope_allows_nested_file() {
        let files = vec!["crates/energeia/src/steward/service.rs".to_owned()];
        let radius = vec!["crates/energeia/".to_owned()];
        assert!(all_files_within_blast_radius(&files, &radius));
    }

    #[test]
    fn blast_radius_rejects_sibling_directory_with_similar_prefix() {
        // WHY: `src/lib` as a declared scope must not match `src/library/mod.rs`.
        let files = vec!["src/library/mod.rs".to_owned()];
        let radius = vec!["src/lib/".to_owned()];
        assert!(!all_files_within_blast_radius(&files, &radius));
    }

    #[test]
    fn blast_radius_exact_file_match() {
        let files = vec!["Cargo.toml".to_owned()];
        let radius = vec!["Cargo.toml".to_owned()];
        assert!(all_files_within_blast_radius(&files, &radius));
    }

    #[test]
    fn blast_radius_one_file_outside_fails_whole_set() {
        let files = vec![
            "crates/energeia/src/lib.rs".to_owned(),
            "crates/other/src/lib.rs".to_owned(),
        ];
        let radius = vec!["crates/energeia/".to_owned()];
        assert!(!all_files_within_blast_radius(&files, &radius));
    }
}
