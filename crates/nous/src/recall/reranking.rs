//! Iterative retrieval helpers: terminology discovery and gap detection.

use std::collections::{HashMap, HashSet};

use tracing::debug;

use mneme::recall::ScoredResult;

/// Check if a word is a common English stopword.
pub(super) fn is_stopword(word: &str) -> bool {
    aletheia_lexica::stopwords::ENGLISH_STOPWORDS.contains(&word)
}

/// Minimum fraction of a sub-question's content words a candidate must contain
/// to count as evidence answering that gap.
pub(super) const EVIDENCE_MATCH_THRESHOLD: f64 = 0.5;

/// Tokenize text into lowercased alphanumeric content words.
///
/// Matches [`discover_terminology`]'s filter: trims non-alphanumerics, keeps
/// words longer than three characters that are not stopwords.
fn content_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| w.len() > 3 && !is_stopword(w))
        .collect()
}

/// Build the answered-evidence map for iterative (`MemR3`) retrieval.
///
/// Decomposes `query` into sub-questions via [`EvidenceGapTracker`], records
/// every `(content, source_id)` candidate whose content lexically covers a
/// sub-question's content words at or above [`EVIDENCE_MATCH_THRESHOLD`] as
/// evidence (confidence = coverage ratio), then flattens the tracker's answered
/// set into a `source_id -> confidence` map for
/// [`RecallEngine::score_evidence_coverage`](mneme::recall::RecallEngine::score_evidence_coverage).
///
/// Built from the merged candidate set so gap-answering facts surfaced in
/// either retrieval cycle are credited.
///
/// WHY: the heuristic decomposition + lexical coverage match the no-LLM design
/// of `EvidenceGapTracker` itself, so the same query produces a stable gap map
/// without an extra model round-trip.
pub(super) fn build_evidence_map<'a, I>(query: &str, candidates: I) -> HashMap<String, f64>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut tracker = mneme::evidence_gap::EvidenceGapTracker::new(query);
    let sub_questions: Vec<String> = tracker.query().sub_questions.clone();

    // PERF: pre-tokenize candidates once (token set + source_id) to avoid
    // re-tokenizing per sub-question.
    let tokenized: Vec<(HashSet<String>, &str)> = candidates
        .into_iter()
        .map(|(content, source_id)| (content_tokens(content).into_iter().collect(), source_id))
        .collect();

    for (idx, sub_q) in sub_questions.iter().enumerate() {
        let sq_tokens = content_tokens(sub_q);
        let sq_set: HashSet<&str> = sq_tokens.iter().map(String::as_str).collect();
        // WHY: a sub-question carrying fewer than two content words is too vague
        // to match meaningfully — any candidate mentioning that single word would
        // clear the coverage threshold and falsely answer the gap. Require at
        // least two content words before recording evidence.
        if sq_set.len() < 2 {
            continue;
        }
        let total = u32::try_from(sq_set.len()).unwrap_or(u32::MAX);

        for (cand_tokens, source_id) in &tokenized {
            let hits = u32::try_from(sq_set.iter().filter(|t| cand_tokens.contains(**t)).count())
                .unwrap_or(u32::MAX);
            let coverage = f64::from(hits) / f64::from(total);
            if coverage >= EVIDENCE_MATCH_THRESHOLD {
                tracker.record_evidence(idx, source_id, coverage);
            }
        }
    }

    let mut map: HashMap<String, f64> = HashMap::new();
    for aq in &tracker.query().answered {
        for id in &aq.evidence_ids {
            map.entry(id.clone())
                .and_modify(|c| *c = c.max(aq.confidence))
                .or_insert(aq.confidence);
        }
    }
    map
}

/// Extract domain-specific terms from first-pass results not present in the original query.
///
/// Splits result content on whitespace, filters stopwords and short words,
/// then returns the top-5 most frequent novel terms.
pub(super) fn discover_terminology(results: &[ScoredResult], original_query: &str) -> Vec<String> {
    let query_words: HashSet<String> = original_query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();

    let mut term_freq: HashMap<String, usize> = HashMap::new();
    for result in results {
        for word in result.content.split_whitespace() {
            let cleaned = word
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if cleaned.len() > 3 && !query_words.contains(&cleaned) && !is_stopword(&cleaned) {
                *term_freq.entry(cleaned).or_default() += 1;
            }
        }
    }

    let mut terms: Vec<_> = term_freq.into_iter().collect();
    terms.sort_by_key(|b| std::cmp::Reverse(b.1));
    terms.into_iter().take(5).map(|(t, _)| t).collect()
}

