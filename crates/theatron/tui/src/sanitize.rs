//! Input sanitization for terminal-rendered content.

use std::borrow::Cow;

/// Sanitize external text for safe terminal display.
///
/// Strips all terminal escape sequences (CSI, OSC, DCS, APC, SOS, PM)
/// and replaces dangerous C0/C1 control characters with safe alternatives.
/// Returns `Cow::Borrowed` when the input requires no modification.
pub fn sanitize_for_display(s: &str) -> Cow<'_, str> {
    if !needs_sanitization(s) {
        return Cow::Borrowed(s);
    }

    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // NOTE: 7-bit ESC introducer: 0x1B
        if b == 0x1B && i + 1 < len {
            let next = bytes[i + 1];
            match next {
                // NOTE: CSI: ESC [
                b'[' => {
                    i = skip_csi(bytes, i + 2);
                    continue;
                }
                // NOTE: OSC: ESC ]
                b']' => {
                    i = skip_osc(bytes, i + 2);
                    continue;
                }
                // NOTE: DCS: ESC P
                b'P' => {
                    i = skip_until_st(bytes, i + 2);
                    continue;
                }
                // NOTE: APC: ESC _
                b'_' => {
                    i = skip_until_st(bytes, i + 2);
                    continue;
                }
                // NOTE: SOS: ESC X
                b'X' => {
                    i = skip_until_st(bytes, i + 2);
                    continue;
                }
                // NOTE: PM: ESC ^
                b'^' => {
                    i = skip_until_st(bytes, i + 2);
                    continue;
                }
                // NOTE: ESC (: character set designation, e.g., ESC(B for ASCII
                b'(' | b')' | b'*' | b'+' if i + 2 < len => {
                    i += 3;
                    continue;
                }
                // NOTE: two-byte ESC sequences such as ESC =, ESC >, ESC 7
                0x20..=0x7E => {
                    i += 2;
                    continue;
                }
                _ => {
                    // NOTE: unrecognized ESC sequence: skip the ESC byte
                    i += 1;
                    continue;
                }
            }
        }

        // NOTE: 8-bit C1 control characters (0x80–0x9F)
        if (0x80..=0x9F).contains(&b) {
            i += 1;
            continue;
        }

        // NOTE: C0 control characters (0x00–0x1F)
        if b < 0x20 {
            match b {
                b'\n' | b'\r' | b'\t' => {
                    out.push(b as char);
                }
                _ => {
                    out.push(control_picture(b));
                }
            }
            i += 1;
            continue;
        }

        // NOTE: DEL: 0x7F
        if b == 0x7F {
            out.push('\u{2421}');
            i += 1;
            continue;
        }

        if let Some((ch, char_len)) = decode_utf8_char(bytes, i) {
            // NOTE: Unicode C1 control characters (U+0080–U+009F): handle as named sequences
            if ('\u{0080}'..='\u{009F}').contains(&ch) {
                match ch {
                    // NOTE: 8-bit CSI: U+009B
                    '\u{009B}' => {
                        i = skip_csi(bytes, i + char_len);
                        continue;
                    }
                    // NOTE: 8-bit OSC: U+009D
                    '\u{009D}' => {
                        i = skip_osc(bytes, i + char_len);
                        continue;
                    }
                    // NOTE: 8-bit DCS: U+0090
                    '\u{0090}' => {
                        i = skip_until_st(bytes, i + char_len);
                        continue;
                    }
                    // NOTE: 8-bit APC: U+009F
                    '\u{009F}' => {
                        i = skip_until_st(bytes, i + char_len);
                        continue;
                    }
                    // NOTE: 8-bit SOS: U+0098
                    '\u{0098}' => {
                        i = skip_until_st(bytes, i + char_len);
                        continue;
                    }
                    // NOTE: 8-bit PM: U+009E
                    '\u{009E}' => {
                        i = skip_until_st(bytes, i + char_len);
                        continue;
                    }
                    // NOTE: 8-bit ST: U+009C, drop
                    '\u{009C}' => {
                        i += char_len;
                        continue;
                    }
                    _ => {
                        i += char_len;
                        continue;
                    }
                }
            }
            out.push(ch);
            i += char_len;
        } else {
            // NOTE: invalid UTF-8 byte: skip
            i += 1;
        }
    }

    Cow::Owned(out)
}

