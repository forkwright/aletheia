//! Simple subsequence fuzzy matcher for command palette and slash completion.

/// Result of a fuzzy match, containing the score and match positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    /// Match score (higher is better).
    pub score: i64,
    /// Indices of matched characters in the candidate string.
    pub indices: Vec<usize>,
}

/// A simple subsequence fuzzy matcher.
///
/// Scores are calculated based on:
/// - Consecutive matches (bonus)
/// - Word boundary matches (bonus)
/// - Start of string match (bonus)
/// - Shorter candidates with same match quality score higher
#[derive(Debug, Clone, Copy, Default)]
pub struct FuzzyMatcher;

impl FuzzyMatcher {
    /// Create a new fuzzy matcher.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Match a pattern against a candidate string.
    ///
    /// Returns `Some(MatchResult)` if the pattern is a subsequence of the candidate,
    /// `None` otherwise. The match is case-insensitive.
    ///
    /// # Examples
    ///
    /// ```
    /// use theatron_tui::fuzzy::FuzzyMatcher;
    ///
    /// let matcher = FuzzyMatcher::new();
    /// assert!(matcher.fuzzy_match("quit", "q").is_some());
    /// assert!(matcher.fuzzy_match("quit", "qt").is_some());
    /// assert!(matcher.fuzzy_match("quit", "xyz").is_none());
    /// ```
    pub fn fuzzy_match(&self, candidate: &str, pattern: &str) -> Option<MatchResult> {
        if pattern.is_empty() {
            return Some(MatchResult {
                score: 0,
                indices: Vec::new(),
            });
        }

        let candidate_lower = candidate.to_lowercase();
        let pattern_lower = pattern.to_lowercase();

        let mut indices = Vec::new();
        let mut pattern_chars = pattern_lower.chars().peekable();
        let mut current_pattern_char = pattern_chars.next()?;

        for (idx, candidate_char) in candidate_lower.char_indices() {
            if candidate_char == current_pattern_char {
                indices.push(idx);
                match pattern_chars.next() {
                    Some(c) => current_pattern_char = c,
                    None => break,
                }
            }
        }

        // Pattern not fully matched
        if indices.len() != pattern.len() {
            return None;
        }

        let score = self.calculate_score(candidate, &indices);

        Some(MatchResult { score, indices })
    }

    /// Calculate a score for a match based on heuristics.
    fn calculate_score(&self, candidate: &str, indices: &[usize]) -> i64 {
        let mut score: i64 = 100; // Base score

        // Bonus for matching at the start
        if let Some(&first) = indices.first() {
            if first == 0 {
                score += 50;
            }
        }

        // Bonus for consecutive matches
        for window in indices.windows(2) {
            let prev = window[0];
            let curr = window[1];
            let prev_char = candidate.chars().nth(prev);
            let curr_char = candidate.chars().nth(curr);

            if let (Some(p), Some(c)) = (prev_char, curr_char) {
                // Consecutive character bonus
                if curr == prev + p.len_utf8() {
                    score += 30;
                }

                // Word boundary bonus (after space, hyphen, underscore, etc.)
                if is_word_boundary(p) && !is_word_boundary(c) {
                    score += 25;
                }
            }
        }

        // Penalty for length (shorter is better)
        let candidate_len = candidate.chars().count();
        score -= candidate_len as i64 * 2;

        // Bonus for matching more of the pattern
        score += indices.len() as i64 * 10;

        score
    }
}

/// Check if a character is a word boundary.
fn is_word_boundary(c: char) -> bool {
    c.is_whitespace() || c == '-' || c == '_' || c == '.' || c == '/' || c == ':'
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn empty_pattern_matches_everything() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.fuzzy_match("hello", "").unwrap();
        assert_eq!(result.score, 0);
        assert!(result.indices.is_empty());
    }

    #[test]
    fn exact_match_scores_high() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.fuzzy_match("quit", "quit").unwrap();
        assert!(result.score > 100);
    }

    #[test]
    fn partial_match_works() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.fuzzy_match("sessions", "sess").unwrap();
        assert_eq!(result.indices, vec![0, 1, 2, 3]);
        assert!(result.score > 0);
    }

    #[test]
    fn case_insensitive_match() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.fuzzy_match("Sessions", "sess").unwrap();
        assert_eq!(result.indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn non_match_returns_none() {
        let matcher = FuzzyMatcher::new();
        assert!(matcher.fuzzy_match("quit", "xyz").is_none());
    }

    #[test]
    fn consecutive_bonus_applied() {
        let matcher = FuzzyMatcher::new();
        // "sess" in "sessions" is consecutive
        let consecutive = matcher.fuzzy_match("sessions", "sess").unwrap();
        // "sns" in "sessions" is not consecutive
        let non_consecutive = matcher.fuzzy_match("sessions", "sns").unwrap();
        assert!(consecutive.score > non_consecutive.score);
    }

    #[test]
    fn start_bonus_applied() {
        let matcher = FuzzyMatcher::new();
        // "quit" at start
        let at_start = matcher.fuzzy_match("quit now", "quit").unwrap();
        // "quit" not at start
        let not_at_start = matcher.fuzzy_match("please quit", "quit").unwrap();
        assert!(at_start.score > not_at_start.score);
    }

    #[test]
    fn word_boundary_bonus() {
        let matcher = FuzzyMatcher::new();
        // "cmd" matches at word boundaries in "my-cmd-here"
        let boundary_match = matcher.fuzzy_match("my-cmd-here", "cmd").unwrap();
        assert!(boundary_match.score > 0);
    }

    #[test]
    fn shorter_candidate_scores_higher() {
        let matcher = FuzzyMatcher::new();
        let short = matcher.fuzzy_match("quit", "q").unwrap();
        let long = matcher.fuzzy_match("quite-long-name-here", "q").unwrap();
        assert!(short.score > long.score);
    }

    #[test]
    fn fuzzy_skips_characters() {
        let matcher = FuzzyMatcher::new();
        // "qt" matches "quit" by skipping 'u' and 'i'
        let result = matcher.fuzzy_match("quit", "qt").unwrap();
        assert_eq!(result.indices, vec![0, 3]);
    }

    #[test]
    fn unicode_handling() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.fuzzy_match("héllo world", "hw").unwrap();
        assert_eq!(result.indices, vec![0, 7]);
    }
}
