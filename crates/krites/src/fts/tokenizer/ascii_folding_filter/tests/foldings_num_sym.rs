//! ASCII folding tests for numbers and symbols.

use super::folding_using_raw_tokenizer_helper;

#[test]
fn test_all_foldings_numbers_and_symbols() {
    let foldings: Vec<(&[&str], &str)> = vec![
        (
            &[
                "⁰",  // U+2070: SUPERSCRIPT ZERO
                "₀",  // U+2080: SUBSCRIPT ZERO
                "⓪",  // U+24EA: CIRCLED DIGIT ZERO
                "⓿",  // U+24FF: NEGATIVE CIRCLED DIGIT ZERO
                "０", // U+FF10: FULLWIDTH DIGIT ZERO
            ],
            "0",
        ),
        (
            &[
                "¹",  // U+00B9: SUPERSCRIPT ONE
                "₁",  // U+2081: SUBSCRIPT ONE
                "①",  // U+2460: CIRCLED DIGIT ONE
                "⓵",  // U+24F5: DOUBLE CIRCLED DIGIT ONE
                "❶",  // U+2776: DINGBAT NEGATIVE CIRCLED DIGIT ONE
                "➀",  // U+2780: DINGBAT CIRCLED SANS-SERIF DIGIT ONE
                "➊",  // U+278A: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT ONE
                "１", // U+FF11: FULLWIDTH DIGIT ONE
            ],
            "1",
        ),
        (
            &[
                "⒈", // U+2488: DIGIT ONE FULL STOP
            ],
            "1.",
        ),
        (
            &[
                "⑴", // U+2474: PARENTHESIZED DIGIT ONE
            ],
            "(1)",
        ),
        (
            &[
                "²",  // U+00B2: SUPERSCRIPT TWO
                "₂",  // U+2082: SUBSCRIPT TWO
                "②",  // U+2461: CIRCLED DIGIT TWO
                "⓶",  // U+24F6: DOUBLE CIRCLED DIGIT TWO
                "❷",  // U+2777: DINGBAT NEGATIVE CIRCLED DIGIT TWO
                "➁",  // U+2781: DINGBAT CIRCLED SANS-SERIF DIGIT TWO
                "➋",  // U+278B: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT TWO
                "２", // U+FF12: FULLWIDTH DIGIT TWO
            ],
            "2",
        ),
        (
            &[
                "⒉", // U+2489: DIGIT TWO FULL STOP
            ],
            "2.",
        ),
        (
            &[
                "⑵", // U+2475: PARENTHESIZED DIGIT TWO
            ],
            "(2)",
        ),
        (
            &[
                "³",  // U+00B3: SUPERSCRIPT THREE
                "₃",  // U+2083: SUBSCRIPT THREE
                "③",  // U+2462: CIRCLED DIGIT THREE
                "⓷",  // U+24F7: DOUBLE CIRCLED DIGIT THREE
                "❸",  // U+2778: DINGBAT NEGATIVE CIRCLED DIGIT THREE
                "➂",  // U+2782: DINGBAT CIRCLED SANS-SERIF DIGIT THREE
                "➌",  // U+278C: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT THREE
                "３", // U+FF13: FULLWIDTH DIGIT THREE
            ],
            "3",
        ),
        (
            &[
                "⒊", // U+248A: DIGIT THREE FULL STOP
            ],
            "3.",
        ),
        (
            &[
                "⑶", // U+2476: PARENTHESIZED DIGIT THREE
            ],
            "(3)",
        ),
        (
            &[
                "⁴",  // U+2074: SUPERSCRIPT FOUR
                "₄",  // U+2084: SUBSCRIPT FOUR
                "④",  // U+2463: CIRCLED DIGIT FOUR
                "⓸",  // U+24F8: DOUBLE CIRCLED DIGIT FOUR
                "❹",  // U+2779: DINGBAT NEGATIVE CIRCLED DIGIT FOUR
                "➃",  // U+2783: DINGBAT CIRCLED SANS-SERIF DIGIT FOUR
                "➍",  // U+278D: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT FOUR
                "４", // U+FF14: FULLWIDTH DIGIT FOUR
            ],
            "4",
        ),
        (
            &[
                "⒋", // U+248B: DIGIT FOUR FULL STOP
            ],
            "4.",
        ),
        (
            &[
                "⑷", // U+2477: PARENTHESIZED DIGIT FOUR
            ],
            "(4)",
        ),
        (
            &[
                "⁵",  // U+2075: SUPERSCRIPT FIVE
                "₅",  // U+2085: SUBSCRIPT FIVE
                "⑤",  // U+2464: CIRCLED DIGIT FIVE
                "⓹",  // U+24F9: DOUBLE CIRCLED DIGIT FIVE
                "❺",  // U+277A: DINGBAT NEGATIVE CIRCLED DIGIT FIVE
                "➄",  // U+2784: DINGBAT CIRCLED SANS-SERIF DIGIT FIVE
                "➎",  // U+278E: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT FIVE
                "５", // U+FF15: FULLWIDTH DIGIT FIVE
            ],
            "5",
        ),
        (
            &[
                "⒌", // U+248C: DIGIT FIVE FULL STOP
            ],
            "5.",
        ),
        (
            &[
                "⑸", // U+2478: PARENTHESIZED DIGIT FIVE
            ],
            "(5)",
        ),
        (
            &[
                "⁶",  // U+2076: SUPERSCRIPT SIX
                "₆",  // U+2086: SUBSCRIPT SIX
                "⑥",  // U+2465: CIRCLED DIGIT SIX
                "⓺",  // U+24FA: DOUBLE CIRCLED DIGIT SIX
                "❻",  // U+277B: DINGBAT NEGATIVE CIRCLED DIGIT SIX
                "➅",  // U+2785: DINGBAT CIRCLED SANS-SERIF DIGIT SIX
                "➏",  // U+278F: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT SIX
                "６", // U+FF16: FULLWIDTH DIGIT SIX
            ],
            "6",
        ),
        (
            &[
                "⒍", // U+248D: DIGIT SIX FULL STOP
            ],
            "6.",
        ),
        (
            &[
                "⑹", // U+2479: PARENTHESIZED DIGIT SIX
            ],
            "(6)",
        ),
        (
            &[
                "⁷",  // U+2077: SUPERSCRIPT SEVEN
                "₇",  // U+2087: SUBSCRIPT SEVEN
                "⑦",  // U+2466: CIRCLED DIGIT SEVEN
                "⓻",  // U+24FB: DOUBLE CIRCLED DIGIT SEVEN
                "❼",  // U+277C: DINGBAT NEGATIVE CIRCLED DIGIT SEVEN
                "➆",  // U+2786: DINGBAT CIRCLED SANS-SERIF DIGIT SEVEN
                "➐",  // U+2790: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT SEVEN
                "７", // U+FF17: FULLWIDTH DIGIT SEVEN
            ],
            "7",
        ),
        (
            &[
                "⒎", // U+248E: DIGIT SEVEN FULL STOP
            ],
            "7.",
        ),
        (
            &[
                "⑺", // U+247A: PARENTHESIZED DIGIT SEVEN
            ],
            "(7)",
        ),
        (
            &[
                "⁸",  // U+2078: SUPERSCRIPT EIGHT
                "₈",  // U+2088: SUBSCRIPT EIGHT
                "⑧",  // U+2467: CIRCLED DIGIT EIGHT
                "⓼",  // U+24FC: DOUBLE CIRCLED DIGIT EIGHT
                "❽",  // U+277D: DINGBAT NEGATIVE CIRCLED DIGIT EIGHT
                "➇",  // U+2787: DINGBAT CIRCLED SANS-SERIF DIGIT EIGHT
                "➑",  // U+2791: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT EIGHT
                "８", // U+FF18: FULLWIDTH DIGIT EIGHT
            ],
            "8",
        ),
        (
            &[
                "⒏", // U+248F: DIGIT EIGHT FULL STOP
            ],
            "8.",
        ),
        (
            &[
                "⑻", // U+247B: PARENTHESIZED DIGIT EIGHT
            ],
            "(8)",
        ),
        (
            &[
                "⁹",  // U+2079: SUPERSCRIPT NINE
                "₉",  // U+2089: SUBSCRIPT NINE
                "⑨",  // U+2468: CIRCLED DIGIT NINE
                "⓽",  // U+24FD: DOUBLE CIRCLED DIGIT NINE
                "❾",  // U+277E: DINGBAT NEGATIVE CIRCLED DIGIT NINE
                "➈",  // U+2788: DINGBAT CIRCLED SANS-SERIF DIGIT NINE
                "➒",  // U+2792: DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT NINE
                "９", // U+FF19: FULLWIDTH DIGIT NINE
            ],
            "9",
        ),
        (
            &[
                "⒐", // U+2490: DIGIT NINE FULL STOP
            ],
            "9.",
        ),
        (
            &[
                "⑼", // U+247C: PARENTHESIZED DIGIT NINE
            ],
            "(9)",
        ),
        (
            &[
                "⑩", // U+2469: CIRCLED NUMBER TEN
                "⓾", // U+24FE: DOUBLE CIRCLED NUMBER TEN
                "❿", // U+277F: DINGBAT NEGATIVE CIRCLED NUMBER TEN
                "➉", // U+2789: DINGBAT CIRCLED SANS-SERIF NUMBER TEN
                "➓", // U+2793: DINGBAT NEGATIVE CIRCLED SANS-SERIF NUMBER TEN
            ],
            "10",
        ),
        (
            &[
                "⒑", // U+2491: NUMBER TEN FULL STOP
            ],
            "10.",
        ),
        (
            &[
                "⑽", // U+247D: PARENTHESIZED NUMBER TEN
            ],
            "(10)",
        ),
        (
            &[
                "⑪", // U+246A: CIRCLED NUMBER ELEVEN
                "⓫", // U+24EB: NEGATIVE CIRCLED NUMBER ELEVEN
            ],
            "11",
        ),
        (
            &[
                "⒒", // U+2492: NUMBER ELEVEN FULL STOP
            ],
            "11.",
        ),
        (
            &[
                "⑾", // U+247E: PARENTHESIZED NUMBER ELEVEN
            ],
            "(11)",
        ),
        (
            &[
                "⑫", // U+246B: CIRCLED NUMBER TWELVE
                "⓬", // U+24EC: NEGATIVE CIRCLED NUMBER TWELVE
            ],
            "12",
        ),
        (
            &[
                "⒓", // U+2493: NUMBER TWELVE FULL STOP
            ],
            "12.",
        ),
        (
            &[
                "⑿", // U+247F: PARENTHESIZED NUMBER TWELVE
            ],
            "(12)",
        ),
        (
            &[
                "⑬", // U+246C: CIRCLED NUMBER THIRTEEN
                "⓭", // U+24ED: NEGATIVE CIRCLED NUMBER THIRTEEN
            ],
            "13",
        ),
        (
            &[
                "⒔", // U+2494: NUMBER THIRTEEN FULL STOP
            ],
            "13.",
        ),
        (
            &[
                "⒀", // U+2480: PARENTHESIZED NUMBER THIRTEEN
            ],
            "(13)",
        ),
        (
            &[
                "⑭", // U+246D: CIRCLED NUMBER FOURTEEN
                "⓮", // U+24EE: NEGATIVE CIRCLED NUMBER FOURTEEN
            ],
            "14",
        ),
        (
            &[
                "⒕", // U+2495: NUMBER FOURTEEN FULL STOP
            ],
            "14.",
        ),
        (
            &[
                "⒁", // U+2481: PARENTHESIZED NUMBER FOURTEEN
            ],
            "(14)",
        ),
        (
            &[
                "⑮", // U+246E: CIRCLED NUMBER FIFTEEN
                "⓯", // U+24EF: NEGATIVE CIRCLED NUMBER FIFTEEN
            ],
            "15",
        ),
        (
            &[
                "⒖", // U+2496: NUMBER FIFTEEN FULL STOP
            ],
            "15.",
        ),
        (
            &[
                "⒂", // U+2482: PARENTHESIZED NUMBER FIFTEEN
            ],
            "(15)",
        ),
        (
            &[
                "⑯", // U+246F: CIRCLED NUMBER SIXTEEN
                "⓰", // U+24F0: NEGATIVE CIRCLED NUMBER SIXTEEN
            ],
            "16",
        ),
        (
            &[
                "⒗", // U+2497: NUMBER SIXTEEN FULL STOP
            ],
            "16.",
        ),
        (
            &[
                "⒃", // U+2483: PARENTHESIZED NUMBER SIXTEEN
            ],
            "(16)",
        ),
        (
            &[
                "⑰", // U+2470: CIRCLED NUMBER SEVENTEEN
                "⓱", // U+24F1: NEGATIVE CIRCLED NUMBER SEVENTEEN
            ],
            "17",
        ),
        (
            &[
                "⒘", // U+2498: NUMBER SEVENTEEN FULL STOP
            ],
            "17.",
        ),
        (
            &[
                "⒄", // U+2484: PARENTHESIZED NUMBER SEVENTEEN
            ],
            "(17)",
        ),
        (
            &[
                "⑱", // U+2471: CIRCLED NUMBER EIGHTEEN
                "⓲", // U+24F2: NEGATIVE CIRCLED NUMBER EIGHTEEN
            ],
            "18",
        ),
        (
            &[
                "⒙", // U+2499: NUMBER EIGHTEEN FULL STOP
            ],
            "18.",
        ),
        (
            &[
                "⒅", // U+2485: PARENTHESIZED NUMBER EIGHTEEN
            ],
            "(18)",
        ),
        (
            &[
                "⑲", // U+2472: CIRCLED NUMBER NINETEEN
                "⓳", // U+24F3: NEGATIVE CIRCLED NUMBER NINETEEN
            ],
            "19",
        ),
        (
            &[
                "⒚", // U+249A: NUMBER NINETEEN FULL STOP
            ],
            "19.",
        ),
        (
            &[
                "⒆", // U+2486: PARENTHESIZED NUMBER NINETEEN
            ],
            "(19)",
        ),
        (
            &[
                "⑳", // U+2473: CIRCLED NUMBER TWENTY
                "⓴", // U+24F4: NEGATIVE CIRCLED NUMBER TWENTY
            ],
            "20",
        ),
        (
            &[
                "⒛", // U+249B: NUMBER TWENTY FULL STOP
            ],
            "20.",
        ),
        (
            &[
                "⒇", // U+2487: PARENTHESIZED NUMBER TWENTY
            ],
            "(20)",
        ),
        (
            &[
                "«",  // U+00AB: LEFT-POINTING DOUBLE ANGLE QUOTATION MARK
                "»",  // U+00BB: RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK
                "“",  // U+201C: LEFT DOUBLE QUOTATION MARK
                "”",  // U+201D: RIGHT DOUBLE QUOTATION MARK
                "„",  // U+201E: DOUBLE LOW-9 QUOTATION MARK
                "″",  // U+2033: DOUBLE PRIME
                "‶",  // U+2036: REVERSED DOUBLE PRIME
                "❝",  // U+275D: HEAVY DOUBLE TURNED COMMA QUOTATION MARK ORNAMENT
                "❞",  // U+275E: HEAVY DOUBLE COMMA QUOTATION MARK ORNAMENT
                "❮",  // U+276E: HEAVY LEFT-POINTING ANGLE QUOTATION MARK ORNAMENT
                "❯",  // U+276F: HEAVY RIGHT-POINTING ANGLE QUOTATION MARK ORNAMENT
                "＂", // U+FF02: FULLWIDTH QUOTATION MARK
            ],
            "\"",
        ),
        (
            &[
                "‘",  // U+2018: LEFT SINGLE QUOTATION MARK
                "’",  // U+2019: RIGHT SINGLE QUOTATION MARK
                "‚",  // U+201A: SINGLE LOW-9 QUOTATION MARK
                "‛",  // U+201B: SINGLE HIGH-REVERSED-9 QUOTATION MARK
                "′",  // U+2032: PRIME
                "‵",  // U+2035: REVERSED PRIME
                "‹",  // U+2039: SINGLE LEFT-POINTING ANGLE QUOTATION MARK
                "›",  // U+203A: SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
                "❛",  // U+275B: HEAVY SINGLE TURNED COMMA QUOTATION MARK ORNAMENT
                "❜",  // U+275C: HEAVY SINGLE COMMA QUOTATION MARK ORNAMENT
                "＇", // U+FF07: FULLWIDTH APOSTROPHE
            ],
            "'",
        ),
        (
            &[
                "‐",  // U+2010: HYPHEN
                "‑",  // U+2011: NON-BREAKING HYPHEN
                "‒",  // U+2012: FIGURE DASH
                "–",  // U+2013: EN DASH
                "—",  // U+2014: EM DASH
                "⁻",  // U+207B: SUPERSCRIPT MINUS
                "₋",  // U+208B: SUBSCRIPT MINUS
                "－", // U+FF0D: FULLWIDTH HYPHEN-MINUS
            ],
            "-",
        ),
        (
            &[
                "⁅",  // U+2045: LEFT SQUARE BRACKET WITH QUILL
                "❲",  // U+2772: LIGHT LEFT TORTOISE SHELL BRACKET ORNAMENT
                "［", // U+FF3B: FULLWIDTH LEFT SQUARE BRACKET
            ],
            "[",
        ),
        (
            &[
                "⁆",  // U+2046: RIGHT SQUARE BRACKET WITH QUILL
                "❳",  // U+2773: LIGHT RIGHT TORTOISE SHELL BRACKET ORNAMENT
                "］", // U+FF3D: FULLWIDTH RIGHT SQUARE BRACKET
            ],
            "]",
        ),
        (
            &[
                "⁽",  // U+207D: SUPERSCRIPT LEFT PARENTHESIS
                "₍",  // U+208D: SUBSCRIPT LEFT PARENTHESIS
                "❨",  // U+2768: MEDIUM LEFT PARENTHESIS ORNAMENT
                "❪",  // U+276A: MEDIUM FLATTENED LEFT PARENTHESIS ORNAMENT
                "（", // U+FF08: FULLWIDTH LEFT PARENTHESIS
            ],
            "(",
        ),
        (
            &[
                "⸨", // U+2E28: LEFT DOUBLE PARENTHESIS
            ],
            "((",
        ),
        (
            &[
                "⁾",  // U+207E: SUPERSCRIPT RIGHT PARENTHESIS
                "₎",  // U+208E: SUBSCRIPT RIGHT PARENTHESIS
                "❩",  // U+2769: MEDIUM RIGHT PARENTHESIS ORNAMENT
                "❫",  // U+276B: MEDIUM FLATTENED RIGHT PARENTHESIS ORNAMENT
                "）", // U+FF09: FULLWIDTH RIGHT PARENTHESIS
            ],
            ")",
        ),
        (
            &[
                "⸩", // U+2E29: RIGHT DOUBLE PARENTHESIS
            ],
            "))",
        ),
        (
            &[
                "❬",  // U+276C: MEDIUM LEFT-POINTING ANGLE BRACKET ORNAMENT
                "❰",  // U+2770: HEAVY LEFT-POINTING ANGLE BRACKET ORNAMENT
                "＜", // U+FF1C: FULLWIDTH LESS-THAN SIGN
            ],
            "<",
        ),
        (
            &[
                "❭",  // U+276D: MEDIUM RIGHT-POINTING ANGLE BRACKET ORNAMENT
                "❱",  // U+2771: HEAVY RIGHT-POINTING ANGLE BRACKET ORNAMENT
                "＞", // U+FF1E: FULLWIDTH GREATER-THAN SIGN
            ],
            ">",
        ),
        (
            &[
                "❴",  // U+2774: MEDIUM LEFT CURLY BRACKET ORNAMENT
                "｛", // U+FF5B: FULLWIDTH LEFT CURLY BRACKET
            ],
            "{",
        ),
        (
            &[
                "❵",  // U+2775: MEDIUM RIGHT CURLY BRACKET ORNAMENT
                "｝", // U+FF5D: FULLWIDTH RIGHT CURLY BRACKET
            ],
            "}",
        ),
        (
            &[
                "⁺",  // U+207A: SUPERSCRIPT PLUS SIGN
                "₊",  // U+208A: SUBSCRIPT PLUS SIGN
                "＋", // U+FF0B: FULLWIDTH PLUS SIGN
            ],
            "+",
        ),
        (
            &[
                "⁼",  // U+207C: SUPERSCRIPT EQUALS SIGN
                "₌",  // U+208C: SUBSCRIPT EQUALS SIGN
                "＝", // U+FF1D: FULLWIDTH EQUALS SIGN
            ],
            "=",
        ),
        (
            &[
                "！", // U+FF01: FULLWIDTH EXCLAMATION MARK
            ],
            "!",
        ),
        (
            &[
                "‼", // U+203C: DOUBLE EXCLAMATION MARK
            ],
            "!!",
        ),
        (
            &[
                "⁉", // U+2049: EXCLAMATION QUESTION MARK
            ],
            "!?",
        ),
        (
            &[
                "＃", // U+FF03: FULLWIDTH NUMBER SIGN
            ],
            "#",
        ),
        (
            &[
                "＄", // U+FF04: FULLWIDTH DOLLAR SIGN
            ],
            "$",
        ),
        (
            &[
                "⁒",  // U+2052: COMMERCIAL MINUS SIGN
                "％", // U+FF05: FULLWIDTH PERCENT SIGN
            ],
            "%",
        ),
        (
            &[
                "＆", // U+FF06: FULLWIDTH AMPERSAND
            ],
            "&",
        ),
        (
            &[
                "⁎",  // U+204E: LOW ASTERISK
                "＊", // U+FF0A: FULLWIDTH ASTERISK
            ],
            "*",
        ),
        (
            &[
                "，", // U+FF0C: FULLWIDTH COMMA
            ],
            ",",
        ),
        (
            &[
                "．", // U+FF0E: FULLWIDTH FULL STOP
            ],
            ".",
        ),
        (
            &[
                "⁄",  // U+2044: FRACTION SLASH
                "／", // U+FF0F: FULLWIDTH SOLIDUS
            ],
            "/",
        ),
        (
            &[
                "：", // U+FF1A: FULLWIDTH COLON
            ],
            ":",
        ),
        (
            &[
                "⁏",  // U+204F: REVERSED SEMICOLON
                "；", // U+FF1B: FULLWIDTH SEMICOLON
            ],
            ";",
        ),
        (
            &[
                "？", // U+FF1F: FULLWIDTH QUESTION MARK
            ],
            "?",
        ),
        (
            &[
                "⁇", // U+2047: DOUBLE QUESTION MARK
            ],
            "??",
        ),
        (
            &[
                "⁈", // U+2048: QUESTION EXCLAMATION MARK
            ],
            "?!",
        ),
        (
            &[
                "＠", // U+FF20: FULLWIDTH COMMERCIAL AT
            ],
            "@",
        ),
        (
            &[
                "＼", // U+FF3C: FULLWIDTH REVERSE SOLIDUS
            ],
            "\\",
        ),
        (
            &[
                "‸",  // U+2038: CARET
                "＾", // U+FF3E: FULLWIDTH CIRCUMFLEX ACCENT
            ],
            "^",
        ),
        (
            &[
                "＿", // U+FF3F: FULLWIDTH LOW LINE
            ],
            "_",
        ),
        (
            &[
                "⁓",  // U+2053: SWUNG DASH
                "～", // U+FF5E: FULLWIDTH TILDE
            ],
            "~",
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
