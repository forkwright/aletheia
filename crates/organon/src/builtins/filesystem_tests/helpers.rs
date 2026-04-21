//! (Split from `filesystem_tests.rs` — see parent mod.)

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;

#[test]
fn test_is_glob_pattern_detects_star() {
    assert!(
        is_glob_pattern("*.rs"),
        "pattern with star should be detected as a glob"
    );
}

#[test]
fn test_is_glob_pattern_detects_question_mark() {
    assert!(
        is_glob_pattern("file?.txt"),
        "pattern with question mark should be detected as a glob"
    );
}

#[test]
fn test_is_glob_pattern_detects_brackets() {
    assert!(
        is_glob_pattern("[abc]def"),
        "pattern with brackets should be detected as a glob"
    );
}

#[test]
fn test_is_glob_pattern_detects_braces() {
    assert!(
        is_glob_pattern("{foo,bar}"),
        "pattern with braces should be detected as a glob"
    );
}

#[test]
fn test_is_glob_pattern_returns_false_for_plain_word() {
    assert!(
        !is_glob_pattern("hello"),
        "plain word should not be detected as a glob"
    );
    assert!(
        !is_glob_pattern("file.txt"),
        "plain filename should not be detected as a glob"
    );
    assert!(
        !is_glob_pattern("some_identifier"),
        "plain identifier should not be detected as a glob"
    );
}

#[test]
fn test_days_to_ymd_known_epoch_gives_1970_01_01() {
    let (y, m, d) = days_to_ymd(0);
    assert_eq!(
        (y, m, d),
        (1970, 1, 1),
        "day 0 should correspond to the Unix epoch 1970-01-01"
    );
}

#[test]
fn test_days_to_ymd_365_days_gives_1971_01_01() {
    let (y, m, d) = days_to_ymd(365);
    assert_eq!(
        (y, m, d),
        (1971, 1, 1),
        "day 365 after a non-leap year should be 1971-01-01"
    );
}

#[test]
fn test_days_to_ymd_known_date_2000_01_01() {
    let (y, m, d) = days_to_ymd(10957);
    assert_eq!(
        (y, m, d),
        (2000, 1, 1),
        "day 10957 from epoch should correspond to 2000-01-01"
    );
}

#[test]
fn test_truncate_output_short_string_returned_unchanged() {
    let short = "hello world".to_owned();
    assert_eq!(
        truncate_output(short.clone()),
        short,
        "short string should be returned unchanged by truncate_output"
    );
}

#[test]
fn test_truncate_output_long_string_appends_truncation_marker() {
    let long = "x".repeat(MAX_OUTPUT_BYTES + 100);
    let result = truncate_output(long);
    assert!(
        result.ends_with("[output truncated]"),
        "should end with truncation marker"
    );
    assert!(
        result.len() <= MAX_OUTPUT_BYTES + 20,
        "truncated result should be close to limit"
    );
}

#[test]
fn test_truncate_output_exactly_at_limit_unchanged() {
    let exactly = "y".repeat(MAX_OUTPUT_BYTES);
    let result = truncate_output(exactly.clone());
    assert_eq!(result, exactly, "exactly at limit should be unchanged");
}

#[test]
fn test_truncate_output_multibyte_at_boundary_produces_valid_utf8() {
    // WHY: If MAX_OUTPUT_BYTES falls in the middle of a multi-byte character,
    // naive truncation produces invalid UTF-8. This test places a 4-byte emoji
    // exactly at the truncation boundary to verify char-boundary-aware
    // truncation. Closes #3335.
    let emoji = "\u{1F600}"; // 4 bytes
    let padding_len = MAX_OUTPUT_BYTES - 2; // 2 bytes short of limit
    let mut input = "x".repeat(padding_len);
    input.push_str(emoji); // total = padding_len + 4 > MAX_OUTPUT_BYTES
    assert!(
        input.len() > MAX_OUTPUT_BYTES,
        "test input should exceed limit"
    );

    let result = truncate_output(input);

    // The result must be valid UTF-8 (it's a String, so this is guaranteed
    // at the type level, but verify the emoji was cleanly removed rather than
    // partially included).
    assert!(
        result.ends_with("[output truncated]"),
        "should end with truncation marker"
    );
    assert!(
        !result.contains(emoji),
        "emoji spanning the boundary should be removed entirely"
    );
    // Verify no partial bytes leaked: the text portion before the marker
    // should be valid on its own.
    let text_part = result.trim_end_matches("\n[output truncated]");
    assert!(
        text_part.len() <= MAX_OUTPUT_BYTES,
        "text portion should not exceed limit"
    );
    assert!(
        text_part.len() >= padding_len,
        "text portion should retain the padding"
    );
}

#[test]
fn test_grep_def_has_pattern_as_required() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    let tn = ToolName::new("grep").expect("valid");
    let def = reg.get_def(&tn).expect("grep registered");
    assert!(
        def.input_schema.required.contains(&"pattern".to_owned()),
        "grep tool definition should require the pattern field"
    );
}

#[test]
fn test_find_def_has_pattern_as_required() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    let tn = ToolName::new("find").expect("valid");
    let def = reg.get_def(&tn).expect("find registered");
    assert!(
        def.input_schema.required.contains(&"pattern".to_owned()),
        "find tool definition should require the pattern field"
    );
}

#[test]
fn test_ls_def_has_no_required_fields() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    let tn = ToolName::new("ls").expect("valid");
    let def = reg.get_def(&tn).expect("ls registered");
    assert!(
        def.input_schema.required.is_empty(),
        "ls should have no required fields"
    );
}

#[test]
fn test_find_def_type_field_has_enum_values() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    let tn = ToolName::new("find").expect("valid");
    let def = reg.get_def(&tn).expect("find registered");
    let type_prop = def.input_schema.properties.get("type").expect("type prop");
    let enum_vals = type_prop.enum_values.as_ref().expect("enum values");
    assert!(
        enum_vals.contains(&"f".to_owned()),
        "find type field should include 'f' for file"
    );
    assert!(
        enum_vals.contains(&"d".to_owned()),
        "find type field should include 'd' for directory"
    );
}
