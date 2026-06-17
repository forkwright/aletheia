/// Strip markdown code fences from an LLM response.
///
/// If the response starts with a code fence but the closing fence is missing,
/// the opening fence marker is still stripped and a warning is logged. Without
/// this, the fence marker would be included in the JSON string, causing a parse
/// error with no clear root cause.
pub(super) fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(stripped) = rest.strip_suffix("```") {
            stripped.trim()
        } else {
            tracing::warn!(
                "LLM response has opening ```json fence but no closing ```, stripping opening fence only"
            );
            rest.trim()
        }
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(stripped) = rest.strip_suffix("```") {
            stripped.trim()
        } else {
            tracing::warn!(
                "LLM response has opening ``` fence but no closing ```, stripping opening fence only"
            );
            rest.trim()
        }
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
pub(crate) fn slugify(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    let normalized: String = s.nfc().collect();
    // WHY: ASCII-only — is_alphanumeric() is Unicode-aware and would let Tamil,
    // Cyrillic, etc. pass through. Restricting to ASCII alnum keeps slugs safe
    // for filenames, URL paths, and Datalog relation keys.
    normalized
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
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
