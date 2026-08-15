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
///
/// WHY(#4414): unconditional, not `mneme-engine`-gated — `crate::skill::export_skills_to_cc`
/// (an ungated public function used by the CLI and the energeia bridge) and
/// `extract::engine` both call this without a feature guard, and `unicode-normalization`
/// is already a non-optional crate dependency, so gating cost nothing and only broke
/// every default-feature build that omits `mneme-engine` (caught by the new episteme
/// gliner/nuextract feature-gate CI step, the first build to exercise that combination).
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