/// Quick check whether the string contains any bytes that need sanitization.
fn needs_sanitization(s: &str) -> bool {
    for &b in s.as_bytes() {
        match b {
            // NOTE: replace C0 controls (except HT/LF/CR) and DEL with control pictures
            0x00..=0x08 | 0x0B..=0x0C | 0x0E..=0x1F | 0x7F => return true,
            _ => {
                // NOTE: printable ASCII byte, no sanitization needed
            }
        }
    }
    for ch in s.chars() {
        if ('\u{0080}'..='\u{009F}').contains(&ch) {
            return true;
        }
    }
    false
}

/// Skip a CSI sequence: parameters (0x30-0x3F), intermediates (0x20-0x2F), final byte (0x40-0x7E).
fn skip_csi(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let len = bytes.len();
    while i < len && (0x30..=0x3F).contains(&bytes[i]) {
        i += 1;
    }
    while i < len && (0x20..=0x2F).contains(&bytes[i]) {
        i += 1;
    }
    if i < len && (0x40..=0x7E).contains(&bytes[i]) {
        i += 1;
    }
    i
}

/// Skip an OSC sequence terminated by BEL (0x07) or ST (ESC \ or 0x9C).
fn skip_osc(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let len = bytes.len();
    while i < len {
        if bytes[i] == 0x07 {
            return i + 1;
        }
        if bytes[i] == 0x1B && i + 1 < len && bytes[i + 1] == b'\\' {
            return i + 2;
        }
        // NOTE: 8-bit ST as UTF-8 (U+009C = 0xC2 0x9C)
        if bytes[i] == 0xC2 && i + 1 < len && bytes[i + 1] == 0x9C {
            return i + 2;
        }
        i += 1;
    }
    // NOTE: unterminated sequence: consume to end of input
    len
}

/// Skip until ST (ESC \ or 8-bit ST). Used for DCS, APC, SOS, PM.
fn skip_until_st(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let len = bytes.len();
    while i < len {
        if bytes[i] == 0x1B && i + 1 < len && bytes[i + 1] == b'\\' {
            return i + 2;
        }
        // NOTE: 8-bit ST as UTF-8 (U+009C = 0xC2 0x9C)
        if bytes[i] == 0xC2 && i + 1 < len && bytes[i + 1] == 0x9C {
            return i + 2;
        }
        i += 1;
    }
    // NOTE: unterminated sequence: consume to end of input
    len
}

/// Map a C0 control byte to its Unicode control picture character (U+2400 block).
fn control_picture(byte: u8) -> char {
    match byte {
        0x00 => '\u{2400}',
        0x01 => '\u{2401}',
        0x02 => '\u{2402}',
        0x03 => '\u{2403}',
        0x04 => '\u{2404}',
        0x05 => '\u{2405}',
        0x06 => '\u{2406}',
        0x07 => '\u{2407}',
        0x08 => '\u{2408}',
        // NOTE: 0x09 TAB, 0x0A LF, 0x0D CR are safe and handled earlier
        0x0B => '\u{240B}',
        0x0C => '\u{240C}',
        0x0E => '\u{240E}',
        0x0F => '\u{240F}',
        0x10 => '\u{2410}',
        0x11 => '\u{2411}',
        0x12 => '\u{2412}',
        0x13 => '\u{2413}',
        0x14 => '\u{2414}',
        0x15 => '\u{2415}',
        0x16 => '\u{2416}',
        0x17 => '\u{2417}',
        0x18 => '\u{2418}',
        0x19 => '\u{2419}',
        0x1A => '\u{241A}',
        0x1B => '\u{241B}',
        0x1C => '\u{241C}',
        0x1D => '\u{241D}',
        0x1E => '\u{241E}',
        0x1F => '\u{241F}',
        _ => '\u{FFFD}',
    }
}

