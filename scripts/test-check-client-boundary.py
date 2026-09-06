#!/usr/bin/env python3
"""Tests for check-client-boundary.py.

Covers the tokenizer (the part this script exists to get right -- a doc
comment quoting plain English in double quotes must never be mistaken for a
string boundary, and vice versa), the three detection signals (reqwest,
`/api/v1/` string literals, parse_stream_event-shaped decoders), the
test-scope exclusion (`#[cfg(test)]`/`#[test]`/`#[tokio::test]`), and the
one-directional ratchet's four required behaviors: unchanged passes, growth
fails, shrink passes and prints the ratchet delta, and a new file fails.
Mirrors test-check-stub-accountability.py's harness: no pytest dependency,
plain `expect()` assertions collected into one FAILURES list.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_client_boundary",
    Path(__file__).resolve().parent / "check-client-boundary.py",
)
assert SPEC and SPEC.loader
CHECK = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECK
SPEC.loader.exec_module(CHECK)

FAILURES: list[str] = []


def expect(label: str, cond: bool, detail: str = "") -> None:
    if not cond:
        FAILURES.append(f"{label}: {detail}" if detail else label)


# --------------------------------------------------------------------------
# tokenize: the failure mode this script exists to avoid -- a two-pass
# "blank literals, then find comments" approach desyncs the instant a doc
# comment quotes plain English, and this codebase does that routinely.


def test_tokenize_doc_comment_prose_quotes_do_not_start_a_string() -> None:
    text = (
        '/// a 413 is "split the note", a 409 is "reload before saving"\n'
        'fn f() { let url = "{base}/api/v1/nous"; }\n'
    )
    segments = CHECK.tokenize(text)
    strings = [text[s:e] for s, e, k in segments if k == "string"]
    expect(
        "exactly one real string literal found",
        strings == ['"{base}/api/v1/nous"'],
        f"got {strings!r}",
    )


def test_tokenize_line_comment() -> None:
    text = 'let x = 1; // reqwest lives here\nlet y = "/api/v1/nous";\n'
    segments = CHECK.tokenize(text)
    kinds_at_comment = {k for s, e, k in segments if "reqwest" in text[s:e]}
    expect("comment text tagged as comment", kinds_at_comment == {"comment"}, f"got {kinds_at_comment!r}")
    strings = [text[s:e] for s, e, k in segments if k == "string"]
    expect("string after the comment still found", strings == ['"/api/v1/nous"'], f"got {strings!r}")


def test_tokenize_block_comment_with_slash_slash_inside() -> None:
    # WHY: a `//` inside a `/* */` block must not end the block comment early
    # or be separately mistaken for a line comment.
    text = '/* see https://example.com/api/v1/nous */\nlet y = "/api/v1/real";\n'
    segments = CHECK.tokenize(text)
    strings = [text[s:e] for s, e, k in segments if k == "string"]
    expect("only the real string outside the block comment found", strings == ['"/api/v1/real"'], f"got {strings!r}")


def test_tokenize_nested_block_comment() -> None:
    text = '/* outer /* inner */ still outer */\nlet y = "/api/v1/real";\n'
    segments = CHECK.tokenize(text)
    strings = [text[s:e] for s, e, k in segments if k == "string"]
    expect("nested block comment closes at outer terminator", strings == ['"/api/v1/real"'], f"got {strings!r}")


def test_tokenize_raw_string_with_hashes() -> None:
    text = 'let pat = r#"contains a "quote" and /api/v1/looks/like/a/path"#;\n'
    segments = CHECK.tokenize(text)
    strings = [text[s:e] for s, e, k in segments if k == "string"]
    expect("raw string with hash delimiter captured whole", len(strings) == 1, f"got {strings!r}")
    if strings:
        expect("embedded quote did not end it early", 'looks/like/a/path' in strings[0], strings[0])


def test_tokenize_char_literal_holding_a_quote_does_not_desync() -> None:
    # WHY: `'"'` is a char literal whose payload IS a double quote. A scanner
    # that does not special-case char literals reads that `"` as a string
    # start and misparses everything after it.
    text = "let c = '\"';\nlet y = \"/api/v1/real\";\n"
    segments = CHECK.tokenize(text)
    strings = [text[s:e] for s, e, k in segments if k == "string"]
    expect("string after a quote-holding char literal still found", strings == ['"/api/v1/real"'], f"got {strings!r}")


def test_tokenize_char_literal_holding_a_brace_does_not_desync_braces() -> None:
    text = "fn f() { let c = '{'; }\n"
    segments = CHECK.tokenize(text)
    code_text = CHECK.blank_spans(text, [(s, e) for s, e, k in segments if k != "code"])
    depth = code_text.count("{") - code_text.count("}")
    expect("brace count balanced despite char literal payload", depth == 0, f"got depth {depth}")


def test_tokenize_lifetime_is_not_a_char_literal() -> None:
    text = "fn f<'a>(x: &'a str) -> &'a str { x }\n"
    segments = CHECK.tokenize(text)
    # A lifetime has no closing quote nearby -- CHAR_LITERAL_RE must not
    # match it, and the whole thing should just be ordinary code.
    kinds = {k for s, e, k in segments}
    expect("lifetime produces no string/comment segment", kinds <= {"code"}, f"got {kinds!r}")


# --------------------------------------------------------------------------
# scan_file: the three detection signals


def test_scan_file_detects_reqwest_use() -> None:
    text = "use reqwest::Client;\nfn f() -> reqwest::Client { todo!() }\n"
    occs = CHECK.scan_file("fixture.rs", text)
    kinds = [o.kind for o in occs]
    expect("two reqwest occurrences found", kinds.count("reqwest") == 2, f"got {occs!r}")


def test_scan_file_ignores_reqwest_mentioned_only_in_a_comment() -> None:
    text = "// WHY: reqwest 0.13 requires a rustls crypto provider\nfn f() {}\n"
    occs = CHECK.scan_file("fixture.rs", text)
    expect("comment-only mention is not a candidate", occs == [], f"got {occs!r}")


def test_scan_file_detects_api_v1_literal_in_format_string() -> None:
    text = 'fn f(base: &str) -> String { format!("{base}/api/v1/nous") }\n'
    occs = CHECK.scan_file("fixture.rs", text)
    expect("one api-literal occurrence found", len(occs) == 1 and occs[0].kind == "api-literal", f"got {occs!r}")


def test_scan_file_ignores_api_v1_quoted_only_in_a_doc_comment() -> None:
    text = '/// see `GET /api/v1/nous` -- quoted here: "/api/v1/nous" in prose\nfn f() {}\n'
    occs = CHECK.scan_file("fixture.rs", text)
    expect("doc-comment-only mention is not a candidate", occs == [], f"got {occs!r}")


def test_scan_file_ignores_literal_without_v1() -> None:
    text = 'fn f() -> &\'static str { "/api/health" }\n'
    occs = CHECK.scan_file("fixture.rs", text)
    expect("a bare /api/ literal without v1 is not a candidate", occs == [], f"got {occs!r}")


def test_scan_file_detects_sse_decoder_shaped_function() -> None:
    text = "fn parse_stream_event(line: &str) -> Option<Event> { None }\n"
    occs = CHECK.scan_file("fixture.rs", text)
    expect("sse-decode occurrence found", len(occs) == 1 and occs[0].kind == "sse-decode", f"got {occs!r}")


def test_scan_file_detects_sse_decoder_helper_names() -> None:
    for name in ("str_field", "str_any_field", "opt_str_any_field", "stream_error_message"):
        text = f"fn {name}() {{}}\n"
        occs = CHECK.scan_file("fixture.rs", text)
        expect(f"helper {name!r} detected", len(occs) == 1 and occs[0].kind == "sse-decode", f"got {occs!r}")


# --------------------------------------------------------------------------
# test-scope exclusion


def test_scan_file_excludes_cfg_test_mod_block() -> None:
    text = (
        "fn real() { let _ = reqwest::Client::new(); }\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        '    fn route() -> &\'static str { "/api/v1/nous" }\n'
        "}\n"
    )
    occs = CHECK.scan_file("fixture.rs", text)
    expect(
        "only the non-test reqwest use is a candidate",
        len(occs) == 1 and occs[0].kind == "reqwest",
        f"got {occs!r}",
    )


def test_scan_file_semicolon_mod_declaration_does_not_swallow_unrelated_code() -> None:
    # WHY: `#[cfg(test)] mod app_tests;` (an external test file declared,
    # not inlined) has no brace body. Searching past the semicolon for the
    # next `{` would land on the following item's block and wrongly
    # exclude everything up to its close -- this is the real shape of
    # crates/theatron/koilon/src/app/mod.rs.
    text = (
        "#[cfg(test)]\n"
        "mod app_tests;\n"
        "\n"
        "fn real() { let _ = reqwest::Client::new(); }\n"
    )
    occs = CHECK.scan_file("fixture.rs", text)
    expect(
        "the real function after a semicolon-form test mod is still scanned",
        len(occs) == 1 and occs[0].kind == "reqwest",
        f"got {occs!r}",
    )


def test_scan_file_excludes_mod_tests_behind_stacked_multiline_attrs() -> None:
    # WHY: the real shape in crates/theatron/koilon/src/config.rs -- multiple
    # stacked attributes, one of them multi-line, between `#[cfg(test)]` and
    # `mod tests {`. The attribute-skipping loop must walk past all of them.
    text = (
        "fn real() { let _ = reqwest::Client::new(); }\n"
        "#[cfg(test)]\n"
        '#[expect(clippy::unwrap_used, reason = "test")]\n'
        "#[expect(\n"
        "    clippy::disallowed_methods,\n"
        '    reason = "tests seed config"\n'
        ")]\n"
        "mod tests {\n"
        '    fn t() { let _ = reqwest::Client::new(); let u = "{base}/api/v1/nous"; }\n'
        "}\n"
    )
    occs = CHECK.scan_file("fixture.rs", text)
    expect(
        "only the pre-attribute-stack function is a candidate",
        len(occs) == 1 and occs[0].line == 1,
        f"got {occs!r}",
    )


def test_scan_file_excludes_tokio_test_function() -> None:
    text = (
        "#[tokio::test]\n"
        "async fn t() {\n"
        '    let url = "{base}/api/v1/nous";\n'
        "}\n"
    )
    occs = CHECK.scan_file("fixture.rs", text)
    expect("tokio::test-attributed function is excluded", occs == [], f"got {occs!r}")


def test_scan_file_excludes_test_suffixed_file() -> None:
    text = 'fn f() { let _ = reqwest::Client::new(); }\n'
    occs = CHECK.scan_file("crates/foo/src/app_tests.rs", text)
    expect("whole *_tests.rs file excluded", occs == [], f"got {occs!r}")


def test_is_test_file_matches_conventions() -> None:
    expect("tests/ dir excluded", CHECK.is_test_file("crates/foo/tests/common/mod.rs"), "")
    expect("_tests.rs suffix excluded", CHECK.is_test_file("crates/foo/src/foo_tests.rs"), "")
    expect("ordinary src file included", not CHECK.is_test_file("crates/foo/src/lib.rs"), "")


# --------------------------------------------------------------------------
# compare / format_report: the four required ratchet behaviors


def test_unchanged_passes() -> None:
    actual = {"a.rs": 3, "b.rs": 2}
    baseline = {"a.rs": 3, "b.rs": 2}
    regressions, improvements = CHECK.compare(actual, baseline)
    expect("no regressions", regressions == [], f"got {regressions!r}")
    expect("no improvements", improvements == [], f"got {improvements!r}")
    code, lines = CHECK.format_report(actual, baseline, {})
    expect("exit code 0", code == 0, f"got {code}")


def test_growth_fails() -> None:
    actual = {"a.rs": 4, "b.rs": 2}
    baseline = {"a.rs": 3, "b.rs": 2}
    regressions, improvements = CHECK.compare(actual, baseline)
    expect("growth is a regression", regressions == [("a.rs", 4, 3)], f"got {regressions!r}")
    expect("growth alone is not an improvement", improvements == [], f"got {improvements!r}")
    code, lines = CHECK.format_report(actual, baseline, {})
    expect("exit code 1", code == 1, f"got {code}")
    expect(
        "message names the regressed file and both counts",
        any("a.rs: 4, baseline 3" in line for line in lines),
        "\n".join(lines),
    )


def test_shrink_passes_and_prints_ratchet_delta() -> None:
    actual = {"a.rs": 1, "b.rs": 2}
    baseline = {"a.rs": 3, "b.rs": 2}
    regressions, improvements = CHECK.compare(actual, baseline)
    expect("shrink is not a regression", regressions == [], f"got {regressions!r}")
    expect("shrink is an improvement", improvements == [("a.rs", 1, 3)], f"got {improvements!r}")
    code, lines = CHECK.format_report(actual, baseline, {})
    expect("exit code 0 (shrink passes)", code == 0, f"got {code}")
    expect(
        "ratchet delta is printed",
        any("ratchet delta" in line.lower() for line in lines),
        "\n".join(lines),
    )
    expect(
        "delta names the paid-down amount",
        any("2 paid down" in line for line in lines),
        "\n".join(lines),
    )


def test_shrink_to_zero_is_an_improvement_not_a_failure() -> None:
    # A file whose last occurrence was fixed drops out of `actual` entirely.
    actual = {"b.rs": 2}
    baseline = {"a.rs": 3, "b.rs": 2}
    regressions, improvements = CHECK.compare(actual, baseline)
    expect("no regressions", regressions == [], f"got {regressions!r}")
    expect("file gone from the tree registers as fully paid down", improvements == [("a.rs", 0, 3)], f"got {improvements!r}")


def test_new_file_fails() -> None:
    actual = {"a.rs": 3, "b.rs": 2, "c.rs": 1}
    baseline = {"a.rs": 3, "b.rs": 2}
    regressions, improvements = CHECK.compare(actual, baseline)
    expect("new file with occurrences is a regression", regressions == [("c.rs", 1, 0)], f"got {regressions!r}")
    code, lines = CHECK.format_report(actual, baseline, {})
    expect("exit code 1", code == 1, f"got {code}")
    expect(
        "message calls out the new file",
        any("c.rs: 1 new" in line for line in lines),
        "\n".join(lines),
    )


def main() -> int:
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    for t in tests:
        t()

    if FAILURES:
        for f in FAILURES:
            print(f"FAIL: {f}", file=sys.stderr)
        print(f"\n{len(FAILURES)} failure(s) across {len(tests)} test functions", file=sys.stderr)
        return 1

    print(f"OK: {len(tests)} test functions passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
