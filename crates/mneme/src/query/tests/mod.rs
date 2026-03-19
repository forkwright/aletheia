use super::*;
use crate::engine::DataValue;

/// Normalize whitespace for comparison: collapse runs of whitespace to single
/// space, trim, then remove spaces adjacent to brackets/braces (`CozoDB`
/// ignores these formatting differences).
fn normalize(s: &str) -> String {
    let collapsed: String = s.split_whitespace().fold(String::new(), |mut acc, word| {
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(word);
        acc
    });
    collapsed
        .replace("[ ", "[")
        .replace(" ]", "]")
        .replace("{ ", "{")
        .replace(" }", "}")
}

mod builders;
mod datalog;
mod fields;
