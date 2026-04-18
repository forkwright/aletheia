// Banned word and phrase list from kanon WRITING.md.
// WHY: hardcoded at compile time — zero runtime overhead, no external file to lose.

use super::{Finding, FindingKind, LineFix};

/// An entry in the banned word/phrase table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BannedEntry {
    /// The word or phrase to detect (matched case-insensitively).
    pub(crate) pattern: &'static str,
    /// Suggested replacement shown in the finding message.
    pub(crate) suggestion: &'static str,
    /// When true, require word boundaries on both sides of the match.
    /// WHY: single words like "robust" must not match inside "robustness" in a
    /// medical context where the prefix is unrelated. Phrases are already
    /// specific enough that substring matching is safe.
    pub(crate) whole_word: bool,
    /// Auto-fix replacement, or None when the fix requires human judgment.
    pub(crate) fix: Option<&'static str>,
}

/// Full banned list: AI tropes, filler phrases, and additional AI vocabulary.
/// Source: kanon `crates/basanos/standards/WRITING.md`.
pub(crate) static BANNED: &[BannedEntry] = &[
    // ── AI tropes ─────────────────────────────────────────────────────────────
    BannedEntry {
        pattern: "delve",
        suggestion: "examine, explore, or look at",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "leverage",
        suggestion: "use",
        whole_word: true,
        fix: Some("use"),
    },
    BannedEntry {
        pattern: "utilize",
        suggestion: "use",
        whole_word: true,
        fix: Some("use"),
    },
    BannedEntry {
        pattern: "facilitate",
        suggestion: "enable, support, or describe what happens",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "streamline",
        suggestion: "simplify, reduce, or speed up",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "robust",
        suggestion: "describe what makes it strong",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "comprehensive",
        suggestion: "complete, thorough, or list what is covered",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "enhance",
        suggestion: "improve, add, extend, or describe the change",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "foster",
        suggestion: "encourage, support, or build",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "showcase",
        suggestion: "demonstrate, show, or present",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "underscore",
        suggestion: "emphasize or highlight (or restructure)",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "illuminate",
        suggestion: "explain, clarify, or show",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "elucidate",
        suggestion: "explain, clarify, or show",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "navigate",
        suggestion: "handle, address, or work through (if metaphorical)",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "embark",
        suggestion: "start or begin",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "harness",
        suggestion: "use or apply",
        whole_word: true,
        fix: Some("use"),
    },
    BannedEntry {
        pattern: "pivotal",
        suggestion: "important, critical, or explain why",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "intricate",
        suggestion: "complex or detailed",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "meticulous",
        suggestion: "careful, thorough, or precise",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "multifaceted",
        suggestion: "complex, or describe the facets",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "nuanced",
        suggestion: "describe the actual nuance",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "paramount",
        suggestion: "important or critical",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "profound",
        suggestion: "significant, or describe the depth",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "groundbreaking",
        suggestion: "new, first, or original (with evidence)",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "holistic",
        suggestion: "complete, whole-system, or end-to-end",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "invaluable",
        suggestion: "valuable, essential, or explain why",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "tapestry",
        suggestion: "describe the actual domain",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "landscape",
        suggestion: "describe the actual domain",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "realm",
        suggestion: "describe the actual domain",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "testament",
        suggestion: "evidence of, or demonstrates",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "journey",
        suggestion: "process, progression, or describe it (if metaphorical)",
        whole_word: true,
        fix: None,
    },
    // ── Filler phrases ────────────────────────────────────────────────────────
    BannedEntry {
        pattern: "it's worth noting",
        suggestion: "state the thing directly",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "it should be noted",
        suggestion: "state the thing directly",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "in order to",
        suggestion: "use \"to\"",
        whole_word: false,
        fix: Some("to"),
    },
    BannedEntry {
        pattern: "a wide range of",
        suggestion: "many, several, or state the count",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "at the end of the day",
        suggestion: "delete or restate the conclusion",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "as such",
        suggestion: "so, therefore, or restructure",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "in terms of",
        suggestion: "about, for, or regarding",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "deep dive",
        suggestion: "examine, analyze, or investigate",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "dive deep",
        suggestion: "examine, analyze, or investigate",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "in today's",
        suggestion: "delete entirely",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "in the ever-evolving",
        suggestion: "delete entirely",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "when it comes to",
        suggestion: "for, regarding, or restructure",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "serves as",
        suggestion: "is",
        whole_word: false,
        fix: Some("is"),
    },
    BannedEntry {
        pattern: "functions as",
        suggestion: "is",
        whole_word: false,
        fix: Some("is"),
    },
    BannedEntry {
        pattern: "stands as",
        suggestion: "is",
        whole_word: false,
        fix: Some("is"),
    },
    BannedEntry {
        pattern: "plays a crucial role",
        suggestion: "matters or contributes (describe how)",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "plays a key role",
        suggestion: "matters or contributes (describe how)",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "plays a vital role",
        suggestion: "matters or contributes (describe how)",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "aims to bridge",
        suggestion: "connects or links (describe the connection)",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "paving the way",
        suggestion: "enabling, or describe what it enables",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "sheds light on",
        suggestion: "explains, reveals, or shows",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "at its core",
        suggestion: "fundamentally, or just state the core thing",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "key takeaway",
        suggestion: "the point is, or just state it",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "going forward",
        suggestion: "from now on, or delete",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "please note that",
        suggestion: "state the thing",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "as mentioned above",
        suggestion: "reference by name or omit",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "as mentioned below",
        suggestion: "reference by name or omit",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "needless to say",
        suggestion: "then don't say it",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "as a matter of fact",
        suggestion: "delete, state the fact",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "arguably",
        suggestion: "state the argument or don't",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "it seems",
        suggestion: "investigate and state the finding",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "it appears",
        suggestion: "investigate and state the finding",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "basically",
        suggestion: "say the thing directly",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "essentially",
        suggestion: "say the thing directly",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "in this context",
        suggestion: "the reader knows the context; delete",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "not only",
        suggestion: "use when genuinely contrasting; never as a default sentence opener",
        whole_word: false,
        fix: None,
    },
    // ── Additional AI vocabulary ──────────────────────────────────────────────
    BannedEntry {
        pattern: "notably",
        suggestion: "state the thing",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "crucially",
        suggestion: "state why it matters",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "specifically",
        suggestion: "delete",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "importantly",
        suggestion: "restructure to show importance",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "interestingly",
        suggestion: "state the thing",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "accordingly",
        suggestion: "so, or as a result",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "consequently",
        suggestion: "so, or as a result",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "furthermore",
        suggestion: "and, also, or delete",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "additionally",
        suggestion: "and, also, or delete",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "ultimately",
        suggestion: "delete or state the conclusion",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "seamless",
        suggestion: "describe the actual integration",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "seamlessly",
        suggestion: "describe the actual integration",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "straightforward",
        suggestion: "describe the actual simplicity",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "in conclusion",
        suggestion: "don't conclude; the last paragraph stands alone",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "in summary",
        suggestion: "don't summarize; the content stands alone",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "as previously mentioned",
        suggestion: "name the thing",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "it is worth mentioning",
        suggestion: "state the thing",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "worth noting",
        suggestion: "state the thing",
        whole_word: false,
        fix: None,
    },
    BannedEntry {
        pattern: "currently",
        suggestion: "state the behavior directly",
        whole_word: true,
        fix: None,
    },
    BannedEntry {
        pattern: "presently",
        suggestion: "state the behavior directly",
        whole_word: true,
        fix: None,
    },
];

