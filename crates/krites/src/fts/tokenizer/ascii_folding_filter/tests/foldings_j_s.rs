//! ASCII folding tests for letters J through S.

use super::folding_using_raw_tokenizer_helper;

#[test]
fn test_all_foldings_j_through_s() {
    let foldings: Vec<(&[&str], &str)> = vec![
        (
            &[
                "Ĵ",  // U+0134: LATIN CAPITAL LETTER J WITH CIRCUMFLEX
                "Ɉ",  // U+0248: LATIN CAPITAL LETTER J WITH STROKE
                "ᴊ",  // U+1D0A: LATIN LETTER SMALL CAPITAL J
                "Ⓙ",  // U+24BF: CIRCLED LATIN CAPITAL LETTER J
                "Ｊ", // U+FF2A: FULLWIDTH LATIN CAPITAL LETTER J
            ],
            "J",
        ),
        (
            &[
                "ĵ",  // U+0135: LATIN SMALL LETTER J WITH CIRCUMFLEX
                "ǰ",  // U+01F0: LATIN SMALL LETTER J WITH CARON
                "ȷ",  // U+0237: LATIN SMALL LETTER DOTLESS J
                "ɉ",  // U+0249: LATIN SMALL LETTER J WITH STROKE
                "ɟ",  // U+025F: LATIN SMALL LETTER DOTLESS J WITH STROKE
                "ʄ",  // U+0284: LATIN SMALL LETTER DOTLESS J WITH STROKE AND HOOK
                "ʝ",  // U+029D: LATIN SMALL LETTER J WITH CROSSED-TAIL
                "ⓙ",  // U+24D9: CIRCLED LATIN SMALL LETTER J
                "ⱼ",  // U+2C7C: LATIN SUBSCRIPT SMALL LETTER J
                "ｊ", // U+FF4A: FULLWIDTH LATIN SMALL LETTER J
            ],
            "j",
        ),
        (
            &[
                "⒥", // U+24A5: PARENTHESIZED LATIN SMALL LETTER J
            ],
            "(j)",
        ),
        (
            &[
                "Ķ",  // U+0136: LATIN CAPITAL LETTER K WITH CEDILLA
                "Ƙ",  // U+0198: LATIN CAPITAL LETTER K WITH HOOK
                "Ǩ",  // U+01E8: LATIN CAPITAL LETTER K WITH CARON
                "ᴋ",  // U+1D0B: LATIN LETTER SMALL CAPITAL K
                "Ḱ",  // U+1E30: LATIN CAPITAL LETTER K WITH ACUTE
                "Ḳ",  // U+1E32: LATIN CAPITAL LETTER K WITH DOT BELOW
                "Ḵ",  // U+1E34: LATIN CAPITAL LETTER K WITH LINE BELOW
                "Ⓚ",  // U+24C0: CIRCLED LATIN CAPITAL LETTER K
                "Ⱪ",  // U+2C69: LATIN CAPITAL LETTER K WITH DESCENDER
                "Ꝁ",  // U+A740: LATIN CAPITAL LETTER K WITH STROKE
                "Ꝃ",  // U+A742: LATIN CAPITAL LETTER K WITH DIAGONAL STROKE
                "Ꝅ",  // U+A744: LATIN CAPITAL LETTER K WITH STROKE AND DIAGONAL STROKE
                "Ｋ", // U+FF2B: FULLWIDTH LATIN CAPITAL LETTER K
            ],
            "K",
        ),
        (
            &[
                "ķ",  // U+0137: LATIN SMALL LETTER K WITH CEDILLA
                "ƙ",  // U+0199: LATIN SMALL LETTER K WITH HOOK
                "ǩ",  // U+01E9: LATIN SMALL LETTER K WITH CARON
                "ʞ",  // U+029E: LATIN SMALL LETTER TURNED K
                "ᶄ",  // U+1D84: LATIN SMALL LETTER K WITH PALATAL HOOK
                "ḱ",  // U+1E31: LATIN SMALL LETTER K WITH ACUTE
                "ḳ",  // U+1E33: LATIN SMALL LETTER K WITH DOT BELOW
                "ḵ",  // U+1E35: LATIN SMALL LETTER K WITH LINE BELOW
                "ⓚ",  // U+24DA: CIRCLED LATIN SMALL LETTER K
                "ⱪ",  // U+2C6A: LATIN SMALL LETTER K WITH DESCENDER
                "ꝁ",  // U+A741: LATIN SMALL LETTER K WITH STROKE
                "ꝃ",  // U+A743: LATIN SMALL LETTER K WITH DIAGONAL STROKE
                "ꝅ",  // U+A745: LATIN SMALL LETTER K WITH STROKE AND DIAGONAL STROKE
                "ｋ", // U+FF4B: FULLWIDTH LATIN SMALL LETTER K
            ],
            "k",
        ),
        (
            &[
                "⒦", // U+24A6: PARENTHESIZED LATIN SMALL LETTER K
            ],
            "(k)",
        ),
        (
            &[
                "Ĺ",  // U+0139: LATIN CAPITAL LETTER L WITH ACUTE
                "Ļ",  // U+013B: LATIN CAPITAL LETTER L WITH CEDILLA
                "Ľ",  // U+013D: LATIN CAPITAL LETTER L WITH CARON
                "Ŀ",  // U+013F: LATIN CAPITAL LETTER L WITH MIDDLE DOT
                "Ł",  // U+0141: LATIN CAPITAL LETTER L WITH STROKE
                "Ƚ",  // U+023D: LATIN CAPITAL LETTER L WITH BAR
                "ʟ",  // U+029F: LATIN LETTER SMALL CAPITAL L
                "ᴌ",  // U+1D0C: LATIN LETTER SMALL CAPITAL L WITH STROKE
                "Ḷ",  // U+1E36: LATIN CAPITAL LETTER L WITH DOT BELOW
                "Ḹ",  // U+1E38: LATIN CAPITAL LETTER L WITH DOT BELOW AND MACRON
                "Ḻ",  // U+1E3A: LATIN CAPITAL LETTER L WITH LINE BELOW
                "Ḽ",  // U+1E3C: LATIN CAPITAL LETTER L WITH CIRCUMFLEX BELOW
                "Ⓛ",  // U+24C1: CIRCLED LATIN CAPITAL LETTER L
                "Ⱡ",  // U+2C60: LATIN CAPITAL LETTER L WITH DOUBLE BAR
                "Ɫ",  // U+2C62: LATIN CAPITAL LETTER L WITH MIDDLE TILDE
                "Ꝇ",  // U+A746: LATIN CAPITAL LETTER BROKEN L
                "Ꝉ",  // U+A748: LATIN CAPITAL LETTER L WITH HIGH STROKE
                "Ꞁ",  // U+A780: LATIN CAPITAL LETTER TURNED L
                "Ｌ", // U+FF2C: FULLWIDTH LATIN CAPITAL LETTER L
            ],
            "L",
        ),
        (
            &[
                "ĺ",  // U+013A: LATIN SMALL LETTER L WITH ACUTE
                "ļ",  // U+013C: LATIN SMALL LETTER L WITH CEDILLA
                "ľ",  // U+013E: LATIN SMALL LETTER L WITH CARON
                "ŀ",  // U+0140: LATIN SMALL LETTER L WITH MIDDLE DOT
                "ł",  // U+0142: LATIN SMALL LETTER L WITH STROKE
                "ƚ",  // U+019A: LATIN SMALL LETTER L WITH BAR
                "ȴ",  // U+0234: LATIN SMALL LETTER L WITH CURL
                "ɫ",  // U+026B: LATIN SMALL LETTER L WITH MIDDLE TILDE
                "ɬ",  // U+026C: LATIN SMALL LETTER L WITH BELT
                "ɭ",  // U+026D: LATIN SMALL LETTER L WITH RETROFLEX HOOK
                "ᶅ",  // U+1D85: LATIN SMALL LETTER L WITH PALATAL HOOK
                "ḷ",  // U+1E37: LATIN SMALL LETTER L WITH DOT BELOW
                "ḹ",  // U+1E39: LATIN SMALL LETTER L WITH DOT BELOW AND MACRON
                "ḻ",  // U+1E3B: LATIN SMALL LETTER L WITH LINE BELOW
                "ḽ",  // U+1E3D: LATIN SMALL LETTER L WITH CIRCUMFLEX BELOW
                "ⓛ",  // U+24DB: CIRCLED LATIN SMALL LETTER L
                "ⱡ",  // U+2C61: LATIN SMALL LETTER L WITH DOUBLE BAR
                "ꝇ",  // U+A747: LATIN SMALL LETTER BROKEN L
                "ꝉ",  // U+A749: LATIN SMALL LETTER L WITH HIGH STROKE
                "ꞁ",  // U+A781: LATIN SMALL LETTER TURNED L
                "ｌ", // U+FF4C: FULLWIDTH LATIN SMALL LETTER L
            ],
            "l",
        ),
        (
            &[
                "Ǉ", // U+01C7: LATIN CAPITAL LETTER LJ
            ],
            "LJ",
        ),
        (
            &[
                "Ỻ", // U+1EFA: LATIN CAPITAL LETTER MIDDLE-WELSH LL
            ],
            "LL",
        ),
        (
            &[
                "ǈ", // U+01C8: LATIN CAPITAL LETTER L WITH SMALL LETTER J
            ],
            "Lj",
        ),
        (
            &[
                "⒧", // U+24A7: PARENTHESIZED LATIN SMALL LETTER L
            ],
            "(l)",
        ),
        (
            &[
                "ǉ", // U+01C9: LATIN SMALL LETTER LJ
            ],
            "lj",
        ),
        (
            &[
                "ỻ", // U+1EFB: LATIN SMALL LETTER MIDDLE-WELSH LL
            ],
            "ll",
        ),
        (
            &[
                "ʪ", // U+02AA: LATIN SMALL LETTER LS DIGRAPH
            ],
            "ls",
        ),
        (
            &[
                "ʫ", // U+02AB: LATIN SMALL LETTER LZ DIGRAPH
            ],
            "lz",
        ),
        (
            &[
                "Ɯ",  // U+019C: LATIN CAPITAL LETTER TURNED M
                "ᴍ",  // U+1D0D: LATIN LETTER SMALL CAPITAL M
                "Ḿ",  // U+1E3E: LATIN CAPITAL LETTER M WITH ACUTE
                "Ṁ",  // U+1E40: LATIN CAPITAL LETTER M WITH DOT ABOVE
                "Ṃ",  // U+1E42: LATIN CAPITAL LETTER M WITH DOT BELOW
                "Ⓜ",  // U+24C2: CIRCLED LATIN CAPITAL LETTER M
                "Ɱ",  // U+2C6E: LATIN CAPITAL LETTER M WITH HOOK
                "ꟽ",  // U+A7FD: LATIN EPIGRAPHIC LETTER INVERTED M
                "ꟿ",  // U+A7FF: LATIN EPIGRAPHIC LETTER ARCHAIC M
                "Ｍ", // U+FF2D: FULLWIDTH LATIN CAPITAL LETTER M
            ],
            "M",
        ),
        (
            &[
                "ɯ",  // U+026F: LATIN SMALL LETTER TURNED M
                "ɰ",  // U+0270: LATIN SMALL LETTER TURNED M WITH LONG LEG
                "ɱ",  // U+0271: LATIN SMALL LETTER M WITH HOOK
                "ᵯ",  // U+1D6F: LATIN SMALL LETTER M WITH MIDDLE TILDE
                "ᶆ",  // U+1D86: LATIN SMALL LETTER M WITH PALATAL HOOK
                "ḿ",  // U+1E3F: LATIN SMALL LETTER M WITH ACUTE
                "ṁ",  // U+1E41: LATIN SMALL LETTER M WITH DOT ABOVE
                "ṃ",  // U+1E43: LATIN SMALL LETTER M WITH DOT BELOW
                "ⓜ",  // U+24DC: CIRCLED LATIN SMALL LETTER M
                "ｍ", // U+FF4D: FULLWIDTH LATIN SMALL LETTER M
            ],
            "m",
        ),
        (
            &[
                "⒨", // U+24A8: PARENTHESIZED LATIN SMALL LETTER M
            ],
            "(m)",
        ),
        (
            &[
                "Ñ",  // U+00D1: LATIN CAPITAL LETTER N WITH TILDE
                "Ń",  // U+0143: LATIN CAPITAL LETTER N WITH ACUTE
                "Ņ",  // U+0145: LATIN CAPITAL LETTER N WITH CEDILLA
                "Ň",  // U+0147: LATIN CAPITAL LETTER N WITH CARON
                "Ŋ",  // U+014A: LATIN CAPITAL LETTER ENG
                "Ɲ",  // U+019D: LATIN CAPITAL LETTER N WITH LEFT HOOK
                "Ǹ",  // U+01F8: LATIN CAPITAL LETTER N WITH GRAVE
                "Ƞ",  // U+0220: LATIN CAPITAL LETTER N WITH LONG RIGHT LEG
                "ɴ",  // U+0274: LATIN LETTER SMALL CAPITAL N
                "ᴎ",  // U+1D0E: LATIN LETTER SMALL CAPITAL REVERSED N
                "Ṅ",  // U+1E44: LATIN CAPITAL LETTER N WITH DOT ABOVE
                "Ṇ",  // U+1E46: LATIN CAPITAL LETTER N WITH DOT BELOW
                "Ṉ",  // U+1E48: LATIN CAPITAL LETTER N WITH LINE BELOW
                "Ṋ",  // U+1E4A: LATIN CAPITAL LETTER N WITH CIRCUMFLEX BELOW
                "Ⓝ",  // U+24C3: CIRCLED LATIN CAPITAL LETTER N
                "Ｎ", // U+FF2E: FULLWIDTH LATIN CAPITAL LETTER N
            ],
            "N",
        ),
        (
            &[
                "ñ",  // U+00F1: LATIN SMALL LETTER N WITH TILDE
                "ń",  // U+0144: LATIN SMALL LETTER N WITH ACUTE
                "ņ",  // U+0146: LATIN SMALL LETTER N WITH CEDILLA
                "ň",  // U+0148: LATIN SMALL LETTER N WITH CARON
                "ŉ",  // U+0149: LATIN SMALL LETTER N PRECEDED BY APOSTROPHE
                "ŋ",  // U+014B: LATIN SMALL LETTER ENG
                "ƞ",  // U+019E: LATIN SMALL LETTER N WITH LONG RIGHT LEG
                "ǹ",  // U+01F9: LATIN SMALL LETTER N WITH GRAVE
                "ȵ",  // U+0235: LATIN SMALL LETTER N WITH CURL
                "ɲ",  // U+0272: LATIN SMALL LETTER N WITH LEFT HOOK
                "ɳ",  // U+0273: LATIN SMALL LETTER N WITH RETROFLEX HOOK
                "ᵰ",  // U+1D70: LATIN SMALL LETTER N WITH MIDDLE TILDE
                "ᶇ",  // U+1D87: LATIN SMALL LETTER N WITH PALATAL HOOK
                "ṅ",  // U+1E45: LATIN SMALL LETTER N WITH DOT ABOVE
                "ṇ",  // U+1E47: LATIN SMALL LETTER N WITH DOT BELOW
                "ṉ",  // U+1E49: LATIN SMALL LETTER N WITH LINE BELOW
                "ṋ",  // U+1E4B: LATIN SMALL LETTER N WITH CIRCUMFLEX BELOW
                "ⁿ",  // U+207F: SUPERSCRIPT LATIN SMALL LETTER N
                "ⓝ",  // U+24DD: CIRCLED LATIN SMALL LETTER N
                "ｎ", // U+FF4E: FULLWIDTH LATIN SMALL LETTER N
            ],
            "n",
        ),
        (
            &[
                "Ǌ", // U+01CA: LATIN CAPITAL LETTER NJ
            ],
            "NJ",
        ),
        (
            &[
                "ǋ", // U+01CB: LATIN CAPITAL LETTER N WITH SMALL LETTER J
            ],
            "Nj",
        ),
        (
            &[
                "⒩", // U+24A9: PARENTHESIZED LATIN SMALL LETTER N
            ],
            "(n)",
        ),
        (
            &[
                "ǌ", // U+01CC: LATIN SMALL LETTER NJ
            ],
            "nj",
        ),
        (
            &[
                "Ò",  // U+00D2: LATIN CAPITAL LETTER O WITH GRAVE
                "Ó",  // U+00D3: LATIN CAPITAL LETTER O WITH ACUTE
                "Ô",  // U+00D4: LATIN CAPITAL LETTER O WITH CIRCUMFLEX
                "Õ",  // U+00D5: LATIN CAPITAL LETTER O WITH TILDE
                "Ö",  // U+00D6: LATIN CAPITAL LETTER O WITH DIAERESIS
                "Ø",  // U+00D8: LATIN CAPITAL LETTER O WITH STROKE
                "Ō",  // U+014C: LATIN CAPITAL LETTER O WITH MACRON
                "Ŏ",  // U+014E: LATIN CAPITAL LETTER O WITH BREVE
                "Ő",  // U+0150: LATIN CAPITAL LETTER O WITH DOUBLE ACUTE
                "Ɔ",  // U+0186: LATIN CAPITAL LETTER OPEN O
                "Ɵ",  // U+019F: LATIN CAPITAL LETTER O WITH MIDDLE TILDE
                "Ơ",  // U+01A0: LATIN CAPITAL LETTER O WITH HORN
                "Ǒ",  // U+01D1: LATIN CAPITAL LETTER O WITH CARON
                "Ǫ",  // U+01EA: LATIN CAPITAL LETTER O WITH OGONEK
                "Ǭ",  // U+01EC: LATIN CAPITAL LETTER O WITH OGONEK AND MACRON
                "Ǿ",  // U+01FE: LATIN CAPITAL LETTER O WITH STROKE AND ACUTE
                "Ȍ",  // U+020C: LATIN CAPITAL LETTER O WITH DOUBLE GRAVE
                "Ȏ",  // U+020E: LATIN CAPITAL LETTER O WITH INVERTED BREVE
                "Ȫ",  // U+022A: LATIN CAPITAL LETTER O WITH DIAERESIS AND MACRON
                "Ȭ",  // U+022C: LATIN CAPITAL LETTER O WITH TILDE AND MACRON
                "Ȯ",  // U+022E: LATIN CAPITAL LETTER O WITH DOT ABOVE
                "Ȱ",  // U+0230: LATIN CAPITAL LETTER O WITH DOT ABOVE AND MACRON
                "ᴏ",  // U+1D0F: LATIN LETTER SMALL CAPITAL O
                "ᴐ",  // U+1D10: LATIN LETTER SMALL CAPITAL OPEN O
                "Ṍ",  // U+1E4C: LATIN CAPITAL LETTER O WITH TILDE AND ACUTE
                "Ṏ",  // U+1E4E: LATIN CAPITAL LETTER O WITH TILDE AND DIAERESIS
                "Ṑ",  // U+1E50: LATIN CAPITAL LETTER O WITH MACRON AND GRAVE
                "Ṓ",  // U+1E52: LATIN CAPITAL LETTER O WITH MACRON AND ACUTE
                "Ọ",  // U+1ECC: LATIN CAPITAL LETTER O WITH DOT BELOW
                "Ỏ",  // U+1ECE: LATIN CAPITAL LETTER O WITH HOOK ABOVE
                "Ố",  // U+1ED0: LATIN CAPITAL LETTER O WITH CIRCUMFLEX AND ACUTE
                "Ồ",  // U+1ED2: LATIN CAPITAL LETTER O WITH CIRCUMFLEX AND GRAVE
                "Ổ",  // U+1ED4: LATIN CAPITAL LETTER O WITH CIRCUMFLEX AND HOOK ABOVE
                "Ỗ",  // U+1ED6: LATIN CAPITAL LETTER O WITH CIRCUMFLEX AND TILDE
                "Ộ",  // U+1ED8: LATIN CAPITAL LETTER O WITH CIRCUMFLEX AND DOT BELOW
                "Ớ",  // U+1EDA: LATIN CAPITAL LETTER O WITH HORN AND ACUTE
                "Ờ",  // U+1EDC: LATIN CAPITAL LETTER O WITH HORN AND GRAVE
                "Ở",  // U+1EDE: LATIN CAPITAL LETTER O WITH HORN AND HOOK ABOVE
                "Ỡ",  // U+1EE0: LATIN CAPITAL LETTER O WITH HORN AND TILDE
                "Ợ",  // U+1EE2: LATIN CAPITAL LETTER O WITH HORN AND DOT BELOW
                "Ⓞ",  // U+24C4: CIRCLED LATIN CAPITAL LETTER O
                "Ꝋ",  // U+A74A: LATIN CAPITAL LETTER O WITH LONG STROKE OVERLAY
                "Ꝍ",  // U+A74C: LATIN CAPITAL LETTER O WITH LOOP
                "Ｏ", // U+FF2F: FULLWIDTH LATIN CAPITAL LETTER O
            ],
            "O",
        ),
        (
            &[
                "ò",  // U+00F2: LATIN SMALL LETTER O WITH GRAVE
                "ó",  // U+00F3: LATIN SMALL LETTER O WITH ACUTE
                "ô",  // U+00F4: LATIN SMALL LETTER O WITH CIRCUMFLEX
                "õ",  // U+00F5: LATIN SMALL LETTER O WITH TILDE
                "ö",  // U+00F6: LATIN SMALL LETTER O WITH DIAERESIS
                "ø",  // U+00F8: LATIN SMALL LETTER O WITH STROKE
                "ō",  // U+014D: LATIN SMALL LETTER O WITH MACRON
                "ŏ",  // U+014F: LATIN SMALL LETTER O WITH BREVE
                "ő",  // U+0151: LATIN SMALL LETTER O WITH DOUBLE ACUTE
                "ơ",  // U+01A1: LATIN SMALL LETTER O WITH HORN
                "ǒ",  // U+01D2: LATIN SMALL LETTER O WITH CARON
                "ǫ",  // U+01EB: LATIN SMALL LETTER O WITH OGONEK
                "ǭ",  // U+01ED: LATIN SMALL LETTER O WITH OGONEK AND MACRON
                "ǿ",  // U+01FF: LATIN SMALL LETTER O WITH STROKE AND ACUTE
                "ȍ",  // U+020D: LATIN SMALL LETTER O WITH DOUBLE GRAVE
                "ȏ",  // U+020F: LATIN SMALL LETTER O WITH INVERTED BREVE
                "ȫ",  // U+022B: LATIN SMALL LETTER O WITH DIAERESIS AND MACRON
                "ȭ",  // U+022D: LATIN SMALL LETTER O WITH TILDE AND MACRON
                "ȯ",  // U+022F: LATIN SMALL LETTER O WITH DOT ABOVE
                "ȱ",  // U+0231: LATIN SMALL LETTER O WITH DOT ABOVE AND MACRON
                "ɔ",  // U+0254: LATIN SMALL LETTER OPEN O
                "ɵ",  // U+0275: LATIN SMALL LETTER BARRED O
                "ᴖ",  // U+1D16: LATIN SMALL LETTER TOP HALF O
                "ᴗ",  // U+1D17: LATIN SMALL LETTER BOTTOM HALF O
                "ᶗ",  // U+1D97: LATIN SMALL LETTER OPEN O WITH RETROFLEX HOOK
                "ṍ",  // U+1E4D: LATIN SMALL LETTER O WITH TILDE AND ACUTE
                "ṏ",  // U+1E4F: LATIN SMALL LETTER O WITH TILDE AND DIAERESIS
                "ṑ",  // U+1E51: LATIN SMALL LETTER O WITH MACRON AND GRAVE
                "ṓ",  // U+1E53: LATIN SMALL LETTER O WITH MACRON AND ACUTE
                "ọ",  // U+1ECD: LATIN SMALL LETTER O WITH DOT BELOW
                "ỏ",  // U+1ECF: LATIN SMALL LETTER O WITH HOOK ABOVE
                "ố",  // U+1ED1: LATIN SMALL LETTER O WITH CIRCUMFLEX AND ACUTE
                "ồ",  // U+1ED3: LATIN SMALL LETTER O WITH CIRCUMFLEX AND GRAVE
                "ổ",  // U+1ED5: LATIN SMALL LETTER O WITH CIRCUMFLEX AND HOOK ABOVE
                "ỗ",  // U+1ED7: LATIN SMALL LETTER O WITH CIRCUMFLEX AND TILDE
                "ộ",  // U+1ED9: LATIN SMALL LETTER O WITH CIRCUMFLEX AND DOT BELOW
                "ớ",  // U+1EDB: LATIN SMALL LETTER O WITH HORN AND ACUTE
                "ờ",  // U+1EDD: LATIN SMALL LETTER O WITH HORN AND GRAVE
                "ở",  // U+1EDF: LATIN SMALL LETTER O WITH HORN AND HOOK ABOVE
                "ỡ",  // U+1EE1: LATIN SMALL LETTER O WITH HORN AND TILDE
                "ợ",  // U+1EE3: LATIN SMALL LETTER O WITH HORN AND DOT BELOW
                "ₒ",  // U+2092: LATIN SUBSCRIPT SMALL LETTER O
                "ⓞ",  // U+24DE: CIRCLED LATIN SMALL LETTER O
                "ⱺ",  // U+2C7A: LATIN SMALL LETTER O WITH LOW RING INSIDE
                "ꝋ",  // U+A74B: LATIN SMALL LETTER O WITH LONG STROKE OVERLAY
                "ꝍ",  // U+A74D: LATIN SMALL LETTER O WITH LOOP
                "ｏ", // U+FF4F: FULLWIDTH LATIN SMALL LETTER O
            ],
            "o",
        ),
        (
            &[
                "Œ", // U+0152: LATIN CAPITAL LIGATURE OE
                "ɶ", // U+0276: LATIN LETTER SMALL CAPITAL OE
            ],
            "OE",
        ),
        (
            &[
                "Ꝏ", // U+A74E: LATIN CAPITAL LETTER OO
            ],
            "OO",
        ),
        (
            &[
                "Ȣ", // U+0222: LATIN CAPITAL LETTER OU
                "ᴕ", // U+1D15: LATIN LETTER SMALL CAPITAL OU
            ],
            "OU",
        ),
        (
            &[
                "⒪", // U+24AA: PARENTHESIZED LATIN SMALL LETTER O
            ],
            "(o)",
        ),
        (
            &[
                "œ", // U+0153: LATIN SMALL LIGATURE OE
                "ᴔ", // U+1D14: LATIN SMALL LETTER TURNED OE
            ],
            "oe",
        ),
        (
            &[
                "ꝏ", // U+A74F: LATIN SMALL LETTER OO
            ],
            "oo",
        ),
        (
            &[
                "ȣ", // U+0223: LATIN SMALL LETTER OU
            ],
            "ou",
        ),
        (
            &[
                "Ƥ",  // U+01A4: LATIN CAPITAL LETTER P WITH HOOK
                "ᴘ",  // U+1D18: LATIN LETTER SMALL CAPITAL P
                "Ṕ",  // U+1E54: LATIN CAPITAL LETTER P WITH ACUTE
                "Ṗ",  // U+1E56: LATIN CAPITAL LETTER P WITH DOT ABOVE
                "Ⓟ",  // U+24C5: CIRCLED LATIN CAPITAL LETTER P
                "Ᵽ",  // U+2C63: LATIN CAPITAL LETTER P WITH STROKE
                "Ꝑ",  // U+A750: LATIN CAPITAL LETTER P WITH STROKE THROUGH DESCENDER
                "Ꝓ",  // U+A752: LATIN CAPITAL LETTER P WITH FLOURISH
                "Ꝕ",  // U+A754: LATIN CAPITAL LETTER P WITH SQUIRREL TAIL
                "Ｐ", // U+FF30: FULLWIDTH LATIN CAPITAL LETTER P
            ],
            "P",
        ),
        (
            &[
                "ƥ",  // U+01A5: LATIN SMALL LETTER P WITH HOOK
                "ᵱ",  // U+1D71: LATIN SMALL LETTER P WITH MIDDLE TILDE
                "ᵽ",  // U+1D7D: LATIN SMALL LETTER P WITH STROKE
                "ᶈ",  // U+1D88: LATIN SMALL LETTER P WITH PALATAL HOOK
                "ṕ",  // U+1E55: LATIN SMALL LETTER P WITH ACUTE
                "ṗ",  // U+1E57: LATIN SMALL LETTER P WITH DOT ABOVE
                "ⓟ",  // U+24DF: CIRCLED LATIN SMALL LETTER P
                "ꝑ",  // U+A751: LATIN SMALL LETTER P WITH STROKE THROUGH DESCENDER
                "ꝓ",  // U+A753: LATIN SMALL LETTER P WITH FLOURISH
                "ꝕ",  // U+A755: LATIN SMALL LETTER P WITH SQUIRREL TAIL
                "ꟼ",  // U+A7FC: LATIN EPIGRAPHIC LETTER REVERSED P
                "ｐ", // U+FF50: FULLWIDTH LATIN SMALL LETTER P
            ],
            "p",
        ),
        (
            &[
                "⒫", // U+24AB: PARENTHESIZED LATIN SMALL LETTER P
            ],
            "(p)",
        ),
        (
            &[
                "Ɋ",  // U+024A: LATIN CAPITAL LETTER SMALL Q WITH HOOK TAIL
                "Ⓠ",  // U+24C6: CIRCLED LATIN CAPITAL LETTER Q
                "Ꝗ",  // U+A756: LATIN CAPITAL LETTER Q WITH STROKE THROUGH DESCENDER
                "Ꝙ",  // U+A758: LATIN CAPITAL LETTER Q WITH DIAGONAL STROKE
                "Ｑ", // U+FF31: FULLWIDTH LATIN CAPITAL LETTER Q
            ],
            "Q",
        ),
        (
            &[
                "ĸ",  // U+0138: LATIN SMALL LETTER KRA
                "ɋ",  // U+024B: LATIN SMALL LETTER Q WITH HOOK TAIL
                "ʠ",  // U+02A0: LATIN SMALL LETTER Q WITH HOOK
                "ⓠ",  // U+24E0: CIRCLED LATIN SMALL LETTER Q
                "ꝗ",  // U+A757: LATIN SMALL LETTER Q WITH STROKE THROUGH DESCENDER
                "ꝙ",  // U+A759: LATIN SMALL LETTER Q WITH DIAGONAL STROKE
                "ｑ", // U+FF51: FULLWIDTH LATIN SMALL LETTER Q
            ],
            "q",
        ),
        (
            &[
                "⒬", // U+24AC: PARENTHESIZED LATIN SMALL LETTER Q
            ],
            "(q)",
        ),
        (
            &[
                "ȹ", // U+0239: LATIN SMALL LETTER QP DIGRAPH
            ],
            "qp",
        ),
        (
            &[
                "Ŕ",  // U+0154: LATIN CAPITAL LETTER R WITH ACUTE
                "Ŗ",  // U+0156: LATIN CAPITAL LETTER R WITH CEDILLA
                "Ř",  // U+0158: LATIN CAPITAL LETTER R WITH CARON
                "Ȑ",  // U+0210: LATIN CAPITAL LETTER R WITH DOUBLE GRAVE
                "Ȓ",  // U+0212: LATIN CAPITAL LETTER R WITH INVERTED BREVE
                "Ɍ",  // U+024C: LATIN CAPITAL LETTER R WITH STROKE
                "ʀ",  // U+0280: LATIN LETTER SMALL CAPITAL R
                "ʁ",  // U+0281: LATIN LETTER SMALL CAPITAL INVERTED R
                "ᴙ",  // U+1D19: LATIN LETTER SMALL CAPITAL REVERSED R
                "ᴚ",  // U+1D1A: LATIN LETTER SMALL CAPITAL TURNED R
                "Ṙ",  // U+1E58: LATIN CAPITAL LETTER R WITH DOT ABOVE
                "Ṛ",  // U+1E5A: LATIN CAPITAL LETTER R WITH DOT BELOW
                "Ṝ",  // U+1E5C: LATIN CAPITAL LETTER R WITH DOT BELOW AND MACRON
                "Ṟ",  // U+1E5E: LATIN CAPITAL LETTER R WITH LINE BELOW
                "Ⓡ",  // U+24C7: CIRCLED LATIN CAPITAL LETTER R
                "Ɽ",  // U+2C64: LATIN CAPITAL LETTER R WITH TAIL
                "Ꝛ",  // U+A75A: LATIN CAPITAL LETTER R ROTUNDA
                "Ꞃ",  // U+A782: LATIN CAPITAL LETTER INSULAR R
                "Ｒ", // U+FF32: FULLWIDTH LATIN CAPITAL LETTER R
            ],
            "R",
        ),
        (
            &[
                "ŕ",  // U+0155: LATIN SMALL LETTER R WITH ACUTE
                "ŗ",  // U+0157: LATIN SMALL LETTER R WITH CEDILLA
                "ř",  // U+0159: LATIN SMALL LETTER R WITH CARON
                "ȑ",  // U+0211: LATIN SMALL LETTER R WITH DOUBLE GRAVE
                "ȓ",  // U+0213: LATIN SMALL LETTER R WITH INVERTED BREVE
                "ɍ",  // U+024D: LATIN SMALL LETTER R WITH STROKE
                "ɼ",  // U+027C: LATIN SMALL LETTER R WITH LONG LEG
                "ɽ",  // U+027D: LATIN SMALL LETTER R WITH TAIL
                "ɾ",  // U+027E: LATIN SMALL LETTER R WITH FISHHOOK
                "ɿ",  // U+027F: LATIN SMALL LETTER REVERSED R WITH FISHHOOK
                "ᵣ",  // U+1D63: LATIN SUBSCRIPT SMALL LETTER R
                "ᵲ",  // U+1D72: LATIN SMALL LETTER R WITH MIDDLE TILDE
                "ᵳ",  // U+1D73: LATIN SMALL LETTER R WITH FISHHOOK AND MIDDLE TILDE
                "ᶉ",  // U+1D89: LATIN SMALL LETTER R WITH PALATAL HOOK
                "ṙ",  // U+1E59: LATIN SMALL LETTER R WITH DOT ABOVE
                "ṛ",  // U+1E5B: LATIN SMALL LETTER R WITH DOT BELOW
                "ṝ",  // U+1E5D: LATIN SMALL LETTER R WITH DOT BELOW AND MACRON
                "ṟ",  // U+1E5F: LATIN SMALL LETTER R WITH LINE BELOW
                "ⓡ",  // U+24E1: CIRCLED LATIN SMALL LETTER R
                "ꝛ",  // U+A75B: LATIN SMALL LETTER R ROTUNDA
                "ꞃ",  // U+A783: LATIN SMALL LETTER INSULAR R
                "ｒ", // U+FF52: FULLWIDTH LATIN SMALL LETTER R
            ],
            "r",
        ),
        (
            &[
                "⒭", // U+24AD: PARENTHESIZED LATIN SMALL LETTER R
            ],
            "(r)",
        ),
        (
            &[
                "Ś",  // U+015A: LATIN CAPITAL LETTER S WITH ACUTE
                "Ŝ",  // U+015C: LATIN CAPITAL LETTER S WITH CIRCUMFLEX
                "Ş",  // U+015E: LATIN CAPITAL LETTER S WITH CEDILLA
                "Š",  // U+0160: LATIN CAPITAL LETTER S WITH CARON
                "Ș",  // U+0218: LATIN CAPITAL LETTER S WITH COMMA BELOW
                "Ṡ",  // U+1E60: LATIN CAPITAL LETTER S WITH DOT ABOVE
                "Ṣ",  // U+1E62: LATIN CAPITAL LETTER S WITH DOT BELOW
                "Ṥ",  // U+1E64: LATIN CAPITAL LETTER S WITH ACUTE AND DOT ABOVE
                "Ṧ",  // U+1E66: LATIN CAPITAL LETTER S WITH CARON AND DOT ABOVE
                "Ṩ",  // U+1E68: LATIN CAPITAL LETTER S WITH DOT BELOW AND DOT ABOVE
                "Ⓢ",  // U+24C8: CIRCLED LATIN CAPITAL LETTER S
                "ꜱ",  // U+A731: LATIN LETTER SMALL CAPITAL S
                "ꞅ",  // U+A785: LATIN SMALL LETTER INSULAR S
                "Ｓ", // U+FF33: FULLWIDTH LATIN CAPITAL LETTER S
            ],
            "S",
        ),
        (
            &[
                "ś",  // U+015B: LATIN SMALL LETTER S WITH ACUTE
                "ŝ",  // U+015D: LATIN SMALL LETTER S WITH CIRCUMFLEX
                "ş",  // U+015F: LATIN SMALL LETTER S WITH CEDILLA
                "š",  // U+0161: LATIN SMALL LETTER S WITH CARON
                "ſ",  // U+017F: LATIN SMALL LETTER LONG S
                "ș",  // U+0219: LATIN SMALL LETTER S WITH COMMA BELOW
                "ȿ",  // U+023F: LATIN SMALL LETTER S WITH SWASH TAIL
                "ʂ",  // U+0282: LATIN SMALL LETTER S WITH HOOK
                "ᵴ",  // U+1D74: LATIN SMALL LETTER S WITH MIDDLE TILDE
                "ᶊ",  // U+1D8A: LATIN SMALL LETTER S WITH PALATAL HOOK
                "ṡ",  // U+1E61: LATIN SMALL LETTER S WITH DOT ABOVE
                "ṣ",  // U+1E63: LATIN SMALL LETTER S WITH DOT BELOW
                "ṥ",  // U+1E65: LATIN SMALL LETTER S WITH ACUTE AND DOT ABOVE
                "ṧ",  // U+1E67: LATIN SMALL LETTER S WITH CARON AND DOT ABOVE
                "ṩ",  // U+1E69: LATIN SMALL LETTER S WITH DOT BELOW AND DOT ABOVE
                "ẜ",  // U+1E9C: LATIN SMALL LETTER LONG S WITH DIAGONAL STROKE
                "ẝ",  // U+1E9D: LATIN SMALL LETTER LONG S WITH HIGH STROKE
                "ⓢ",  // U+24E2: CIRCLED LATIN SMALL LETTER S
                "Ꞅ",  // U+A784: LATIN CAPITAL LETTER INSULAR S
                "ｓ", // U+FF53: FULLWIDTH LATIN SMALL LETTER S
            ],
            "s",
        ),
        (
            &[
                "ẞ", // U+1E9E: LATIN CAPITAL LETTER SHARP S
            ],
            "SS",
        ),
        (
            &[
                "⒮", // U+24AE: PARENTHESIZED LATIN SMALL LETTER S
            ],
            "(s)",
        ),
        (
            &[
                "ß", // U+00DF: LATIN SMALL LETTER SHARP S
            ],
            "ss",
        ),
        (
            &[
                "ﬆ", // U+FB06: LATIN SMALL LIGATURE ST
            ],
            "st",
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
