//! Tests for string operations and regex.
#![expect(clippy::expect_used, reason = "test assertions")]
use regex::Regex;

use crate::data::functions::*;
use crate::data::value::DataValue;
use crate::data::value::RegexWrapper;
#[test]
fn test_concat() {
    assert_eq!(
        op_concat(&[DataValue::Str("abc".into()), DataValue::Str("def".into())])
            .expect("test assertion"),
        DataValue::Str("abcdef".into()),
        "string concatenation should join strings"
    );

    assert_eq!(
        op_concat(&[
            DataValue::List(vec![DataValue::from(true), DataValue::from(false)]),
            DataValue::List(vec![DataValue::from(true)])
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::from(true),
            DataValue::from(false),
            DataValue::from(true),
        ]),
        "list concatenation should join lists"
    );
}

#[test]
fn test_str_includes() {
    assert_eq!(
        op_str_includes(&[
            DataValue::Str("abcdef".into()),
            DataValue::Str("bcd".into())
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "str_includes should return true when substring is present"
    );
    assert_eq!(
        op_str_includes(&[DataValue::Str("abcdef".into()), DataValue::Str("bd".into())])
            .expect("test assertion"),
        DataValue::from(false),
        "str_includes should return false when substring is absent"
    );
}

#[test]
fn test_casings() {
    assert_eq!(
        op_lowercase(&[DataValue::Str("NAÏVE".into())]).expect("test assertion"),
        DataValue::Str("naïve".into()),
        "lowercase should handle unicode correctly"
    );
    assert_eq!(
        op_uppercase(&[DataValue::Str("naïve".into())]).expect("test assertion"),
        DataValue::Str("NAÏVE".into()),
        "uppercase should handle unicode correctly"
    );
}

#[test]
fn test_trim() {
    assert_eq!(
        op_trim(&[DataValue::Str(" a ".into())]).expect("test assertion"),
        DataValue::Str("a".into()),
        "trim should remove leading and trailing whitespace"
    );
    assert_eq!(
        op_trim_start(&[DataValue::Str(" a ".into())]).expect("test assertion"),
        DataValue::Str("a ".into()),
        "trim_start should remove only leading whitespace"
    );
    assert_eq!(
        op_trim_end(&[DataValue::Str(" a ".into())]).expect("test assertion"),
        DataValue::Str(" a".into()),
        "trim_end should remove only trailing whitespace"
    );
}

#[test]
fn test_starts_ends_with() {
    assert_eq!(
        op_starts_with(&[
            DataValue::Str("abcdef".into()),
            DataValue::Str("abc".into())
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "string starting with given prefix should return true"
    );
    assert_eq!(
        op_starts_with(&[DataValue::Str("abcdef".into()), DataValue::Str("bc".into())])
            .expect("test assertion"),
        DataValue::from(false),
        "string not starting with given prefix should return false"
    );
    assert_eq!(
        op_ends_with(&[
            DataValue::Str("abcdef".into()),
            DataValue::Str("def".into())
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "string ending with given suffix should return true"
    );
    assert_eq!(
        op_ends_with(&[DataValue::Str("abcdef".into()), DataValue::Str("bc".into())])
            .expect("test assertion"),
        DataValue::from(false),
        "string not ending with given suffix should return false"
    );
}

#[test]
fn test_regex() {
    assert_eq!(
        op_regex_matches(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("c.e").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "regex matching interior substring should return true"
    );

    assert_eq!(
        op_regex_matches(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("c.ef$").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "regex matching at end of string should return true"
    );

    assert_eq!(
        op_regex_matches(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("c.e$").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::from(false),
        "regex requiring end anchor that does not match should return false"
    );

    assert_eq!(
        op_regex_replace(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("[be]").expect("test assertion"))),
            DataValue::Str("x".into())
        ])
        .expect("test assertion"),
        DataValue::Str("axcdef".into()),
        "regex_replace should replace only the first match"
    );

    assert_eq!(
        op_regex_replace_all(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("[be]").expect("test assertion"))),
            DataValue::Str("x".into())
        ])
        .expect("test assertion"),
        DataValue::Str("axcdxf".into()),
        "regex_replace_all should replace all matches"
    );
    assert_eq!(
        op_regex_extract(&[
            DataValue::Str("abCDefGH".into()),
            DataValue::Regex(RegexWrapper(
                Regex::new("[xayef]|(GH)").expect("test assertion")
            ))
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::Str("a".into()),
            DataValue::Str("e".into()),
            DataValue::Str("f".into()),
            DataValue::Str("GH".into()),
        ]),
        "regex_extract should return all matches in order"
    );
    assert_eq!(
        op_regex_extract_first(&[
            DataValue::Str("abCDefGH".into()),
            DataValue::Regex(RegexWrapper(
                Regex::new("[xayef]|(GH)").expect("test assertion")
            ))
        ])
        .expect("test assertion"),
        DataValue::Str("a".into()),
        "regex_extract_first should return first match"
    );
    assert_eq!(
        op_regex_extract(&[
            DataValue::Str("abCDefGH".into()),
            DataValue::Regex(RegexWrapper(Regex::new("xyz").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::List(vec![]),
        "regex_extract with no matches should return empty list"
    );

    assert_eq!(
        op_regex_extract_first(&[
            DataValue::Str("abCDefGH".into()),
            DataValue::Regex(RegexWrapper(Regex::new("xyz").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::Null,
        "regex_extract_first with no matches should return Null"
    );
}