/// Scan `effective_lines` for banned words and phrases.
///
/// `effective_lines` must contain only non-comment lines. Each tuple is
/// `(1-indexed line number, line content)`.
pub(crate) fn check(effective_lines: &[(usize, &str)]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for &(line_num, line) in effective_lines {
        let lower = line.to_lowercase();

        for entry in BANNED {
            let lower_pattern = entry.pattern.to_lowercase();
            let pattern_len = lower_pattern.len();
            debug_assert!(
                !lower_pattern.is_empty(),
                "banned pattern must not be empty: {entry:?}"
            );

            let mut search_start = 0usize;
            while search_start < lower.len() {
                let Some(tail) = lower.get(search_start..) else {
                    break;
                };
                let Some(offset) = tail.find(lower_pattern.as_str()) else {
                    break;
                };
                let match_start = search_start + offset;
                let match_end = match_start + pattern_len;

                let matched = if line.is_char_boundary(match_start)
                    && line.is_char_boundary(match_end)
                {
                    // SAFETY: both boundaries are verified as char boundaries above
                    #[expect(
                        clippy::string_slice,
                        reason = "both boundaries are verified as char boundaries immediately above"
                    )]
                    &line[match_start..match_end]
                } else {
                    // NOTE: non-ASCII boundary shift from lowercasing; skip safely.
                    search_start = match_start + 1;
                    continue;
                };

                let at_boundary =
                    !entry.whole_word || is_word_boundary(&lower, match_start, match_end);

                if at_boundary {
                    let fix = entry.fix.map(|replacement| LineFix {
                        line_number: line_num,
                        matched: matched.to_owned(),
                        replacement: replacement.to_owned(),
                    });

                    findings.push(Finding {
                        line_start: line_num,
                        line_end: line_num,
                        message: format!("banned word {:?} -> {}", matched, entry.suggestion),
                        kind: FindingKind::BannedWord,
                        fix,
                    });
                }

                search_start = match_start + pattern_len;
            }
        }
    }

    findings
}

/// Return true when the byte range `[start, end)` in `text` falls on word
/// boundaries (i.e., adjacent characters are not alphanumeric or apostrophe).
fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    // SAFETY: start and end are char boundaries (verified by callers before this fn)
    #[expect(
        clippy::string_slice,
        reason = "start and end are verified as char boundaries by the caller before calling is_word_boundary"
    )]
    let before_ok = start == 0
        || text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '\'');

    #[expect(
        clippy::string_slice,
        reason = "end is verified as a char boundary by the caller before calling is_word_boundary"
    )]
    let after_ok = end >= text.len()
        || text[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '\'');

    before_ok && after_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banned_list_meets_minimum_count() {
        assert!(
            BANNED.len() >= 73,
            "banned word list must have at least 73 entries, found {}",
            BANNED.len()
        );
    }

    #[test]
    fn detects_single_banned_word() {
        let lines = [(1usize, "The approach is robust and comprehensive.")];
        let findings = check(&lines);
        let messages: Vec<&str> = findings.iter().map(|f| f.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("\"robust\"")),
            "must flag 'robust'"
        );
        assert!(
            messages.iter().any(|m| m.contains("\"comprehensive\"")),
            "must flag 'comprehensive'"
        );
    }

    #[test]
    fn whole_word_boundary_respected() {
        // "robustness" contains "robust" but is a different word; must not flag.
        let lines = [(1usize, "The robustness of the system was tested.")];
        let findings = check(&lines);
        assert!(
            !findings.iter().any(|f| f.message.contains("\"robust\"")),
            "must not flag 'robust' inside 'robustness'"
        );
    }

    #[test]
    fn detects_filler_phrase() {
        // WHY: use lowercase so the matched text in the message matches the assertion.
        let lines = [(1usize, "in order to understand, we examine the data.")];
        let findings = check(&lines);
        assert!(
            findings
                .iter()
                .any(|f| f.message.contains("\"in order to\"")),
            "must flag 'in order to'"
        );
    }
}
