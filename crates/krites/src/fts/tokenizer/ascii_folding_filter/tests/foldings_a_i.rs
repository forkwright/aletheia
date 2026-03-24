//! ASCII folding tests for letters A through I.

use super::folding_using_raw_tokenizer_helper;

#[test]
fn test_all_foldings_a_through_i() {
    let foldings: Vec<(&[&str], &str)> = vec![
        (
            &[
                "À",  // U+00C0: LATIN CAPITAL LETTER A WITH GRAVE
                "Á",  // U+00C1: LATIN CAPITAL LETTER A WITH ACUTE
                "Â",  // U+00C2: LATIN CAPITAL LETTER A WITH CIRCUMFLEX
                "Ã",  // U+00C3: LATIN CAPITAL LETTER A WITH TILDE
                "Ä",  // U+00C4: LATIN CAPITAL LETTER A WITH DIAERESIS
                "Å",  // U+00C5: LATIN CAPITAL LETTER A WITH RING ABOVE
                "Ā",  // U+0100: LATIN CAPITAL LETTER A WITH MACRON
                "Ă",  // U+0102: LATIN CAPITAL LETTER A WITH BREVE
                "Ą",  // U+0104: LATIN CAPITAL LETTER A WITH OGONEK
                "Ə",  // U+018F: LATIN CAPITAL LETTER SCHWA
                "Ǎ",  // U+01CD: LATIN CAPITAL LETTER A WITH CARON
                "Ǟ",  // U+01DE: LATIN CAPITAL LETTER A WITH DIAERESIS AND MACRON
                "Ǡ",  // U+01E0: LATIN CAPITAL LETTER A WITH DOT ABOVE AND MACRON
                "Ǻ",  // U+01FA: LATIN CAPITAL LETTER A WITH RING ABOVE AND ACUTE
                "Ȁ",  // U+0200: LATIN CAPITAL LETTER A WITH DOUBLE GRAVE
                "Ȃ",  // U+0202: LATIN CAPITAL LETTER A WITH INVERTED BREVE
                "Ȧ",  // U+0226: LATIN CAPITAL LETTER A WITH DOT ABOVE
                "Ⱥ",  // U+023A: LATIN CAPITAL LETTER A WITH STROKE
                "ᴀ",  // U+1D00: LATIN LETTER SMALL CAPITAL A
                "Ḁ",  // U+1E00: LATIN CAPITAL LETTER A WITH RING BELOW
                "Ạ",  // U+1EA0: LATIN CAPITAL LETTER A WITH DOT BELOW
                "Ả",  // U+1EA2: LATIN CAPITAL LETTER A WITH HOOK ABOVE
                "Ấ",  // U+1EA4: LATIN CAPITAL LETTER A WITH CIRCUMFLEX AND ACUTE
                "Ầ",  // U+1EA6: LATIN CAPITAL LETTER A WITH CIRCUMFLEX AND GRAVE
                "Ẩ",  // U+1EA8: LATIN CAPITAL LETTER A WITH CIRCUMFLEX AND HOOK ABOVE
                "Ẫ",  // U+1EAA: LATIN CAPITAL LETTER A WITH CIRCUMFLEX AND TILDE
                "Ậ",  // U+1EAC: LATIN CAPITAL LETTER A WITH CIRCUMFLEX AND DOT BELOW
                "Ắ",  // U+1EAE: LATIN CAPITAL LETTER A WITH BREVE AND ACUTE
                "Ằ",  // U+1EB0: LATIN CAPITAL LETTER A WITH BREVE AND GRAVE
                "Ẳ",  // U+1EB2: LATIN CAPITAL LETTER A WITH BREVE AND HOOK ABOVE
                "Ẵ",  // U+1EB4: LATIN CAPITAL LETTER A WITH BREVE AND TILDE
                "Ặ",  // U+1EB6: LATIN CAPITAL LETTER A WITH BREVE AND DOT BELOW
                "Ⓐ",  // U+24B6: CIRCLED LATIN CAPITAL LETTER A
                "Ａ", // U+FF21: FULLWIDTH LATIN CAPITAL LETTER A
            ],
            "A",
        ),
        (
            &[
                "à",  // U+00E0: LATIN SMALL LETTER A WITH GRAVE
                "á",  // U+00E1: LATIN SMALL LETTER A WITH ACUTE
                "â",  // U+00E2: LATIN SMALL LETTER A WITH CIRCUMFLEX
                "ã",  // U+00E3: LATIN SMALL LETTER A WITH TILDE
                "ä",  // U+00E4: LATIN SMALL LETTER A WITH DIAERESIS
                "å",  // U+00E5: LATIN SMALL LETTER A WITH RING ABOVE
                "ā",  // U+0101: LATIN SMALL LETTER A WITH MACRON
                "ă",  // U+0103: LATIN SMALL LETTER A WITH BREVE
                "ą",  // U+0105: LATIN SMALL LETTER A WITH OGONEK
                "ǎ",  // U+01CE: LATIN SMALL LETTER A WITH CARON
                "ǟ",  // U+01DF: LATIN SMALL LETTER A WITH DIAERESIS AND MACRON
                "ǡ",  // U+01E1: LATIN SMALL LETTER A WITH DOT ABOVE AND MACRON
                "ǻ",  // U+01FB: LATIN SMALL LETTER A WITH RING ABOVE AND ACUTE
                "ȁ",  // U+0201: LATIN SMALL LETTER A WITH DOUBLE GRAVE
                "ȃ",  // U+0203: LATIN SMALL LETTER A WITH INVERTED BREVE
                "ȧ",  // U+0227: LATIN SMALL LETTER A WITH DOT ABOVE
                "ɐ",  // U+0250: LATIN SMALL LETTER TURNED A
                "ə",  // U+0259: LATIN SMALL LETTER SCHWA
                "ɚ",  // U+025A: LATIN SMALL LETTER SCHWA WITH HOOK
                "ᶏ",  // U+1D8F: LATIN SMALL LETTER A WITH RETROFLEX HOOK
                "ḁ",  // U+1E01: LATIN SMALL LETTER A WITH RING BELOW
                "ᶕ",  // U+1D95: LATIN SMALL LETTER SCHWA WITH RETROFLEX HOOK
                "ẚ",  // U+1E9A: LATIN SMALL LETTER A WITH RIGHT HALF RING
                "ạ",  // U+1EA1: LATIN SMALL LETTER A WITH DOT BELOW
                "ả",  // U+1EA3: LATIN SMALL LETTER A WITH HOOK ABOVE
                "ấ",  // U+1EA5: LATIN SMALL LETTER A WITH CIRCUMFLEX AND ACUTE
                "ầ",  // U+1EA7: LATIN SMALL LETTER A WITH CIRCUMFLEX AND GRAVE
                "ẩ",  // U+1EA9: LATIN SMALL LETTER A WITH CIRCUMFLEX AND HOOK ABOVE
                "ẫ",  // U+1EAB: LATIN SMALL LETTER A WITH CIRCUMFLEX AND TILDE
                "ậ",  // U+1EAD: LATIN SMALL LETTER A WITH CIRCUMFLEX AND DOT BELOW
                "ắ",  // U+1EAF: LATIN SMALL LETTER A WITH BREVE AND ACUTE
                "ằ",  // U+1EB1: LATIN SMALL LETTER A WITH BREVE AND GRAVE
                "ẳ",  // U+1EB3: LATIN SMALL LETTER A WITH BREVE AND HOOK ABOVE
                "ẵ",  // U+1EB5: LATIN SMALL LETTER A WITH BREVE AND TILDE
                "ặ",  // U+1EB7: LATIN SMALL LETTER A WITH BREVE AND DOT BELOW
                "ₐ",  // U+2090: LATIN SUBSCRIPT SMALL LETTER A
                "ₔ",  // U+2094: LATIN SUBSCRIPT SMALL LETTER SCHWA
                "ⓐ",  // U+24D0: CIRCLED LATIN SMALL LETTER A
                "ⱥ",  // U+2C65: LATIN SMALL LETTER A WITH STROKE
                "Ɐ",  // U+2C6F: LATIN CAPITAL LETTER TURNED A
                "ａ", // U+FF41: FULLWIDTH LATIN SMALL LETTER A
            ],
            "a",
        ),
        (
            &[
                "Ꜳ", // U+A732: LATIN CAPITAL LETTER AA
            ],
            "AA",
        ),
        (
            &[
                "Æ", // U+00C6: LATIN CAPITAL LETTER AE
                "Ǣ", // U+01E2: LATIN CAPITAL LETTER AE WITH MACRON
                "Ǽ", // U+01FC: LATIN CAPITAL LETTER AE WITH ACUTE
                "ᴁ", // U+1D01: LATIN LETTER SMALL CAPITAL AE
            ],
            "AE",
        ),
        (
            &[
                "Ꜵ", // U+A734: LATIN CAPITAL LETTER AO
            ],
            "AO",
        ),
        (
            &[
                "Ꜷ", // U+A736: LATIN CAPITAL LETTER AU
            ],
            "AU",
        ),
        (
            &[
                "Ꜹ", // U+A738: LATIN CAPITAL LETTER AV
                "Ꜻ", // U+A73A: LATIN CAPITAL LETTER AV WITH HORIZONTAL BAR
            ],
            "AV",
        ),
        (
            &[
                "Ꜽ", // U+A73C: LATIN CAPITAL LETTER AY
            ],
            "AY",
        ),
        (
            &[
                "⒜", // U+249C: PARENTHESIZED LATIN SMALL LETTER A
            ],
            "(a)",
        ),
        (
            &[
                "ꜳ", // U+A733: LATIN SMALL LETTER AA
            ],
            "aa",
        ),
        (
            &[
                "æ", // U+00E6: LATIN SMALL LETTER AE
                "ǣ", // U+01E3: LATIN SMALL LETTER AE WITH MACRON
                "ǽ", // U+01FD: LATIN SMALL LETTER AE WITH ACUTE
                "ᴂ", // U+1D02: LATIN SMALL LETTER TURNED AE
            ],
            "ae",
        ),
        (
            &[
                "ꜵ", // U+A735: LATIN SMALL LETTER AO
            ],
            "ao",
        ),
        (
            &[
                "ꜷ", // U+A737: LATIN SMALL LETTER AU
            ],
            "au",
        ),
        (
            &[
                "ꜹ", // U+A739: LATIN SMALL LETTER AV
                "ꜻ", // U+A73B: LATIN SMALL LETTER AV WITH HORIZONTAL BAR
            ],
            "av",
        ),
        (
            &[
                "ꜽ", // U+A73D: LATIN SMALL LETTER AY
            ],
            "ay",
        ),
        (
            &[
                "Ɓ",  // U+0181: LATIN CAPITAL LETTER B WITH HOOK
                "Ƃ",  // U+0182: LATIN CAPITAL LETTER B WITH TOPBAR
                "Ƀ",  // U+0243: LATIN CAPITAL LETTER B WITH STROKE
                "ʙ",  // U+0299: LATIN LETTER SMALL CAPITAL B
                "ᴃ",  // U+1D03: LATIN LETTER SMALL CAPITAL BARRED B
                "Ḃ",  // U+1E02: LATIN CAPITAL LETTER B WITH DOT ABOVE
                "Ḅ",  // U+1E04: LATIN CAPITAL LETTER B WITH DOT BELOW
                "Ḇ",  // U+1E06: LATIN CAPITAL LETTER B WITH LINE BELOW
                "Ⓑ",  // U+24B7: CIRCLED LATIN CAPITAL LETTER B
                "Ｂ", // U+FF22: FULLWIDTH LATIN CAPITAL LETTER B
            ],
            "B",
        ),
        (
            &[
                "ƀ",  // U+0180: LATIN SMALL LETTER B WITH STROKE
                "ƃ",  // U+0183: LATIN SMALL LETTER B WITH TOPBAR
                "ɓ",  // U+0253: LATIN SMALL LETTER B WITH HOOK
                "ᵬ",  // U+1D6C: LATIN SMALL LETTER B WITH MIDDLE TILDE
                "ᶀ",  // U+1D80: LATIN SMALL LETTER B WITH PALATAL HOOK
                "ḃ",  // U+1E03: LATIN SMALL LETTER B WITH DOT ABOVE
                "ḅ",  // U+1E05: LATIN SMALL LETTER B WITH DOT BELOW
                "ḇ",  // U+1E07: LATIN SMALL LETTER B WITH LINE BELOW
                "ⓑ",  // U+24D1: CIRCLED LATIN SMALL LETTER B
                "ｂ", // U+FF42: FULLWIDTH LATIN SMALL LETTER B
            ],
            "b",
        ),
        (
            &[
                "⒝", // U+249D: PARENTHESIZED LATIN SMALL LETTER B
            ],
            "(b)",
        ),
        (
            &[
                "Ç",  // U+00C7: LATIN CAPITAL LETTER C WITH CEDILLA
                "Ć",  // U+0106: LATIN CAPITAL LETTER C WITH ACUTE
                "Ĉ",  // U+0108: LATIN CAPITAL LETTER C WITH CIRCUMFLEX
                "Ċ",  // U+010A: LATIN CAPITAL LETTER C WITH DOT ABOVE
                "Č",  // U+010C: LATIN CAPITAL LETTER C WITH CARON
                "Ƈ",  // U+0187: LATIN CAPITAL LETTER C WITH HOOK
                "Ȼ",  // U+023B: LATIN CAPITAL LETTER C WITH STROKE
                "ʗ",  // U+0297: LATIN LETTER STRETCHED C
                "ᴄ",  // U+1D04: LATIN LETTER SMALL CAPITAL C
                "Ḉ",  // U+1E08: LATIN CAPITAL LETTER C WITH CEDILLA AND ACUTE
                "Ⓒ",  // U+24B8: CIRCLED LATIN CAPITAL LETTER C
                "Ｃ", // U+FF23: FULLWIDTH LATIN CAPITAL LETTER C
            ],
            "C",
        ),
        (
            &[
                "ç",  // U+00E7: LATIN SMALL LETTER C WITH CEDILLA
                "ć",  // U+0107: LATIN SMALL LETTER C WITH ACUTE
                "ĉ",  // U+0109: LATIN SMALL LETTER C WITH CIRCUMFLEX
                "ċ",  // U+010B: LATIN SMALL LETTER C WITH DOT ABOVE
                "č",  // U+010D: LATIN SMALL LETTER C WITH CARON
                "ƈ",  // U+0188: LATIN SMALL LETTER C WITH HOOK
                "ȼ",  // U+023C: LATIN SMALL LETTER C WITH STROKE
                "ɕ",  // U+0255: LATIN SMALL LETTER C WITH CURL
                "ḉ",  // U+1E09: LATIN SMALL LETTER C WITH CEDILLA AND ACUTE
                "ↄ",  // U+2184: LATIN SMALL LETTER REVERSED C
                "ⓒ",  // U+24D2: CIRCLED LATIN SMALL LETTER C
                "Ꜿ",  // U+A73E: LATIN CAPITAL LETTER REVERSED C WITH DOT
                "ꜿ",  // U+A73F: LATIN SMALL LETTER REVERSED C WITH DOT
                "ｃ", // U+FF43: FULLWIDTH LATIN SMALL LETTER C
            ],
            "c",
        ),
        (
            &[
                "⒞", // U+249E: PARENTHESIZED LATIN SMALL LETTER C
            ],
            "(c)",
        ),
        (
            &[
                "Ð",  // U+00D0: LATIN CAPITAL LETTER ETH
                "Ď",  // U+010E: LATIN CAPITAL LETTER D WITH CARON
                "Đ",  // U+0110: LATIN CAPITAL LETTER D WITH STROKE
                "Ɖ",  // U+0189: LATIN CAPITAL LETTER AFRICAN D
                "Ɗ",  // U+018A: LATIN CAPITAL LETTER D WITH HOOK
                "Ƌ",  // U+018B: LATIN CAPITAL LETTER D WITH TOPBAR
                "ᴅ",  // U+1D05: LATIN LETTER SMALL CAPITAL D
                "ᴆ",  // U+1D06: LATIN LETTER SMALL CAPITAL ETH
                "Ḋ",  // U+1E0A: LATIN CAPITAL LETTER D WITH DOT ABOVE
                "Ḍ",  // U+1E0C: LATIN CAPITAL LETTER D WITH DOT BELOW
                "Ḏ",  // U+1E0E: LATIN CAPITAL LETTER D WITH LINE BELOW
                "Ḑ",  // U+1E10: LATIN CAPITAL LETTER D WITH CEDILLA
                "Ḓ",  // U+1E12: LATIN CAPITAL LETTER D WITH CIRCUMFLEX BELOW
                "Ⓓ",  // U+24B9: CIRCLED LATIN CAPITAL LETTER D
                "Ꝺ",  // U+A779: LATIN CAPITAL LETTER INSULAR D
                "Ｄ", // U+FF24: FULLWIDTH LATIN CAPITAL LETTER D
            ],
            "D",
        ),
        (
            &[
                "ð",  // U+00F0: LATIN SMALL LETTER ETH
                "ď",  // U+010F: LATIN SMALL LETTER D WITH CARON
                "đ",  // U+0111: LATIN SMALL LETTER D WITH STROKE
                "ƌ",  // U+018C: LATIN SMALL LETTER D WITH TOPBAR
                "ȡ",  // U+0221: LATIN SMALL LETTER D WITH CURL
                "ɖ",  // U+0256: LATIN SMALL LETTER D WITH TAIL
                "ɗ",  // U+0257: LATIN SMALL LETTER D WITH HOOK
                "ᵭ",  // U+1D6D: LATIN SMALL LETTER D WITH MIDDLE TILDE
                "ᶁ",  // U+1D81: LATIN SMALL LETTER D WITH PALATAL HOOK
                "ᶑ",  // U+1D91: LATIN SMALL LETTER D WITH HOOK AND TAIL
                "ḋ",  // U+1E0B: LATIN SMALL LETTER D WITH DOT ABOVE
                "ḍ",  // U+1E0D: LATIN SMALL LETTER D WITH DOT BELOW
                "ḏ",  // U+1E0F: LATIN SMALL LETTER D WITH LINE BELOW
                "ḑ",  // U+1E11: LATIN SMALL LETTER D WITH CEDILLA
                "ḓ",  // U+1E13: LATIN SMALL LETTER D WITH CIRCUMFLEX BELOW
                "ⓓ",  // U+24D3: CIRCLED LATIN SMALL LETTER D
                "ꝺ",  // U+A77A: LATIN SMALL LETTER INSULAR D
                "ｄ", // U+FF44: FULLWIDTH LATIN SMALL LETTER D
            ],
            "d",
        ),
        (
            &[
                "Ǆ", // U+01C4: LATIN CAPITAL LETTER DZ WITH CARON
                "Ǳ", // U+01F1: LATIN CAPITAL LETTER DZ
            ],
            "DZ",
        ),
        (
            &[
                "ǅ", // U+01C5: LATIN CAPITAL LETTER D WITH SMALL LETTER Z WITH CARON
                "ǲ", // U+01F2: LATIN CAPITAL LETTER D WITH SMALL LETTER Z
            ],
            "Dz",
        ),
        (
            &[
                "⒟", // U+249F: PARENTHESIZED LATIN SMALL LETTER D
            ],
            "(d)",
        ),
        (
            &[
                "ȸ", // U+0238: LATIN SMALL LETTER DB DIGRAPH
            ],
            "db",
        ),
        (
            &[
                "ǆ", // U+01C6: LATIN SMALL LETTER DZ WITH CARON
                "ǳ", // U+01F3: LATIN SMALL LETTER DZ
                "ʣ", // U+02A3: LATIN SMALL LETTER DZ DIGRAPH
                "ʥ", // U+02A5: LATIN SMALL LETTER DZ DIGRAPH WITH CURL
            ],
            "dz",
        ),
        (
            &[
                "È",  // U+00C8: LATIN CAPITAL LETTER E WITH GRAVE
                "É",  // U+00C9: LATIN CAPITAL LETTER E WITH ACUTE
                "Ê",  // U+00CA: LATIN CAPITAL LETTER E WITH CIRCUMFLEX
                "Ë",  // U+00CB: LATIN CAPITAL LETTER E WITH DIAERESIS
                "Ē",  // U+0112: LATIN CAPITAL LETTER E WITH MACRON
                "Ĕ",  // U+0114: LATIN CAPITAL LETTER E WITH BREVE
                "Ė",  // U+0116: LATIN CAPITAL LETTER E WITH DOT ABOVE
                "Ę",  // U+0118: LATIN CAPITAL LETTER E WITH OGONEK
                "Ě",  // U+011A: LATIN CAPITAL LETTER E WITH CARON
                "Ǝ",  // U+018E: LATIN CAPITAL LETTER REVERSED E
                "Ɛ",  // U+0190: LATIN CAPITAL LETTER OPEN E
                "Ȅ",  // U+0204: LATIN CAPITAL LETTER E WITH DOUBLE GRAVE
                "Ȇ",  // U+0206: LATIN CAPITAL LETTER E WITH INVERTED BREVE
                "Ȩ",  // U+0228: LATIN CAPITAL LETTER E WITH CEDILLA
                "Ɇ",  // U+0246: LATIN CAPITAL LETTER E WITH STROKE
                "ᴇ",  // U+1D07: LATIN LETTER SMALL CAPITAL E
                "Ḕ",  // U+1E14: LATIN CAPITAL LETTER E WITH MACRON AND GRAVE
                "Ḗ",  // U+1E16: LATIN CAPITAL LETTER E WITH MACRON AND ACUTE
                "Ḙ",  // U+1E18: LATIN CAPITAL LETTER E WITH CIRCUMFLEX BELOW
                "Ḛ",  // U+1E1A: LATIN CAPITAL LETTER E WITH TILDE BELOW
                "Ḝ",  // U+1E1C: LATIN CAPITAL LETTER E WITH CEDILLA AND BREVE
                "Ẹ",  // U+1EB8: LATIN CAPITAL LETTER E WITH DOT BELOW
                "Ẻ",  // U+1EBA: LATIN CAPITAL LETTER E WITH HOOK ABOVE
                "Ẽ",  // U+1EBC: LATIN CAPITAL LETTER E WITH TILDE
                "Ế",  // U+1EBE: LATIN CAPITAL LETTER E WITH CIRCUMFLEX AND ACUTE
                "Ề",  // U+1EC0: LATIN CAPITAL LETTER E WITH CIRCUMFLEX AND GRAVE
                "Ể",  // U+1EC2: LATIN CAPITAL LETTER E WITH CIRCUMFLEX AND HOOK ABOVE
                "Ễ",  // U+1EC4: LATIN CAPITAL LETTER E WITH CIRCUMFLEX AND TILDE
                "Ệ",  // U+1EC6: LATIN CAPITAL LETTER E WITH CIRCUMFLEX AND DOT BELOW
                "Ⓔ",  // U+24BA: CIRCLED LATIN CAPITAL LETTER E
                "ⱻ",  // U+2C7B: LATIN LETTER SMALL CAPITAL TURNED E
                "Ｅ", // U+FF25: FULLWIDTH LATIN CAPITAL LETTER E
            ],
            "E",
        ),
        (
            &[
                "è",  // U+00E8: LATIN SMALL LETTER E WITH GRAVE
                "é",  // U+00E9: LATIN SMALL LETTER E WITH ACUTE
                "ê",  // U+00EA: LATIN SMALL LETTER E WITH CIRCUMFLEX
                "ë",  // U+00EB: LATIN SMALL LETTER E WITH DIAERESIS
                "ē",  // U+0113: LATIN SMALL LETTER E WITH MACRON
                "ĕ",  // U+0115: LATIN SMALL LETTER E WITH BREVE
                "ė",  // U+0117: LATIN SMALL LETTER E WITH DOT ABOVE
                "ę",  // U+0119: LATIN SMALL LETTER E WITH OGONEK
                "ě",  // U+011B: LATIN SMALL LETTER E WITH CARON
                "ǝ",  // U+01DD: LATIN SMALL LETTER TURNED E
                "ȅ",  // U+0205: LATIN SMALL LETTER E WITH DOUBLE GRAVE
                "ȇ",  // U+0207: LATIN SMALL LETTER E WITH INVERTED BREVE
                "ȩ",  // U+0229: LATIN SMALL LETTER E WITH CEDILLA
                "ɇ",  // U+0247: LATIN SMALL LETTER E WITH STROKE
                "ɘ",  // U+0258: LATIN SMALL LETTER REVERSED E
                "ɛ",  // U+025B: LATIN SMALL LETTER OPEN E
                "ɜ",  // U+025C: LATIN SMALL LETTER REVERSED OPEN E
                "ɝ",  // U+025D: LATIN SMALL LETTER REVERSED OPEN E WITH HOOK
                "ɞ",  // U+025E: LATIN SMALL LETTER CLOSED REVERSED OPEN E
                "ʚ",  // U+029A: LATIN SMALL LETTER CLOSED OPEN E
                "ᴈ",  // U+1D08: LATIN SMALL LETTER TURNED OPEN E
                "ᶒ",  // U+1D92: LATIN SMALL LETTER E WITH RETROFLEX HOOK
                "ᶓ",  // U+1D93: LATIN SMALL LETTER OPEN E WITH RETROFLEX HOOK
                "ᶔ",  // U+1D94: LATIN SMALL LETTER REVERSED OPEN E WITH RETROFLEX HOOK
                "ḕ",  // U+1E15: LATIN SMALL LETTER E WITH MACRON AND GRAVE
                "ḗ",  // U+1E17: LATIN SMALL LETTER E WITH MACRON AND ACUTE
                "ḙ",  // U+1E19: LATIN SMALL LETTER E WITH CIRCUMFLEX BELOW
                "ḛ",  // U+1E1B: LATIN SMALL LETTER E WITH TILDE BELOW
                "ḝ",  // U+1E1D: LATIN SMALL LETTER E WITH CEDILLA AND BREVE
                "ẹ",  // U+1EB9: LATIN SMALL LETTER E WITH DOT BELOW
                "ẻ",  // U+1EBB: LATIN SMALL LETTER E WITH HOOK ABOVE
                "ẽ",  // U+1EBD: LATIN SMALL LETTER E WITH TILDE
                "ế",  // U+1EBF: LATIN SMALL LETTER E WITH CIRCUMFLEX AND ACUTE
                "ề",  // U+1EC1: LATIN SMALL LETTER E WITH CIRCUMFLEX AND GRAVE
                "ể",  // U+1EC3: LATIN SMALL LETTER E WITH CIRCUMFLEX AND HOOK ABOVE
                "ễ",  // U+1EC5: LATIN SMALL LETTER E WITH CIRCUMFLEX AND TILDE
                "ệ",  // U+1EC7: LATIN SMALL LETTER E WITH CIRCUMFLEX AND DOT BELOW
                "ₑ",  // U+2091: LATIN SUBSCRIPT SMALL LETTER E
                "ⓔ",  // U+24D4: CIRCLED LATIN SMALL LETTER E
                "ⱸ",  // U+2C78: LATIN SMALL LETTER E WITH NOTCH
                "ｅ", // U+FF45: FULLWIDTH LATIN SMALL LETTER E
            ],
            "e",
        ),
        (
            &[
                "⒠", // U+24A0: PARENTHESIZED LATIN SMALL LETTER E
            ],
            "(e)",
        ),
        (
            &[
                "Ƒ",  // U+0191: LATIN CAPITAL LETTER F WITH HOOK
                "Ḟ",  // U+1E1E: LATIN CAPITAL LETTER F WITH DOT ABOVE
                "Ⓕ",  // U+24BB: CIRCLED LATIN CAPITAL LETTER F
                "ꜰ",  // U+A730: LATIN LETTER SMALL CAPITAL F
                "Ꝼ",  // U+A77B: LATIN CAPITAL LETTER INSULAR F
                "ꟻ",  // U+A7FB: LATIN EPIGRAPHIC LETTER REVERSED F
                "Ｆ", // U+FF26: FULLWIDTH LATIN CAPITAL LETTER F
            ],
            "F",
        ),
        (
            &[
                "ƒ",  // U+0192: LATIN SMALL LETTER F WITH HOOK
                "ᵮ",  // U+1D6E: LATIN SMALL LETTER F WITH MIDDLE TILDE
                "ᶂ",  // U+1D82: LATIN SMALL LETTER F WITH PALATAL HOOK
                "ḟ",  // U+1E1F: LATIN SMALL LETTER F WITH DOT ABOVE
                "ẛ",  // U+1E9B: LATIN SMALL LETTER LONG S WITH DOT ABOVE
                "ⓕ",  // U+24D5: CIRCLED LATIN SMALL LETTER F
                "ꝼ",  // U+A77C: LATIN SMALL LETTER INSULAR F
                "ｆ", // U+FF46: FULLWIDTH LATIN SMALL LETTER F
            ],
            "f",
        ),
        (
            &[
                "⒡", // U+24A1: PARENTHESIZED LATIN SMALL LETTER F
            ],
            "(f)",
        ),
        (
            &[
                "ﬀ", // U+FB00: LATIN SMALL LIGATURE FF
            ],
            "ff",
        ),
        (
            &[
                "ﬃ", // U+FB03: LATIN SMALL LIGATURE FFI
            ],
            "ffi",
        ),
        (
            &[
                "ﬄ", // U+FB04: LATIN SMALL LIGATURE FFL
            ],
            "ffl",
        ),
        (
            &[
                "ﬁ", // U+FB01: LATIN SMALL LIGATURE FI
            ],
            "fi",
        ),
        (
            &[
                "ﬂ", // U+FB02: LATIN SMALL LIGATURE FL
            ],
            "fl",
        ),
        (
            &[
                "Ĝ",  // U+011C: LATIN CAPITAL LETTER G WITH CIRCUMFLEX
                "Ğ",  // U+011E: LATIN CAPITAL LETTER G WITH BREVE
                "Ġ",  // U+0120: LATIN CAPITAL LETTER G WITH DOT ABOVE
                "Ģ",  // U+0122: LATIN CAPITAL LETTER G WITH CEDILLA
                "Ɠ",  // U+0193: LATIN CAPITAL LETTER G WITH HOOK
                "Ǥ",  // U+01E4: LATIN CAPITAL LETTER G WITH STROKE
                "ǥ",  // U+01E5: LATIN SMALL LETTER G WITH STROKE
                "Ǧ",  // U+01E6: LATIN CAPITAL LETTER G WITH CARON
                "ǧ",  // U+01E7: LATIN SMALL LETTER G WITH CARON
                "Ǵ",  // U+01F4: LATIN CAPITAL LETTER G WITH ACUTE
                "ɢ",  // U+0262: LATIN LETTER SMALL CAPITAL G
                "ʛ",  // U+029B: LATIN LETTER SMALL CAPITAL G WITH HOOK
                "Ḡ",  // U+1E20: LATIN CAPITAL LETTER G WITH MACRON
                "Ⓖ",  // U+24BC: CIRCLED LATIN CAPITAL LETTER G
                "Ᵹ",  // U+A77D: LATIN CAPITAL LETTER INSULAR G
                "Ꝿ",  // U+A77E: LATIN CAPITAL LETTER TURNED INSULAR G
                "Ｇ", // U+FF27: FULLWIDTH LATIN CAPITAL LETTER G
            ],
            "G",
        ),
        (
            &[
                "ĝ",  // U+011D: LATIN SMALL LETTER G WITH CIRCUMFLEX
                "ğ",  // U+011F: LATIN SMALL LETTER G WITH BREVE
                "ġ",  // U+0121: LATIN SMALL LETTER G WITH DOT ABOVE
                "ģ",  // U+0123: LATIN SMALL LETTER G WITH CEDILLA
                "ǵ",  // U+01F5: LATIN SMALL LETTER G WITH ACUTE
                "ɠ",  // U+0260: LATIN SMALL LETTER G WITH HOOK
                "ɡ",  // U+0261: LATIN SMALL LETTER SCRIPT G
                "ᵷ",  // U+1D77: LATIN SMALL LETTER TURNED G
                "ᵹ",  // U+1D79: LATIN SMALL LETTER INSULAR G
                "ᶃ",  // U+1D83: LATIN SMALL LETTER G WITH PALATAL HOOK
                "ḡ",  // U+1E21: LATIN SMALL LETTER G WITH MACRON
                "ⓖ",  // U+24D6: CIRCLED LATIN SMALL LETTER G
                "ꝿ",  // U+A77F: LATIN SMALL LETTER TURNED INSULAR G
                "ｇ", // U+FF47: FULLWIDTH LATIN SMALL LETTER G
            ],
            "g",
        ),
        (
            &[
                "⒢", // U+24A2: PARENTHESIZED LATIN SMALL LETTER G
            ],
            "(g)",
        ),
        (
            &[
                "Ĥ",  // U+0124: LATIN CAPITAL LETTER H WITH CIRCUMFLEX
                "Ħ",  // U+0126: LATIN CAPITAL LETTER H WITH STROKE
                "Ȟ",  // U+021E: LATIN CAPITAL LETTER H WITH CARON
                "ʜ",  // U+029C: LATIN LETTER SMALL CAPITAL H
                "Ḣ",  // U+1E22: LATIN CAPITAL LETTER H WITH DOT ABOVE
                "Ḥ",  // U+1E24: LATIN CAPITAL LETTER H WITH DOT BELOW
                "Ḧ",  // U+1E26: LATIN CAPITAL LETTER H WITH DIAERESIS
                "Ḩ",  // U+1E28: LATIN CAPITAL LETTER H WITH CEDILLA
                "Ḫ",  // U+1E2A: LATIN CAPITAL LETTER H WITH BREVE BELOW
                "Ⓗ",  // U+24BD: CIRCLED LATIN CAPITAL LETTER H
                "Ⱨ",  // U+2C67: LATIN CAPITAL LETTER H WITH DESCENDER
                "Ⱶ",  // U+2C75: LATIN CAPITAL LETTER HALF H
                "Ｈ", // U+FF28: FULLWIDTH LATIN CAPITAL LETTER H
            ],
            "H",
        ),
        (
            &[
                "ĥ",  // U+0125: LATIN SMALL LETTER H WITH CIRCUMFLEX
                "ħ",  // U+0127: LATIN SMALL LETTER H WITH STROKE
                "ȟ",  // U+021F: LATIN SMALL LETTER H WITH CARON
                "ɥ",  // U+0265: LATIN SMALL LETTER TURNED H
                "ɦ",  // U+0266: LATIN SMALL LETTER H WITH HOOK
                "ʮ",  // U+02AE: LATIN SMALL LETTER TURNED H WITH FISHHOOK
                "ʯ",  // U+02AF: LATIN SMALL LETTER TURNED H WITH FISHHOOK AND TAIL
                "ḣ",  // U+1E23: LATIN SMALL LETTER H WITH DOT ABOVE
                "ḥ",  // U+1E25: LATIN SMALL LETTER H WITH DOT BELOW
                "ḧ",  // U+1E27: LATIN SMALL LETTER H WITH DIAERESIS
                "ḩ",  // U+1E29: LATIN SMALL LETTER H WITH CEDILLA
                "ḫ",  // U+1E2B: LATIN SMALL LETTER H WITH BREVE BELOW
                "ẖ",  // U+1E96: LATIN SMALL LETTER H WITH LINE BELOW
                "ⓗ",  // U+24D7: CIRCLED LATIN SMALL LETTER H
                "ⱨ",  // U+2C68: LATIN SMALL LETTER H WITH DESCENDER
                "ⱶ",  // U+2C76: LATIN SMALL LETTER HALF H
                "ｈ", // U+FF48: FULLWIDTH LATIN SMALL LETTER H
            ],
            "h",
        ),
        (
            &[
                "Ƕ", // U+01F6: LATIN CAPITAL LETTER HWAIR
            ],
            "HV",
        ),
        (
            &[
                "⒣", // U+24A3: PARENTHESIZED LATIN SMALL LETTER H
            ],
            "(h)",
        ),
        (
            &[
                "ƕ", // U+0195: LATIN SMALL LETTER HV
            ],
            "hv",
        ),
        (
            &[
                "Ì",  // U+00CC: LATIN CAPITAL LETTER I WITH GRAVE
                "Í",  // U+00CD: LATIN CAPITAL LETTER I WITH ACUTE
                "Î",  // U+00CE: LATIN CAPITAL LETTER I WITH CIRCUMFLEX
                "Ï",  // U+00CF: LATIN CAPITAL LETTER I WITH DIAERESIS
                "Ĩ",  // U+0128: LATIN CAPITAL LETTER I WITH TILDE
                "Ī",  // U+012A: LATIN CAPITAL LETTER I WITH MACRON
                "Ĭ",  // U+012C: LATIN CAPITAL LETTER I WITH BREVE
                "Į",  // U+012E: LATIN CAPITAL LETTER I WITH OGONEK
                "İ",  // U+0130: LATIN CAPITAL LETTER I WITH DOT ABOVE
                "Ɩ",  // U+0196: LATIN CAPITAL LETTER IOTA
                "Ɨ",  // U+0197: LATIN CAPITAL LETTER I WITH STROKE
                "Ǐ",  // U+01CF: LATIN CAPITAL LETTER I WITH CARON
                "Ȉ",  // U+0208: LATIN CAPITAL LETTER I WITH DOUBLE GRAVE
                "Ȋ",  // U+020A: LATIN CAPITAL LETTER I WITH INVERTED BREVE
                "ɪ",  // U+026A: LATIN LETTER SMALL CAPITAL I
                "ᵻ",  // U+1D7B: LATIN SMALL CAPITAL LETTER I WITH STROKE
                "Ḭ",  // U+1E2C: LATIN CAPITAL LETTER I WITH TILDE BELOW
                "Ḯ",  // U+1E2E: LATIN CAPITAL LETTER I WITH DIAERESIS AND ACUTE
                "Ỉ",  // U+1EC8: LATIN CAPITAL LETTER I WITH HOOK ABOVE
                "Ị",  // U+1ECA: LATIN CAPITAL LETTER I WITH DOT BELOW
                "Ⓘ",  // U+24BE: CIRCLED LATIN CAPITAL LETTER I
                "ꟾ",  // U+A7FE: LATIN EPIGRAPHIC LETTER I LONGA
                "Ｉ", // U+FF29: FULLWIDTH LATIN CAPITAL LETTER I
            ],
            "I",
        ),
        (
            &[
                "ì",  // U+00EC: LATIN SMALL LETTER I WITH GRAVE
                "í",  // U+00ED: LATIN SMALL LETTER I WITH ACUTE
                "î",  // U+00EE: LATIN SMALL LETTER I WITH CIRCUMFLEX
                "ï",  // U+00EF: LATIN SMALL LETTER I WITH DIAERESIS
                "ĩ",  // U+0129: LATIN SMALL LETTER I WITH TILDE
                "ī",  // U+012B: LATIN SMALL LETTER I WITH MACRON
                "ĭ",  // U+012D: LATIN SMALL LETTER I WITH BREVE
                "į",  // U+012F: LATIN SMALL LETTER I WITH OGONEK
                "ı",  // U+0131: LATIN SMALL LETTER DOTLESS I
                "ǐ",  // U+01D0: LATIN SMALL LETTER I WITH CARON
                "ȉ",  // U+0209: LATIN SMALL LETTER I WITH DOUBLE GRAVE
                "ȋ",  // U+020B: LATIN SMALL LETTER I WITH INVERTED BREVE
                "ɨ",  // U+0268: LATIN SMALL LETTER I WITH STROKE
                "ᴉ",  // U+1D09: LATIN SMALL LETTER TURNED I
                "ᵢ",  // U+1D62: LATIN SUBSCRIPT SMALL LETTER I
                "ᵼ",  // U+1D7C: LATIN SMALL LETTER IOTA WITH STROKE
                "ᶖ",  // U+1D96: LATIN SMALL LETTER I WITH RETROFLEX HOOK
                "ḭ",  // U+1E2D: LATIN SMALL LETTER I WITH TILDE BELOW
                "ḯ",  // U+1E2F: LATIN SMALL LETTER I WITH DIAERESIS AND ACUTE
                "ỉ",  // U+1EC9: LATIN SMALL LETTER I WITH HOOK ABOVE
                "ị",  // U+1ECB: LATIN SMALL LETTER I WITH DOT BELOW
                "ⁱ",  // U+2071: SUPERSCRIPT LATIN SMALL LETTER I
                "ⓘ",  // U+24D8: CIRCLED LATIN SMALL LETTER I
                "ｉ", // U+FF49: FULLWIDTH LATIN SMALL LETTER I
            ],
            "i",
        ),
        (
            &[
                "Ĳ", // U+0132: LATIN CAPITAL LIGATURE IJ
            ],
            "IJ",
        ),
        (
            &[
                "⒤", // U+24A4: PARENTHESIZED LATIN SMALL LETTER I
            ],
            "(i)",
        ),
        (
            &[
                "ĳ", // U+0133: LATIN SMALL LIGATURE IJ
            ],
            "ij",
        ),
    ];

    for (characters, folded) in foldings {
        for &c in characters {
            assert_eq!(
                folding_using_raw_tokenizer_helper(c),
                folded,
                "testing that character \"{}\" becomes \"{}\"",
                c,
                folded
            );
        }
    }
}
