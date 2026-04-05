//! File overlap detection and merge ordering for concurrent PRs.
//!
//! When multiple PRs are ready to merge, their changed files may overlap.
//! Overlapping PRs must merge sequentially to avoid conflicts; non-overlapping
//! PRs can merge in parallel. This module computes merge groups based on file overlap.

use std::collections::{HashMap, HashSet};

use super::types::ClassifiedPr;

/// Compute the merge order for a set of PRs based on file overlaps.
///
/// Returns groups of PR numbers. Each group can merge in parallel (no
/// file overlaps within a group). Groups execute sequentially -- the first
/// group merges, then rebase/retry, then the next group.
///
/// Algorithm:
/// 1. Build a conflict graph (undirected edges between PRs sharing files)
/// 2. Greedy graph coloring to partition PRs into independent sets
/// 3. Each color becomes a merge group
#[must_use]
pub fn compute_merge_order(prs: &[ClassifiedPr]) -> Vec<Vec<u64>> {
    if prs.is_empty() {
        return Vec::new();
    }

    if prs.len() == 1 {
        if let Some(first) = prs.first() {
            return vec![vec![first.pr.number]];
        }
        return Vec::new();
    }

    // NOTE: Step 1 -- build the conflict graph as an adjacency list.
    let pr_numbers: Vec<u64> = prs.iter().map(|p| p.pr.number).collect();
    let mut conflicts: HashMap<u64, HashSet<u64>> = HashMap::new();

    for i in 0..prs.len() {
        for j in (i + 1)..prs.len() {
            let Some(pr_i) = prs.get(i) else { continue };
            let Some(pr_j) = prs.get(j) else { continue };
            let overlap = file_overlap(pr_i, pr_j);
            if !overlap.is_empty() {
                conflicts
                    .entry(pr_i.pr.number)
                    .or_default()
                    .insert(pr_j.pr.number);
                conflicts
                    .entry(pr_j.pr.number)
                    .or_default()
                    .insert(pr_i.pr.number);
            }
        }
    }

    // NOTE: Step 2 -- greedy graph coloring. PRs with fewer conflicts first
    // (least overlap first) to minimize the number of groups.
    let mut sorted_prs: Vec<u64> = pr_numbers;
    sorted_prs.sort_by_key(|n| {
        let conflict_count = conflicts.get(n).map_or(0, HashSet::len);
        // WHY: Sort by (conflict_count ascending, pr_number ascending) so
        // that PRs with the fewest conflicts get assigned first and PRs
        // with no conflicts are guaranteed to land in the first group.
        (conflict_count, *n)
    });

    let mut colors: HashMap<u64, usize> = HashMap::new();
    let mut num_groups: usize = 0;

    for &pr_num in &sorted_prs {
        // NOTE: Find the smallest color not used by any neighbor.
        let neighbor_colors: HashSet<usize> = conflicts
            .get(&pr_num)
            .map(|neighbors| {
                neighbors
                    .iter()
                    .filter_map(|n| colors.get(n).copied())
                    .collect()
            })
            .unwrap_or_default();

        let mut color = 0;
        while neighbor_colors.contains(&color) {
            color += 1;
        }

        colors.insert(pr_num, color);
        if color >= num_groups {
            num_groups = color + 1;
        }
    }

    // NOTE: Step 3 -- collect PR numbers into groups by color.
    let mut groups: Vec<Vec<u64>> = vec![Vec::new(); num_groups];
    for &pr_num in &sorted_prs {
        if let Some(&color) = colors.get(&pr_num)
            && let Some(group) = groups.get_mut(color)
        {
            group.push(pr_num);
        }
    }

    // NOTE: Sort within each group for deterministic output.
    for group in &mut groups {
        group.sort_unstable();
    }

    // NOTE: Remove empty groups (shouldn't happen, but defensive).
    groups.retain(|g| !g.is_empty());

    groups
}