/// Decode a single UTF-8 character from a byte slice, returning the char and its byte length.
fn decode_utf8_char(bytes: &[u8], start: usize) -> Option<(char, usize)> {
    let remaining = &bytes[start..];
    let s = std::str::from_utf8(remaining).ok()?;
    let ch = s.chars().next()?;
    Some((ch, ch.len_utf8()))
}

/// Legacy alias: prefer `sanitize_for_display` for new code.
#[cfg(test)]
fn strip_ansi(s: &str) -> Cow<'_, str> {
    sanitize_for_display(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- CSI sequences ---

    #[test]
    fn strip_csi_color_codes() {
        assert_eq!(sanitize_for_display("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn strip_csi_complex_sgr() {
        let input = "\x1b[1;31;42mbold red on green\x1b[0m normal";
        assert_eq!(sanitize_for_display(input), "bold red on green normal");
    }

    #[test]
    fn strip_csi_cursor_movement() {
        // CUU (cursor up), CUD (cursor down), CUF (forward), CUB (back)
        assert_eq!(
            sanitize_for_display("a\x1b[2Ab\x1b[3Bc\x1b[4Cd\x1b[5De"),
            "abcde"
        );
    }

    #[test]
    fn strip_csi_erase() {
        // ED (erase display) and EL (erase line)
        assert_eq!(sanitize_for_display("text\x1b[2J\x1b[K"), "text");
    }

    // --- OSC sequences ---

    #[test]
    fn strip_osc_title_bel() {
        let input = "\x1b]0;evil title\x07visible text";
        assert_eq!(sanitize_for_display(input), "visible text");
    }

    #[test]
    fn strip_osc_title_st() {
        let input = "\x1b]2;evil title\x1b\\visible text";
        assert_eq!(sanitize_for_display(input), "visible text");
    }

    #[test]
    fn strip_osc_clipboard_injection() {
        // OSC 52: clipboard write
        let input = "\x1b]52;c;SGVsbG8gV29ybGQ=\x07safe content";
        assert_eq!(sanitize_for_display(input), "safe content");
    }

    #[test]
    fn strip_osc_clipboard_st_terminated() {
        let input = "\x1b]52;c;SGVsbG8=\x1b\\safe";
        assert_eq!(sanitize_for_display(input), "safe");
    }

    // --- DCS sequences ---

    #[test]
    fn strip_dcs_sequence() {
        let input = "before\x1bPsomething\x1b\\after";
        assert_eq!(sanitize_for_display(input), "beforeafter");
    }

    #[test]
    fn strip_dcs_sixel() {
        // Sixel graphics: DCS q ... ST
        let input = "\x1bPq#0;2;0;0;0#0!10~\x1b\\text";
        assert_eq!(sanitize_for_display(input), "text");
    }

    // --- APC sequences ---

    #[test]
    fn strip_apc_sequence() {
        let input = "before\x1b_application data\x1b\\after";
        assert_eq!(sanitize_for_display(input), "beforeafter");
    }

    // --- SOS sequences ---

    #[test]
    fn strip_sos_sequence() {
        let input = "before\x1bXstring data\x1b\\after";
        assert_eq!(sanitize_for_display(input), "beforeafter");
    }

    // --- PM sequences ---

    #[test]
    fn strip_pm_sequence() {
        let input = "before\x1b^privacy message\x1b\\after";
        assert_eq!(sanitize_for_display(input), "beforeafter");
    }

    // --- 8-bit C1 controls (UTF-8 encoded) ---

    #[test]
    fn strip_8bit_csi() {
        // U+009B (8-bit CSI) encoded as UTF-8: 0xC2 0x9B
        let input = format!("text{}31mred", '\u{009B}');
        assert_eq!(sanitize_for_display(&input), "textred");
    }

    #[test]
    fn strip_8bit_osc() {
        // U+009D (8-bit OSC) encoded as UTF-8
        let input = format!("text{}0;title\x07visible", '\u{009D}');
        assert_eq!(sanitize_for_display(&input), "textvisible");
    }

    #[test]
    fn strip_8bit_dcs() {
        // U+0090 (8-bit DCS) encoded as UTF-8
        let input = format!("text{}data\x1b\\visible", '\u{0090}');
        assert_eq!(sanitize_for_display(&input), "textvisible");
    }

    #[test]
    fn strip_c1_controls_silently() {
        // Various C1 controls that should be dropped
        let input = format!("a{}b{}c", '\u{0085}', '\u{008A}');
        assert_eq!(sanitize_for_display(&input), "abc");
    }

    // --- C0 control characters ---

    #[test]
    fn replace_null_with_picture() {
        assert_eq!(sanitize_for_display("a\x00b"), "a\u{2400}b");
    }

    #[test]
    fn replace_bel_with_picture() {
        assert_eq!(sanitize_for_display("a\x07b"), "a\u{2407}b");
    }

    #[test]
    fn replace_backspace_with_picture() {
        assert_eq!(sanitize_for_display("a\x08b"), "a\u{2408}b");
    }

    #[test]
    fn replace_del_with_picture() {
        assert_eq!(sanitize_for_display("a\x7Fb"), "a\u{2421}b");
    }

    #[test]
    fn preserve_safe_controls() {
        // Tab, newline, carriage return should pass through
        assert_eq!(
            sanitize_for_display("line1\nline2\ttab\rreturn"),
            "line1\nline2\ttab\rreturn"
        );
    }

    // --- Character set designation ---

    #[test]
    fn strip_charset_designation() {
        assert_eq!(sanitize_for_display("\x1b(Btext"), "text");
        assert_eq!(sanitize_for_display("\x1b)0text"), "text");
    }

    // --- Clean text passthrough ---

    #[test]
    fn clean_ascii_passthrough() {
        let clean = "no escapes here";
        let result = sanitize_for_display(clean);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(&*result, clean);
    }

    #[test]
    fn clean_unicode_passthrough() {
        let clean = "hello \u{1F600} world \u{00E9}\u{00E8}\u{00EA}";
        let result = sanitize_for_display(clean);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(&*result, clean);
    }

    #[test]
    fn clean_cjk_passthrough() {
        let clean = "\u{4F60}\u{597D}\u{4E16}\u{754C}";
        let result = sanitize_for_display(clean);
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    // --- Mixed and complex cases ---

    #[test]
    fn mixed_sequences_in_one_string() {
        let input = "\x1b[31mred\x1b[0m \x1b]0;title\x07 \x1bPdcs\x1b\\ visible";
        assert_eq!(sanitize_for_display(input), "red   visible");
    }

    #[test]
    fn unicode_text_with_embedded_escapes() {
        let input = "caf\u{00E9}\x1b[1mbold\x1b[0m \u{1F600}";
        assert_eq!(sanitize_for_display(input), "caf\u{00E9}bold \u{1F600}");
    }

    #[test]
    fn nested_malformed_sequences() {
        // ESC [ with no final byte, then normal text
        let input = "\x1b[text after incomplete csi";
        let result = sanitize_for_display(input);
        // The skip_csi consumes parameter/intermediate bytes but 't' is a valid final byte
        // so it will consume \x1b[ then 't' as final byte, then "ext after..."
        assert!(result.contains("after"));
    }

    #[test]
    fn unterminated_osc_consumed_to_end() {
        let input = "before\x1b]0;no terminator";
        assert_eq!(sanitize_for_display(input), "before");
    }

    #[test]
    fn unterminated_dcs_consumed_to_end() {
        let input = "before\x1bPno terminator";
        assert_eq!(sanitize_for_display(input), "before");
    }

    #[test]
    fn multiple_escape_types_interleaved() {
        let input = "\x1b[31m\x1b]52;c;dGVzdA==\x07\x1bPdcs\x1b\\\x1b_apc\x1b\\safe";
        assert_eq!(sanitize_for_display(input), "safe");
    }

    #[test]
    fn real_world_llm_response_with_ansi() {
        let input = "Here is the \x1b[1;36mresult\x1b[0m of the analysis:\n\
                      - Item 1\n\
                      - Item 2\n\
                      \x1b]0;hijack\x07";
        let result = sanitize_for_display(input);
        assert!(result.contains("result"));
        assert!(result.contains("Item 1"));
        assert!(!result.contains("hijack"));
        assert!(!result.contains("\x1b"));
    }

    #[test]
    fn strip_ansi_backward_compat() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    // --- Edge cases ---

    #[test]
    fn empty_string() {
        let result = sanitize_for_display("");
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(&*result, "");
    }

    #[test]
    fn bare_esc_at_end() {
        let result = sanitize_for_display("text\x1b");
        // The bare ESC at end without a follow-up byte is just skipped
        assert!(!result.contains('\x1b'));
    }

    #[test]
    fn only_escape_sequences() {
        let input = "\x1b[31m\x1b[0m\x1b]0;x\x07";
        assert_eq!(sanitize_for_display(input), "");
    }

    #[test]
    fn cursor_repositioning_attack() {
        // Attempt to overwrite visible content using cursor movement
        let input = "safe text\x1b[H\x1b[2Jmalicious overwrite";
        let result = sanitize_for_display(input);
        assert!(result.contains("safe text"));
        assert!(result.contains("malicious overwrite"));
        assert!(!result.contains("\x1b"));
    }

    #[test]
    fn key_remapping_attack() {
        // DCS sequence attempting to redefine keys
        let input = "\x1bP+q4B\x1b\\visible";
        assert_eq!(sanitize_for_display(input), "visible");
    }

    // --- Additional edge cases ---

    #[test]
    fn safe_whitespace_only_string_is_borrowed() {
        // \t, \n, \r do not trigger sanitization — result must be Cow::Borrowed
        let input = "\thello\nworld\r";
        let result = sanitize_for_display(input);
        assert!(
            matches!(result, Cow::Borrowed(_)),
            "expected Borrowed for safe whitespace input"
        );
        assert_eq!(&*result, input);
    }

    #[test]
    fn osc_terminated_by_8bit_st_utf8() {
        // U+009C (ST) encoded as UTF-8 0xC2 0x9C terminates the OSC
        let input = "\x1b]0;title\u{009C}visible".to_string();
        assert_eq!(sanitize_for_display(&input), "visible");
    }

    #[test]
    fn esc_charset_star_designation_stripped() {
        // ESC * <designator>: 3-byte charset designation sequence
        let input = "\x1b*Btext";
        assert_eq!(sanitize_for_display(input), "text");
    }

    #[test]
    fn esc_charset_plus_designation_stripped() {
        // ESC + <designator>: 3-byte charset designation sequence
        let input = "\x1b+0text";
        assert_eq!(sanitize_for_display(input), "text");
    }

    #[test]
    fn csi_with_private_parameter_stripped() {
        // ESC [ ? 2 5 l: hide cursor (CSI with DEC-private '?' parameter byte)
        assert_eq!(sanitize_for_display("\x1b[?25ltext"), "text");
    }

    #[test]
    fn csi_hide_cursor_does_not_appear_in_output() {
        // ESC [ ? 2 5 h: show cursor — only "text" should survive
        let result = sanitize_for_display("before\x1b[?25hafter");
        assert_eq!(result, "beforeafter", "cursor-show CSI must be stripped");
    }

    #[test]
    fn esc_two_byte_equals_sign_stripped() {
        // ESC = (application keypad mode): 2-byte sequence, '=' is in 0x20..=0x7E
        assert_eq!(sanitize_for_display("\x1b=text"), "text");
    }

    #[test]
    fn double_esc_then_csi_both_stripped() {
        // First ESC is followed by second ESC (0x1B is not in 0x20..=0x7E and not a
        // recognised introducer), so the first ESC is dropped and then the second
        // ESC starts a normal CSI sequence.
        let input = "\x1b\x1b[0mtext";
        let result = sanitize_for_display(input);
        assert_eq!(result, "text", "both ESC and ESC CSI must be stripped");
    }
}
