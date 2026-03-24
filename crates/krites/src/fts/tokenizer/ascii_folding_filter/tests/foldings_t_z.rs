//! ASCII folding tests for letters T through Z.

use super::folding_using_raw_tokenizer_helper;

#[test]
fn test_all_foldings_t_through_z() {
    let foldings: Vec<(&[&str], &str)> = vec![
        (
            &[
                "Ţ",  // U+0162: LATIN CAPITAL LETTER T WITH CEDILLA
                "Ť",  // U+0164: LATIN CAPITAL LETTER T WITH CARON
                "Ŧ",  // U+0166: LATIN CAPITAL LETTER T WITH STROKE
                "Ƭ",  // U+01AC: LATIN CAPITAL LETTER T WITH HOOK
                "Ʈ",  // U+01AE: LATIN CAPITAL LETTER T WITH RETROFLEX HOOK
                "Ț",  // U+021A: LATIN CAPITAL LETTER T WITH COMMA BELOW
                "Ⱦ",  // U+023E: LATIN CAPITAL LETTER T WITH DIAGONAL STROKE
                "ᴛ",  // U+1D1B: LATIN LETTER SMALL CAPITAL T
                "Ṫ",  // U+1E6A: LATIN CAPITAL LETTER T WITH DOT ABOVE
                "Ṭ",  // U+1E6C: LATIN CAPITAL LETTER T WITH DOT BELOW
                "Ṯ",  // U+1E6E: LATIN CAPITAL LETTER T WITH LINE BELOW
                "Ṱ",  // U+1E70: LATIN CAPITAL LETTER T WITH CIRCUMFLEX BELOW
                "Ⓣ",  // U+24C9: CIRCLED LATIN CAPITAL LETTER T
                "Ꞇ",  // U+A786: LATIN CAPITAL LETTER INSULAR T
                "Ｔ", // U+FF34: FULLWIDTH LATIN CAPITAL LETTER T
            ],
            "T",
        ),
        (
            &[
                "ţ",  // U+0163: LATIN SMALL LETTER T WITH CEDILLA
                "ť",  // U+0165: LATIN SMALL LETTER T WITH CARON
                "ŧ",  // U+0167: LATIN SMALL LETTER T WITH STROKE
                "ƫ",  // U+01AB: LATIN SMALL LETTER T WITH PALATAL HOOK
                "ƭ",  // U+01AD: LATIN SMALL LETTER T WITH HOOK
                "ț",  // U+021B: LATIN SMALL LETTER T WITH COMMA BELOW
                "ȶ",  // U+0236: LATIN SMALL LETTER T WITH CURL
                "ʇ",  // U+0287: LATIN SMALL LETTER TURNED T
                "ʈ",  // U+0288: LATIN SMALL LETTER T WITH RETROFLEX HOOK
                "ᵵ",  // U+1D75: LATIN SMALL LETTER T WITH MIDDLE TILDE
                "ṫ",  // U+1E6B: LATIN SMALL LETTER T WITH DOT ABOVE
                "ṭ",  // U+1E6D: LATIN SMALL LETTER T WITH DOT BELOW
                "ṯ",  // U+1E6F: LATIN SMALL LETTER T WITH LINE BELOW
                "ṱ",  // U+1E71: LATIN SMALL LETTER T WITH CIRCUMFLEX BELOW
                "ẗ",  // U+1E97: LATIN SMALL LETTER T WITH DIAERESIS
                "ⓣ",  // U+24E3: CIRCLED LATIN SMALL LETTER T
                "ⱦ",  // U+2C66: LATIN SMALL LETTER T WITH DIAGONAL STROKE
                "ｔ", // U+FF54: FULLWIDTH LATIN SMALL LETTER T
            ],
            "t",
        ),
        (
            &[
                "Þ", // U+00DE: LATIN CAPITAL LETTER THORN
                "Ꝧ", // U+A766: LATIN CAPITAL LETTER THORN WITH STROKE THROUGH DESCENDER
            ],
            "TH",
        ),
        (
            &[
                "Ꜩ", // U+A728: LATIN CAPITAL LETTER TZ
            ],
            "TZ",
        ),
        (
            &[
                "⒯", // U+24AF: PARENTHESIZED LATIN SMALL LETTER T
            ],
            "(t)",
        ),
        (
            &[
                "ʨ", // U+02A8: LATIN SMALL LETTER TC DIGRAPH WITH CURL
            ],
            "tc",
        ),
        (
            &[
                "þ", // U+00FE: LATIN SMALL LETTER THORN
                "ᵺ", // U+1D7A: LATIN SMALL LETTER TH WITH STRIKETHROUGH
                "ꝧ", // U+A767: LATIN SMALL LETTER THORN WITH STROKE THROUGH DESCENDER
            ],
            "th",
        ),
        (
            &[
                "ʦ", // U+02A6: LATIN SMALL LETTER TS DIGRAPH
            ],
            "ts",
        ),
        (
            &[
                "ꜩ", // U+A729: LATIN SMALL LETTER TZ
            ],
            "tz",
        ),
        (
            &[
                "Ù",  // U+00D9: LATIN CAPITAL LETTER U WITH GRAVE
                "Ú",  // U+00DA: LATIN CAPITAL LETTER U WITH ACUTE
                "Û",  // U+00DB: LATIN CAPITAL LETTER U WITH CIRCUMFLEX
                "Ü",  // U+00DC: LATIN CAPITAL LETTER U WITH DIAERESIS
                "Ũ",  // U+0168: LATIN CAPITAL LETTER U WITH TILDE
                "Ū",  // U+016A: LATIN CAPITAL LETTER U WITH MACRON
                "Ŭ",  // U+016C: LATIN CAPITAL LETTER U WITH BREVE
                "Ů",  // U+016E: LATIN CAPITAL LETTER U WITH RING ABOVE
                "Ű",  // U+0170: LATIN CAPITAL LETTER U WITH DOUBLE ACUTE
                "Ų",  // U+0172: LATIN CAPITAL LETTER U WITH OGONEK
                "Ư",  // U+01AF: LATIN CAPITAL LETTER U WITH HORN
                "Ǔ",  // U+01D3: LATIN CAPITAL LETTER U WITH CARON
                "Ǖ",  // U+01D5: LATIN CAPITAL LETTER U WITH DIAERESIS AND MACRON
                "Ǘ",  // U+01D7: LATIN CAPITAL LETTER U WITH DIAERESIS AND ACUTE
                "Ǚ",  // U+01D9: LATIN CAPITAL LETTER U WITH DIAERESIS AND CARON
                "Ǜ",  // U+01DB: LATIN CAPITAL LETTER U WITH DIAERESIS AND GRAVE
                "Ȕ",  // U+0214: LATIN CAPITAL LETTER U WITH DOUBLE GRAVE
                "Ȗ",  // U+0216: LATIN CAPITAL LETTER U WITH INVERTED BREVE
                "Ʉ",  // U+0244: LATIN CAPITAL LETTER U BAR
                "ᴜ",  // U+1D1C: LATIN LETTER SMALL CAPITAL U
                "ᵾ",  // U+1D7E: LATIN SMALL CAPITAL LETTER U WITH STROKE
                "Ṳ",  // U+1E72: LATIN CAPITAL LETTER U WITH DIAERESIS BELOW
                "Ṵ",  // U+1E74: LATIN CAPITAL LETTER U WITH TILDE BELOW
                "Ṷ",  // U+1E76: LATIN CAPITAL LETTER U WITH CIRCUMFLEX BELOW
                "Ṹ",  // U+1E78: LATIN CAPITAL LETTER U WITH TILDE AND ACUTE
                "Ṻ",  // U+1E7A: LATIN CAPITAL LETTER U WITH MACRON AND DIAERESIS
                "Ụ",  // U+1EE4: LATIN CAPITAL LETTER U WITH DOT BELOW
                "Ủ",  // U+1EE6: LATIN CAPITAL LETTER U WITH HOOK ABOVE
                "Ứ",  // U+1EE8: LATIN CAPITAL LETTER U WITH HORN AND ACUTE
                "Ừ",  // U+1EEA: LATIN CAPITAL LETTER U WITH HORN AND GRAVE
                "Ử",  // U+1EEC: LATIN CAPITAL LETTER U WITH HORN AND HOOK ABOVE
                "Ữ",  // U+1EEE: LATIN CAPITAL LETTER U WITH HORN AND TILDE
                "Ự",  // U+1EF0: LATIN CAPITAL LETTER U WITH HORN AND DOT BELOW
                "Ⓤ",  // U+24CA: CIRCLED LATIN CAPITAL LETTER U
                "Ｕ", // U+FF35: FULLWIDTH LATIN CAPITAL LETTER U
            ],
            "U",
        ),
        (
            &[
                "ù",  // U+00F9: LATIN SMALL LETTER U WITH GRAVE
                "ú",  // U+00FA: LATIN SMALL LETTER U WITH ACUTE
                "û",  // U+00FB: LATIN SMALL LETTER U WITH CIRCUMFLEX
                "ü",  // U+00FC: LATIN SMALL LETTER U WITH DIAERESIS
                "ũ",  // U+0169: LATIN SMALL LETTER U WITH TILDE
                "ū",  // U+016B: LATIN SMALL LETTER U WITH MACRON
                "ŭ",  // U+016D: LATIN SMALL LETTER U WITH BREVE
                "ů",  // U+016F: LATIN SMALL LETTER U WITH RING ABOVE
                "ű",  // U+0171: LATIN SMALL LETTER U WITH DOUBLE ACUTE
                "ų",  // U+0173: LATIN SMALL LETTER U WITH OGONEK
                "ư",  // U+01B0: LATIN SMALL LETTER U WITH HORN
                "ǔ",  // U+01D4: LATIN SMALL LETTER U WITH CARON
                "ǖ",  // U+01D6: LATIN SMALL LETTER U WITH DIAERESIS AND MACRON
                "ǘ",  // U+01D8: LATIN SMALL LETTER U WITH DIAERESIS AND ACUTE
                "ǚ",  // U+01DA: LATIN SMALL LETTER U WITH DIAERESIS AND CARON
                "ǜ",  // U+01DC: LATIN SMALL LETTER U WITH DIAERESIS AND GRAVE
                "ȕ",  // U+0215: LATIN SMALL LETTER U WITH DOUBLE GRAVE
                "ȗ",  // U+0217: LATIN SMALL LETTER U WITH INVERTED BREVE
                "ʉ",  // U+0289: LATIN SMALL LETTER U BAR
                "ᵤ",  // U+1D64: LATIN SUBSCRIPT SMALL LETTER U
                "ᶙ",  // U+1D99: LATIN SMALL LETTER U WITH RETROFLEX HOOK
                "ṳ",  // U+1E73: LATIN SMALL LETTER U WITH DIAERESIS BELOW
                "ṵ",  // U+1E75: LATIN SMALL LETTER U WITH TILDE BELOW
                "ṷ",  // U+1E77: LATIN SMALL LETTER U WITH CIRCUMFLEX BELOW
                "ṹ",  // U+1E79: LATIN SMALL LETTER U WITH TILDE AND ACUTE
                "ṻ",  // U+1E7B: LATIN SMALL LETTER U WITH MACRON AND DIAERESIS
                "ụ",  // U+1EE5: LATIN SMALL LETTER U WITH DOT BELOW
                "ủ",  // U+1EE7: LATIN SMALL LETTER U WITH HOOK ABOVE
                "ứ",  // U+1EE9: LATIN SMALL LETTER U WITH HORN AND ACUTE
                "ừ",  // U+1EEB: LATIN SMALL LETTER U WITH HORN AND GRAVE
                "ử",  // U+1EED: LATIN SMALL LETTER U WITH HORN AND HOOK ABOVE
                "ữ",  // U+1EEF: LATIN SMALL LETTER U WITH HORN AND TILDE
                "ự",  // U+1EF1: LATIN SMALL LETTER U WITH HORN AND DOT BELOW
                "ⓤ",  // U+24E4: CIRCLED LATIN SMALL LETTER U
                "ｕ", // U+FF55: FULLWIDTH LATIN SMALL LETTER U
            ],
            "u",
        ),
        (
            &[
                "⒰", // U+24B0: PARENTHESIZED LATIN SMALL LETTER U
            ],
            "(u)",
        ),
        (
            &[
                "ᵫ", // U+1D6B: LATIN SMALL LETTER UE
            ],
            "ue",
        ),
        (
            &[
                "Ʋ",  // U+01B2: LATIN CAPITAL LETTER V WITH HOOK
                "Ʌ",  // U+0245: LATIN CAPITAL LETTER TURNED V
                "ᴠ",  // U+1D20: LATIN LETTER SMALL CAPITAL V
                "Ṽ",  // U+1E7C: LATIN CAPITAL LETTER V WITH TILDE
                "Ṿ",  // U+1E7E: LATIN CAPITAL LETTER V WITH DOT BELOW
                "Ỽ",  // U+1EFC: LATIN CAPITAL LETTER MIDDLE-WELSH V
                "Ⓥ",  // U+24CB: CIRCLED LATIN CAPITAL LETTER V
                "Ꝟ",  // U+A75E: LATIN CAPITAL LETTER V WITH DIAGONAL STROKE
                "Ꝩ",  // U+A768: LATIN CAPITAL LETTER VEND
                "Ｖ", // U+FF36: FULLWIDTH LATIN CAPITAL LETTER V
            ],
            "V",
        ),
        (
            &[
                "ʋ",  // U+028B: LATIN SMALL LETTER V WITH HOOK
                "ʌ",  // U+028C: LATIN SMALL LETTER TURNED V
                "ᵥ",  // U+1D65: LATIN SUBSCRIPT SMALL LETTER V
                "ᶌ",  // U+1D8C: LATIN SMALL LETTER V WITH PALATAL HOOK
                "ṽ",  // U+1E7D: LATIN SMALL LETTER V WITH TILDE
                "ṿ",  // U+1E7F: LATIN SMALL LETTER V WITH DOT BELOW
                "ⓥ",  // U+24E5: CIRCLED LATIN SMALL LETTER V
                "ⱱ",  // U+2C71: LATIN SMALL LETTER V WITH RIGHT HOOK
                "ⱴ",  // U+2C74: LATIN SMALL LETTER V WITH CURL
                "ꝟ",  // U+A75F: LATIN SMALL LETTER V WITH DIAGONAL STROKE
                "ｖ", // U+FF56: FULLWIDTH LATIN SMALL LETTER V
            ],
            "v",
        ),
        (
            &[
                "Ꝡ", // U+A760: LATIN CAPITAL LETTER VY
            ],
            "VY",
        ),
        (
            &[
                "⒱", // U+24B1: PARENTHESIZED LATIN SMALL LETTER V
            ],
            "(v)",
        ),
        (
            &[
                "ꝡ", // U+A761: LATIN SMALL LETTER VY
            ],
            "vy",
        ),
        (
            &[
                "Ŵ",  // U+0174: LATIN CAPITAL LETTER W WITH CIRCUMFLEX
                "Ƿ",  // U+01F7: LATIN CAPITAL LETTER WYNN
                "ᴡ",  // U+1D21: LATIN LETTER SMALL CAPITAL W
                "Ẁ",  // U+1E80: LATIN CAPITAL LETTER W WITH GRAVE
                "Ẃ",  // U+1E82: LATIN CAPITAL LETTER W WITH ACUTE
                "Ẅ",  // U+1E84: LATIN CAPITAL LETTER W WITH DIAERESIS
                "Ẇ",  // U+1E86: LATIN CAPITAL LETTER W WITH DOT ABOVE
                "Ẉ",  // U+1E88: LATIN CAPITAL LETTER W WITH DOT BELOW
                "Ⓦ",  // U+24CC: CIRCLED LATIN CAPITAL LETTER W
                "Ⱳ",  // U+2C72: LATIN CAPITAL LETTER W WITH HOOK
                "Ｗ", // U+FF37: FULLWIDTH LATIN CAPITAL LETTER W
            ],
            "W",
        ),
        (
            &[
                "ŵ",  // U+0175: LATIN SMALL LETTER W WITH CIRCUMFLEX
                "ƿ",  // U+01BF: LATIN LETTER WYNN
                "ʍ",  // U+028D: LATIN SMALL LETTER TURNED W
                "ẁ",  // U+1E81: LATIN SMALL LETTER W WITH GRAVE
                "ẃ",  // U+1E83: LATIN SMALL LETTER W WITH ACUTE
                "ẅ",  // U+1E85: LATIN SMALL LETTER W WITH DIAERESIS
                "ẇ",  // U+1E87: LATIN SMALL LETTER W WITH DOT ABOVE
                "ẉ",  // U+1E89: LATIN SMALL LETTER W WITH DOT BELOW
                "ẘ",  // U+1E98: LATIN SMALL LETTER W WITH RING ABOVE
                "ⓦ",  // U+24E6: CIRCLED LATIN SMALL LETTER W
                "ⱳ",  // U+2C73: LATIN SMALL LETTER W WITH HOOK
                "ｗ", // U+FF57: FULLWIDTH LATIN SMALL LETTER W
            ],
            "w",
        ),
        (
            &[
                "⒲", // U+24B2: PARENTHESIZED LATIN SMALL LETTER W
            ],
            "(w)",
        ),
        (
            &[
                "Ẋ",  // U+1E8A: LATIN CAPITAL LETTER X WITH DOT ABOVE
                "Ẍ",  // U+1E8C: LATIN CAPITAL LETTER X WITH DIAERESIS
                "Ⓧ",  // U+24CD: CIRCLED LATIN CAPITAL LETTER X
                "Ｘ", // U+FF38: FULLWIDTH LATIN CAPITAL LETTER X
            ],
            "X",
        ),
        (
            &[
                "ᶍ",  // U+1D8D: LATIN SMALL LETTER X WITH PALATAL HOOK
                "ẋ",  // U+1E8B: LATIN SMALL LETTER X WITH DOT ABOVE
                "ẍ",  // U+1E8D: LATIN SMALL LETTER X WITH DIAERESIS
                "ₓ",  // U+2093: LATIN SUBSCRIPT SMALL LETTER X
                "ⓧ",  // U+24E7: CIRCLED LATIN SMALL LETTER X
                "ｘ", // U+FF58: FULLWIDTH LATIN SMALL LETTER X
            ],
            "x",
        ),
        (
            &[
                "⒳", // U+24B3: PARENTHESIZED LATIN SMALL LETTER X
            ],
            "(x)",
        ),
        (
            &[
                "Ý",  // U+00DD: LATIN CAPITAL LETTER Y WITH ACUTE
                "Ŷ",  // U+0176: LATIN CAPITAL LETTER Y WITH CIRCUMFLEX
                "Ÿ",  // U+0178: LATIN CAPITAL LETTER Y WITH DIAERESIS
                "Ƴ",  // U+01B3: LATIN CAPITAL LETTER Y WITH HOOK
                "Ȳ",  // U+0232: LATIN CAPITAL LETTER Y WITH MACRON
                "Ɏ",  // U+024E: LATIN CAPITAL LETTER Y WITH STROKE
                "ʏ",  // U+028F: LATIN LETTER SMALL CAPITAL Y
                "Ẏ",  // U+1E8E: LATIN CAPITAL LETTER Y WITH DOT ABOVE
                "Ỳ",  // U+1EF2: LATIN CAPITAL LETTER Y WITH GRAVE
                "Ỵ",  // U+1EF4: LATIN CAPITAL LETTER Y WITH DOT BELOW
                "Ỷ",  // U+1EF6: LATIN CAPITAL LETTER Y WITH HOOK ABOVE
                "Ỹ",  // U+1EF8: LATIN CAPITAL LETTER Y WITH TILDE
                "Ỿ",  // U+1EFE: LATIN CAPITAL LETTER Y WITH LOOP
                "Ⓨ",  // U+24CE: CIRCLED LATIN CAPITAL LETTER Y
                "Ｙ", // U+FF39: FULLWIDTH LATIN CAPITAL LETTER Y
            ],
            "Y",
        ),
        (
            &[
                "ý",  // U+00FD: LATIN SMALL LETTER Y WITH ACUTE
                "ÿ",  // U+00FF: LATIN SMALL LETTER Y WITH DIAERESIS
                "ŷ",  // U+0177: LATIN SMALL LETTER Y WITH CIRCUMFLEX
                "ƴ",  // U+01B4: LATIN SMALL LETTER Y WITH HOOK
                "ȳ",  // U+0233: LATIN SMALL LETTER Y WITH MACRON
                "ɏ",  // U+024F: LATIN SMALL LETTER Y WITH STROKE
                "ʎ",  // U+028E: LATIN SMALL LETTER TURNED Y
                "ẏ",  // U+1E8F: LATIN SMALL LETTER Y WITH DOT ABOVE
                "ẙ",  // U+1E99: LATIN SMALL LETTER Y WITH RING ABOVE
                "ỳ",  // U+1EF3: LATIN SMALL LETTER Y WITH GRAVE
                "ỵ",  // U+1EF5: LATIN SMALL LETTER Y WITH DOT BELOW
                "ỷ",  // U+1EF7: LATIN SMALL LETTER Y WITH HOOK ABOVE
                "ỹ",  // U+1EF9: LATIN SMALL LETTER Y WITH TILDE
                "ỿ",  // U+1EFF: LATIN SMALL LETTER Y WITH LOOP
                "ⓨ",  // U+24E8: CIRCLED LATIN SMALL LETTER Y
                "ｙ", // U+FF59: FULLWIDTH LATIN SMALL LETTER Y
            ],
            "y",
        ),
        (
            &[
                "⒴", // U+24B4: PARENTHESIZED LATIN SMALL LETTER Y
            ],
            "(y)",
        ),
        (
            &[
                "Ź",  // U+0179: LATIN CAPITAL LETTER Z WITH ACUTE
                "Ż",  // U+017B: LATIN CAPITAL LETTER Z WITH DOT ABOVE
                "Ž",  // U+017D: LATIN CAPITAL LETTER Z WITH CARON
                "Ƶ",  // U+01B5: LATIN CAPITAL LETTER Z WITH STROKE
                "Ȝ",  // U+021C: LATIN CAPITAL LETTER YOGH
                "Ȥ",  // U+0224: LATIN CAPITAL LETTER Z WITH HOOK
                "ᴢ",  // U+1D22: LATIN LETTER SMALL CAPITAL Z
                "Ẑ",  // U+1E90: LATIN CAPITAL LETTER Z WITH CIRCUMFLEX
                "Ẓ",  // U+1E92: LATIN CAPITAL LETTER Z WITH DOT BELOW
                "Ẕ",  // U+1E94: LATIN CAPITAL LETTER Z WITH LINE BELOW
                "Ⓩ",  // U+24CF: CIRCLED LATIN CAPITAL LETTER Z
                "Ⱬ",  // U+2C6B: LATIN CAPITAL LETTER Z WITH DESCENDER
                "Ꝣ",  // U+A762: LATIN CAPITAL LETTER VISIGOTHIC Z
                "Ｚ", // U+FF3A: FULLWIDTH LATIN CAPITAL LETTER Z
            ],
            "Z",
        ),
        (
            &[
                "ź",  // U+017A: LATIN SMALL LETTER Z WITH ACUTE
                "ż",  // U+017C: LATIN SMALL LETTER Z WITH DOT ABOVE
                "ž",  // U+017E: LATIN SMALL LETTER Z WITH CARON
                "ƶ",  // U+01B6: LATIN SMALL LETTER Z WITH STROKE
                "ȝ",  // U+021D: LATIN SMALL LETTER YOGH
                "ȥ",  // U+0225: LATIN SMALL LETTER Z WITH HOOK
                "ɀ",  // U+0240: LATIN SMALL LETTER Z WITH SWASH TAIL
                "ʐ",  // U+0290: LATIN SMALL LETTER Z WITH RETROFLEX HOOK
                "ʑ",  // U+0291: LATIN SMALL LETTER Z WITH CURL
                "ᵶ",  // U+1D76: LATIN SMALL LETTER Z WITH MIDDLE TILDE
                "ᶎ",  // U+1D8E: LATIN SMALL LETTER Z WITH PALATAL HOOK
                "ẑ",  // U+1E91: LATIN SMALL LETTER Z WITH CIRCUMFLEX
                "ẓ",  // U+1E93: LATIN SMALL LETTER Z WITH DOT BELOW
                "ẕ",  // U+1E95: LATIN SMALL LETTER Z WITH LINE BELOW
                "ⓩ",  // U+24E9: CIRCLED LATIN SMALL LETTER Z
                "ⱬ",  // U+2C6C: LATIN SMALL LETTER Z WITH DESCENDER
                "ꝣ",  // U+A763: LATIN SMALL LETTER VISIGOTHIC Z
                "ｚ", // U+FF5A: FULLWIDTH LATIN SMALL LETTER Z
            ],
            "z",
        ),
        (
            &[
                "⒵", // U+24B5: PARENTHESIZED LATIN SMALL LETTER Z
            ],
            "(z)",
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