/// Return the list of files changed by both PRs.
#[must_use]
pub fn file_overlap(a: &ClassifiedPr, b: &ClassifiedPr) -> Vec<String> {
    let a_files: HashSet<&str> = a.changed_files.iter().map(String::as_str).collect();
    let b_files: HashSet<&str> = b.changed_files.iter().map(String::as_str).collect();

    let mut overlap: Vec<String> = a_files
        .intersection(&b_files)
        .map(|s| (*s).to_string())
        .collect();

    // NOTE: Sort for deterministic output.
    overlap.sort();

    overlap
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::steward::types::{CiStatus, PullRequest};

    fn make_classified(number: u64, files: Vec<&str>) -> ClassifiedPr {
        ClassifiedPr {
            pr: PullRequest {
                number,
                title: format!("PR #{number}"),
                head_ref_name: None,
                head_sha: None,
                state: None,
                mergeable: Some("MERGEABLE".to_string()),
                body: None,
                updated_at: None,
                merged_at: None,
            },
            ci_status: CiStatus::Green,
            changed_files: files
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect(),
            prompt_number: Some(u32::try_from(number).expect("test PR number fits in u32")),
            blast_radius_ok: true,
            merge_safe: true,
            has_gate_trailer: true,
            suppression_findings: Vec::new(),
            qa_verdict: None,
        }
    }

    #[test]
    fn file_overlap_detects_shared_files() {
        let a = make_classified(1, vec!["src/main.rs", "src/lib.rs"]);
        let b = make_classified(2, vec!["src/lib.rs", "src/utils.rs"]);

        let overlap = file_overlap(&a, &b);
        assert_eq!(overlap, vec!["src/lib.rs"]);
    }

    #[test]
    fn file_overlap_empty_when_no_shared_files() {
        let a = make_classified(1, vec!["src/main.rs"]);
        let b = make_classified(2, vec!["src/utils.rs"]);

        let overlap = file_overlap(&a, &b);
        assert!(overlap.is_empty());
    }

    #[test]
    fn file_overlap_multiple_shared() {
        let a = make_classified(1, vec!["a.rs", "b.rs", "c.rs"]);
        let b = make_classified(2, vec!["b.rs", "c.rs", "d.rs"]);

        let overlap = file_overlap(&a, &b);
        assert_eq!(overlap, vec!["b.rs", "c.rs"]);
    }

    #[test]
    fn merge_order_empty_input() {
        let groups = compute_merge_order(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn merge_order_single_pr() {
        let prs = vec![make_classified(1, vec!["src/main.rs"])];
        let groups = compute_merge_order(&prs);
        assert_eq!(groups, vec![vec![1]]);
    }

    #[test]
    fn merge_order_no_overlaps_single_group() {
        let prs = vec![
            make_classified(1, vec!["src/a.rs"]),
            make_classified(2, vec!["src/b.rs"]),
            make_classified(3, vec!["src/c.rs"]),
        ];

        let groups = compute_merge_order(&prs);

        // WHY: All PRs can merge in parallel when there are no overlaps.
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![1, 2, 3]);
    }

    #[test]
    fn merge_order_full_overlap_sequential() {
        let prs = vec![
            make_classified(1, vec!["src/shared.rs"]),
            make_classified(2, vec!["src/shared.rs"]),
            make_classified(3, vec!["src/shared.rs"]),
        ];

        let groups = compute_merge_order(&prs);

        // WHY: All PRs overlap, so each must be in its own group.
        assert_eq!(groups.len(), 3);
        // NOTE: Each group should contain exactly one PR.
        let total: usize = groups.iter().map(std::vec::Vec::len).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn merge_order_partial_overlap() {
        // PR 1 touches a.rs, PR 2 touches a.rs + b.rs, PR 3 touches c.rs
        // -> PR 1 and PR 2 overlap, PR 3 is independent
        let prs = vec![
            make_classified(1, vec!["a.rs"]),
            make_classified(2, vec!["a.rs", "b.rs"]),
            make_classified(3, vec!["c.rs"]),
        ];

        let groups = compute_merge_order(&prs);

        // NOTE: PR 3 can go with either 1 or 2 (no overlap), but 1 and 2 must be separate.
        assert!(groups.len() >= 2);

        // NOTE: Verify that 1 and 2 are never in the same group.
        for group in &groups {
            assert!(!(group.contains(&1) && group.contains(&2)));
        }

        // NOTE: Verify all PRs are present.
        let all_prs: Vec<u64> = groups.iter().flat_map(|g| g.iter().copied()).collect();
        assert!(all_prs.contains(&1));
        assert!(all_prs.contains(&2));
        assert!(all_prs.contains(&3));
    }
}