/// Detect entity references in results that aren't captured as result IDs.
///
/// Scans for capitalized multi-word phrases (2+ consecutive capitalized words)
/// and quoted strings. These represent referenced-but-unretrieved entities.
pub(super) fn detect_gaps(results: &[ScoredResult]) -> Vec<String> {
    let source_ids: HashSet<&str> = results.iter().map(|r| r.source_id.as_str()).collect();
    let mut gaps: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for result in results {
        let words: Vec<&str> = result.content.split_whitespace().collect();
        let mut i = 0;
        while i < words.len() {
            #[expect(
                clippy::indexing_slicing,
                reason = "i < words.len() is checked by the while guard above"
            )]
            if starts_with_uppercase(words[i]) {
                let start = i;
                while i < words.len() {
                    #[expect(
                        clippy::indexing_slicing,
                        reason = "i < words.len() is checked by the while guard"
                    )]
                    if !starts_with_uppercase(words[i]) {
                        break;
                    }
                    i += 1;
                }
                if i - start >= 2 {
                    #[expect(
                        clippy::indexing_slicing,
                        reason = "start and i are both bounded by words.len()"
                    )]
                    let phrase = words[start..i].join(" ");
                    if !source_ids.contains(phrase.as_str()) && seen.insert(phrase.clone()) {
                        gaps.push(phrase);
                    }
                }
            } else {
                i += 1;
            }
        }

        for quoted in extract_quoted_strings(&result.content) {
            if !source_ids.contains(quoted.as_str()) && seen.insert(quoted.clone()) {
                gaps.push(quoted);
            }
        }
    }

    debug!(count = gaps.len(), "detected gaps in recall results");
    gaps
}

fn starts_with_uppercase(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

fn extract_quoted_strings(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split('"').collect();
    parts
        .iter()
        .enumerate()
        .filter(|(i, part)| i % 2 == 1 && !part.is_empty() && part.len() < 100)
        .map(|(_, part)| (*part).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scored(content: &str, source_id: &str) -> ScoredResult {
        ScoredResult {
            content: content.to_owned(),
            source_type: "fact".to_owned(),
            source_id: source_id.to_owned(),
            nous_id: "syn".to_owned(),
            factors: mneme::recall::FactorScores::default(),
            score: 0.0,
            sensitivity: mneme::knowledge::FactSensitivity::Public,
            visibility: mneme::knowledge::Visibility::Private,
            scope: None,
            project_id: None,
        }
    }

    #[test]
    fn evidence_map_credits_gap_answering_facts() {
        // A compound query decomposes into two sub-questions on " and ".
        let query = "what is the capital of France and what is the population of France";
        let ranked = [
            scored(
                "The capital of France is Paris, a major European city.",
                "fact-capital",
            ),
            scored(
                "The population of France is roughly 68 million people.",
                "fact-population",
            ),
            scored(
                "Bananas are an excellent source of potassium.",
                "fact-banana",
            ),
        ];

        let map = build_evidence_map(
            query,
            ranked
                .iter()
                .map(|s| (s.content.as_str(), s.source_id.as_str())),
        );

        assert!(
            map.contains_key("fact-capital"),
            "capital fact should answer a gap: {map:?}"
        );
        assert!(
            map.contains_key("fact-population"),
            "population fact should answer a gap: {map:?}"
        );
        assert!(
            !map.contains_key("fact-banana"),
            "unrelated fact must not be credited: {map:?}"
        );
        for confidence in map.values() {
            assert!(
                *confidence > 0.0 && *confidence <= 1.0,
                "confidence {confidence} should be in (0, 1]"
            );
        }
    }

    #[test]
    fn evidence_map_empty_without_lexical_overlap() {
        let ranked = [scored(
            "The weather today is sunny and mild.",
            "fact-weather",
        )];
        let map = build_evidence_map(
            "quantum chromodynamics lattice gauge theory",
            ranked
                .iter()
                .map(|s| (s.content.as_str(), s.source_id.as_str())),
        );
        assert!(
            map.is_empty(),
            "no lexical overlap should yield no evidence: {map:?}"
        );
    }

    #[test]
    fn evidence_map_skips_single_token_subquestions() {
        // A query that decomposes to one content word ("population") must not
        // credit a candidate merely mentioning that word — a single-token gap is
        // too vague to count as answered.
        let ranked = [scored(
            "The population census was conducted across the region in 2020",
            "fact-census",
        )];
        let map = build_evidence_map(
            "population",
            ranked
                .iter()
                .map(|s| (s.content.as_str(), s.source_id.as_str())),
        );
        assert!(
            map.is_empty(),
            "single-content-word sub-question must not credit tangential facts: {map:?}"
        );
    }
}
