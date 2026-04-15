//! String literal parsing for quoted, single-quoted, and raw strings.
//!
//! The three string forms share escape-sequence handling; this module
//! factors the common logic into [`parse_escape_sequences`] so that
//! `parse_quoted_string` and `parse_s_quoted_string` are not duplicated.
#![expect(
    clippy::as_conversions,
    clippy::pedantic,
    clippy::result_large_err,
    reason = "String parser — numeric cast for Unicode escapes, InternalError is the crate-wide Result type"
)]

use compact_str::CompactString;

use crate::error::InternalResult as Result;
use crate::parse::error::InvalidQuerySnafu;
use crate::parse::{Pair, Rule};

use super::parse_int;

/// Parse any string literal (double-quoted, single-quoted, raw, or bare ident).
///
/// # Errors
///
/// Returns an error if the string contains invalid escape sequences or if
/// the pair rule is not a recognized string type.
pub(crate) fn parse_string(pair: Pair<'_>) -> Result<CompactString> {
    match pair.as_rule() {
        Rule::quoted_string => parse_quoted_string(pair),
        Rule::s_quoted_string => parse_s_quoted_string(pair),
        Rule::raw_string => parse_raw_string(pair),
        Rule::ident => Ok(CompactString::from(pair.as_str())),
        r => {
            return Err(InvalidQuerySnafu {
                message: format!(
                    "unexpected rule {:?} in string parser - grammar mismatch, please file a bug",
                    r
                ),
            }
            .build()
            .into());
        }
    }
}

/// Parse a double-quoted string literal, processing escape sequences.
fn parse_quoted_string(pair: Pair<'_>) -> Result<CompactString> {
    let pairs = pair
        .into_inner()
        .next()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "empty quoted string".to_string(),
            }
            .build()
        })?
        .into_inner();
    let mut ret = CompactString::default();
    for pair in pairs {
        parse_escape_sequence(pair.as_str(), '"', &mut ret)?;
    }
    Ok(ret)
}

/// Parse a single-quoted string literal, processing escape sequences.
fn parse_s_quoted_string(pair: Pair<'_>) -> Result<CompactString> {
    let pairs = pair
        .into_inner()
        .next()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "empty single-quoted string".to_string(),
            }
            .build()
        })?
        .into_inner();
    let mut ret = CompactString::default();
    for pair in pairs {
        parse_escape_sequence(pair.as_str(), '\'', &mut ret)?;
    }
    Ok(ret)
}

/// Parse a raw string literal (no escape processing).
fn parse_raw_string(pair: Pair<'_>) -> Result<CompactString> {
    Ok(CompactString::from(
        pair.into_inner()
            .next()
            .ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "empty raw string".to_string(),
                }
                .build()
            })?
            .as_str(),
    ))
}

/// Process a single segment of a string literal, handling escape sequences.
///
/// `quote_char` is `'"'` for double-quoted strings or `'\''` for single-quoted.
/// Common escapes (`\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`) are
/// shared; the quote-specific escape is determined by `quote_char`.
fn parse_escape_sequence(
    segment: &str,
    quote_char: char,
    output: &mut CompactString,
) -> Result<()> {
    // WHY: build the quote escape string (e.g. `\"` or `\'`) for matching.
    let quote_escape: String = format!("\\{quote_char}");

    match segment {
        s if s == quote_escape => output.push(quote_char),
        r"\\" => output.push('\\'),
        r"\/" => output.push('/'),
        r"\b" => output.push('\x08'),
        r"\f" => output.push('\x0c'),
        r"\n" => output.push('\n'),
        r"\r" => output.push('\r'),
        r"\t" => output.push('\t'),
        s if s.starts_with(r"\u") => {
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
            let code = parse_int(s, 16)? as u32;
            let ch = char::from_u32(code).ok_or_else(|| {
                InvalidQuerySnafu {
                    message: format!("invalid UTF8 code {code}"),
                }
                .build()
            })?;
            output.push(ch);
        }
        s if s.starts_with('\\') => {
            return Err(InvalidQuerySnafu {
                message: format!("invalid escape sequence {s}"),
            }
            .build()
            .into());
        }
        s => output.push_str(s),
    }
    Ok(())
}
