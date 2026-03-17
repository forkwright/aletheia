/// Strip markdown code fences from an LLM response.
pub(super) fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    }
}

/// Slugify a string: NFC-normalize, lowercase, spaces to hyphens, keep alphanumeric and hyphens.
///
/// Unicode Normalization Form C is applied first so that visually identical strings
/// with different codepoint sequences (e.g. composed vs decomposed "café") produce the
/// same slug.
#[cfg(any(feature = "mneme-engine", test))]
pub(super) fn slugify(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    let normalized: String = s.nfc().collect();
    normalized
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .fold(String::new(), |mut acc, part| {
            if !acc.is_empty() {
                acc.push('-');
            }
            acc.push_str(part);
            acc
        })
}
