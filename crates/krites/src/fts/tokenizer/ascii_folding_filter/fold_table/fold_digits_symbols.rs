//! Folding table for digits and symbols.

pub(super) fn fold_digit_or_symbol(c: char) -> Option<&'static str> {
    match c {
        '\u{2070}' | // ⁰  [SUPERSCRIPT ZERO]
        '\u{2080}' | // ₀  [SUBSCRIPT ZERO]
        '\u{24EA}' | // ⓪  [CIRCLED DIGIT ZERO]
        '\u{24FF}' | // ⓿  [NEGATIVE CIRCLED DIGIT ZERO]
        '\u{FF10}' // ０  [FULLWIDTH DIGIT ZERO]
        => Some("0"),
        '\u{00B9}' | // ¹  [SUPERSCRIPT ONE]
        '\u{2081}' | // ₁  [SUBSCRIPT ONE]
        '\u{2460}' | // ①  [CIRCLED DIGIT ONE]
        '\u{24F5}' | // ⓵  [DOUBLE CIRCLED DIGIT ONE]
        '\u{2776}' | // ❶  [DINGBAT NEGATIVE CIRCLED DIGIT ONE]
        '\u{2780}' | // ➀  [DINGBAT CIRCLED SANS-SERIF DIGIT ONE]
        '\u{278A}' | // ➊  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT ONE]
        '\u{FF11}' // １  [FULLWIDTH DIGIT ONE]
        => Some("1"),
        '\u{2488}' // ⒈  [DIGIT ONE FULL STOP]
        => Some("1."),
        '\u{2474}' // ⑴  [PARENTHESIZED DIGIT ONE]
        => Some("(1)"),
        '\u{00B2}' | // ²  [SUPERSCRIPT TWO]
        '\u{2082}' | // ₂  [SUBSCRIPT TWO]
        '\u{2461}' | // ②  [CIRCLED DIGIT TWO]
        '\u{24F6}' | // ⓶  [DOUBLE CIRCLED DIGIT TWO]
        '\u{2777}' | // ❷  [DINGBAT NEGATIVE CIRCLED DIGIT TWO]
        '\u{2781}' | // ➁  [DINGBAT CIRCLED SANS-SERIF DIGIT TWO]
        '\u{278B}' | // ➋  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT TWO]
        '\u{FF12}' // ２  [FULLWIDTH DIGIT TWO]
        => Some("2"),
        '\u{2489}' // ⒉  [DIGIT TWO FULL STOP]
        => Some("2."),
        '\u{2475}' // ⑵  [PARENTHESIZED DIGIT TWO]
        => Some("(2)"),
        '\u{00B3}' | // ³  [SUPERSCRIPT THREE]
        '\u{2083}' | // ₃  [SUBSCRIPT THREE]
        '\u{2462}' | // ③  [CIRCLED DIGIT THREE]
        '\u{24F7}' | // ⓷  [DOUBLE CIRCLED DIGIT THREE]
        '\u{2778}' | // ❸  [DINGBAT NEGATIVE CIRCLED DIGIT THREE]
        '\u{2782}' | // ➂  [DINGBAT CIRCLED SANS-SERIF DIGIT THREE]
        '\u{278C}' | // ➌  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT THREE]
        '\u{FF13}' // ３  [FULLWIDTH DIGIT THREE]
        => Some("3"),
        '\u{248A}' // ⒊  [DIGIT THREE FULL STOP]
        => Some("3."),
        '\u{2476}' // ⑶  [PARENTHESIZED DIGIT THREE]
        => Some("(3)"),
        '\u{2074}' | // ⁴  [SUPERSCRIPT FOUR]
        '\u{2084}' | // ₄  [SUBSCRIPT FOUR]
        '\u{2463}' | // ④  [CIRCLED DIGIT FOUR]
        '\u{24F8}' | // ⓸  [DOUBLE CIRCLED DIGIT FOUR]
        '\u{2779}' | // ❹  [DINGBAT NEGATIVE CIRCLED DIGIT FOUR]
        '\u{2783}' | // ➃  [DINGBAT CIRCLED SANS-SERIF DIGIT FOUR]
        '\u{278D}' | // ➍  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT FOUR]
        '\u{FF14}' // ４  [FULLWIDTH DIGIT FOUR]
        => Some("4"),
        '\u{248B}' // ⒋  [DIGIT FOUR FULL STOP]
        => Some("4."),
        '\u{2477}' // ⑷  [PARENTHESIZED DIGIT FOUR]
        => Some("(4)"),
        '\u{2075}' | // ⁵  [SUPERSCRIPT FIVE]
        '\u{2085}' | // ₅  [SUBSCRIPT FIVE]
        '\u{2464}' | // ⑤  [CIRCLED DIGIT FIVE]
        '\u{24F9}' | // ⓹  [DOUBLE CIRCLED DIGIT FIVE]
        '\u{277A}' | // ❺  [DINGBAT NEGATIVE CIRCLED DIGIT FIVE]
        '\u{2784}' | // ➄  [DINGBAT CIRCLED SANS-SERIF DIGIT FIVE]
        '\u{278E}' | // ➎  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT FIVE]
        '\u{FF15}' // ５  [FULLWIDTH DIGIT FIVE]
        => Some("5"),
        '\u{248C}' // ⒌  [DIGIT FIVE FULL STOP]
        => Some("5."),
        '\u{2478}' // ⑸  [PARENTHESIZED DIGIT FIVE]
        => Some("(5)"),
        '\u{2076}' | // ⁶  [SUPERSCRIPT SIX]
        '\u{2086}' | // ₆  [SUBSCRIPT SIX]
        '\u{2465}' | // ⑥  [CIRCLED DIGIT SIX]
        '\u{24FA}' | // ⓺  [DOUBLE CIRCLED DIGIT SIX]
        '\u{277B}' | // ❻  [DINGBAT NEGATIVE CIRCLED DIGIT SIX]
        '\u{2785}' | // ➅  [DINGBAT CIRCLED SANS-SERIF DIGIT SIX]
        '\u{278F}' | // ➏  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT SIX]
        '\u{FF16}' // ６  [FULLWIDTH DIGIT SIX]
        => Some("6"),
        '\u{248D}' // ⒍  [DIGIT SIX FULL STOP]
        => Some("6."),
        '\u{2479}' // ⑹  [PARENTHESIZED DIGIT SIX]
        => Some("(6)"),
        '\u{2077}' | // ⁷  [SUPERSCRIPT SEVEN]
        '\u{2087}' | // ₇  [SUBSCRIPT SEVEN]
        '\u{2466}' | // ⑦  [CIRCLED DIGIT SEVEN]
        '\u{24FB}' | // ⓻  [DOUBLE CIRCLED DIGIT SEVEN]
        '\u{277C}' | // ❼  [DINGBAT NEGATIVE CIRCLED DIGIT SEVEN]
        '\u{2786}' | // ➆  [DINGBAT CIRCLED SANS-SERIF DIGIT SEVEN]
        '\u{2790}' | // ➐  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT SEVEN]
        '\u{FF17}' // ７  [FULLWIDTH DIGIT SEVEN]
        => Some("7"),
        '\u{248E}' // ⒎  [DIGIT SEVEN FULL STOP]
        => Some("7."),
        '\u{247A}' // ⑺  [PARENTHESIZED DIGIT SEVEN]
        => Some("(7)"),
        '\u{2078}' | // ⁸  [SUPERSCRIPT EIGHT]
        '\u{2088}' | // ₈  [SUBSCRIPT EIGHT]
        '\u{2467}' | // ⑧  [CIRCLED DIGIT EIGHT]
        '\u{24FC}' | // ⓼  [DOUBLE CIRCLED DIGIT EIGHT]
        '\u{277D}' | // ❽  [DINGBAT NEGATIVE CIRCLED DIGIT EIGHT]
        '\u{2787}' | // ➇  [DINGBAT CIRCLED SANS-SERIF DIGIT EIGHT]
        '\u{2791}' | // ➑  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT EIGHT]
        '\u{FF18}' // ８  [FULLWIDTH DIGIT EIGHT]
        => Some("8"),
        '\u{248F}' // ⒏  [DIGIT EIGHT FULL STOP]
        => Some("8."),
        '\u{247B}' // ⑻  [PARENTHESIZED DIGIT EIGHT]
        => Some("(8)"),
        '\u{2079}' | // ⁹  [SUPERSCRIPT NINE]
        '\u{2089}' | // ₉  [SUBSCRIPT NINE]
        '\u{2468}' | // ⑨  [CIRCLED DIGIT NINE]
        '\u{24FD}' | // ⓽  [DOUBLE CIRCLED DIGIT NINE]
        '\u{277E}' | // ❾  [DINGBAT NEGATIVE CIRCLED DIGIT NINE]
        '\u{2788}' | // ➈  [DINGBAT CIRCLED SANS-SERIF DIGIT NINE]
        '\u{2792}' | // ➒  [DINGBAT NEGATIVE CIRCLED SANS-SERIF DIGIT NINE]
        '\u{FF19}' // ９  [FULLWIDTH DIGIT NINE]
        => Some("9"),
        '\u{2490}' // ⒐  [DIGIT NINE FULL STOP]
        => Some("9."),
        '\u{247C}' // ⑼  [PARENTHESIZED DIGIT NINE]
        => Some("(9)"),
        '\u{2469}' | // ⑩  [CIRCLED NUMBER TEN]
        '\u{24FE}' | // ⓾  [DOUBLE CIRCLED NUMBER TEN]
        '\u{277F}' | // ❿  [DINGBAT NEGATIVE CIRCLED NUMBER TEN]
        '\u{2789}' | // ➉  [DINGBAT CIRCLED SANS-SERIF NUMBER TEN]
        '\u{2793}' // ➓  [DINGBAT NEGATIVE CIRCLED SANS-SERIF NUMBER TEN]
        => Some("10"),
        '\u{2491}' // ⒑  [NUMBER TEN FULL STOP]
        => Some("10."),
        '\u{247D}' // ⑽  [PARENTHESIZED NUMBER TEN]
        => Some("(10)"),
        '\u{246A}' | // ⑪  [CIRCLED NUMBER ELEVEN]
        '\u{24EB}' // ⓫  [NEGATIVE CIRCLED NUMBER ELEVEN]
        => Some("11"),
        '\u{2492}' // ⒒  [NUMBER ELEVEN FULL STOP]
        => Some("11."),
        '\u{247E}' // ⑾  [PARENTHESIZED NUMBER ELEVEN]
        => Some("(11)"),
        '\u{246B}' | // ⑫  [CIRCLED NUMBER TWELVE]
        '\u{24EC}' // ⓬  [NEGATIVE CIRCLED NUMBER TWELVE]
        => Some("12"),
        '\u{2493}' // ⒓  [NUMBER TWELVE FULL STOP]
        => Some("12."),
        '\u{247F}' // ⑿  [PARENTHESIZED NUMBER TWELVE]
        => Some("(12)"),
        '\u{246C}' | // ⑬  [CIRCLED NUMBER THIRTEEN]
        '\u{24ED}' // ⓭  [NEGATIVE CIRCLED NUMBER THIRTEEN]
        => Some("13"),
        '\u{2494}' // ⒔  [NUMBER THIRTEEN FULL STOP]
        => Some("13."),
        '\u{2480}' // ⒀  [PARENTHESIZED NUMBER THIRTEEN]
        => Some("(13)"),
        '\u{246D}' | // ⑭  [CIRCLED NUMBER FOURTEEN]
        '\u{24EE}' // ⓮  [NEGATIVE CIRCLED NUMBER FOURTEEN]
        => Some("14"),
        '\u{2495}' // ⒕  [NUMBER FOURTEEN FULL STOP]
        => Some("14."),
        '\u{2481}' // ⒁  [PARENTHESIZED NUMBER FOURTEEN]
        => Some("(14)"),
        '\u{246E}' | // ⑮  [CIRCLED NUMBER FIFTEEN]
        '\u{24EF}' // ⓯  [NEGATIVE CIRCLED NUMBER FIFTEEN]
        => Some("15"),
        '\u{2496}' // ⒖  [NUMBER FIFTEEN FULL STOP]
        => Some("15."),
        '\u{2482}' // ⒂  [PARENTHESIZED NUMBER FIFTEEN]
        => Some("(15)"),
        '\u{246F}' | // ⑯  [CIRCLED NUMBER SIXTEEN]
        '\u{24F0}' // ⓰  [NEGATIVE CIRCLED NUMBER SIXTEEN]
        => Some("16"),
        '\u{2497}' // ⒗  [NUMBER SIXTEEN FULL STOP]
        => Some("16."),
        '\u{2483}' // ⒃  [PARENTHESIZED NUMBER SIXTEEN]
        => Some("(16)"),
        '\u{2470}' | // ⑰  [CIRCLED NUMBER SEVENTEEN]
        '\u{24F1}' // ⓱  [NEGATIVE CIRCLED NUMBER SEVENTEEN]
        => Some("17"),
        '\u{2498}' // ⒘  [NUMBER SEVENTEEN FULL STOP]
        => Some("17."),
        '\u{2484}' // ⒄  [PARENTHESIZED NUMBER SEVENTEEN]
        => Some("(17)"),
        '\u{2471}' | // ⑱  [CIRCLED NUMBER EIGHTEEN]
        '\u{24F2}' // ⓲  [NEGATIVE CIRCLED NUMBER EIGHTEEN]
        => Some("18"),
        '\u{2499}' // ⒙  [NUMBER EIGHTEEN FULL STOP]
        => Some("18."),
        '\u{2485}' // ⒅  [PARENTHESIZED NUMBER EIGHTEEN]
        => Some("(18)"),
        '\u{2472}' | // ⑲  [CIRCLED NUMBER NINETEEN]
        '\u{24F3}' // ⓳  [NEGATIVE CIRCLED NUMBER NINETEEN]
        => Some("19"),
        '\u{249A}' // ⒚  [NUMBER NINETEEN FULL STOP]
        => Some("19."),
        '\u{2486}' // ⒆  [PARENTHESIZED NUMBER NINETEEN]
        => Some("(19)"),
        '\u{2473}' | // ⑳  [CIRCLED NUMBER TWENTY]
        '\u{24F4}' // ⓴  [NEGATIVE CIRCLED NUMBER TWENTY]
        => Some("20"),
        '\u{249B}' // ⒛  [NUMBER TWENTY FULL STOP]
        => Some("20."),
        '\u{2487}' // ⒇  [PARENTHESIZED NUMBER TWENTY]
        => Some("(20)"),
        '\u{00AB}' | // «  [LEFT-POINTING DOUBLE ANGLE QUOTATION MARK]
        '\u{00BB}' | // »  [RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK]
        '\u{201C}' | // “  [LEFT DOUBLE QUOTATION MARK]
        '\u{201D}' | // ”  [RIGHT DOUBLE QUOTATION MARK]
        '\u{201E}' | // „  [DOUBLE LOW-9 QUOTATION MARK]
        '\u{2033}' | // ″  [DOUBLE PRIME]
        '\u{2036}' | // ‶  [REVERSED DOUBLE PRIME]
        '\u{275D}' | // ❝  [HEAVY DOUBLE TURNED COMMA QUOTATION MARK ORNAMENT]
        '\u{275E}' | // ❞  [HEAVY DOUBLE COMMA QUOTATION MARK ORNAMENT]
        '\u{276E}' | // ❮  [HEAVY LEFT-POINTING ANGLE QUOTATION MARK ORNAMENT]
        '\u{276F}' | // ❯  [HEAVY RIGHT-POINTING ANGLE QUOTATION MARK ORNAMENT]
        '\u{FF02}' // ＂  [FULLWIDTH QUOTATION MARK]
        => Some("\""),
        '\u{2018}' | // ‘  [LEFT SINGLE QUOTATION MARK]
        '\u{2019}' | // ’  [RIGHT SINGLE QUOTATION MARK]
        '\u{201A}' | // ‚  [SINGLE LOW-9 QUOTATION MARK]
        '\u{201B}' | // ‛  [SINGLE HIGH-REVERSED-9 QUOTATION MARK]
        '\u{2032}' | // ′  [PRIME]
        '\u{2035}' | // ‵  [REVERSED PRIME]
        '\u{2039}' | // ‹  [SINGLE LEFT-POINTING ANGLE QUOTATION MARK]
        '\u{203A}' | // ›  [SINGLE RIGHT-POINTING ANGLE QUOTATION MARK]
        '\u{275B}' | // ❛  [HEAVY SINGLE TURNED COMMA QUOTATION MARK ORNAMENT]
        '\u{275C}' | // ❜  [HEAVY SINGLE COMMA QUOTATION MARK ORNAMENT]
        '\u{FF07}' // ＇  [FULLWIDTH APOSTROPHE]
        => Some("\'"),
        '\u{2010}' | // ‐  [HYPHEN]
        '\u{2011}' | // ‑  [NON-BREAKING HYPHEN]
        '\u{2012}' | // ‒  [FIGURE DASH]
        '\u{2013}' | // –  [EN DASH]
        '\u{2014}' | //:  [EM DASH]
        '\u{207B}' | // ⁻  [SUPERSCRIPT MINUS]
        '\u{208B}' | // ₋  [SUBSCRIPT MINUS]
        '\u{FF0D}' // －  [FULLWIDTH HYPHEN-MINUS]
        => Some("-"),
        '\u{2045}' | // ⁅  [LEFT SQUARE BRACKET WITH QUILL]
        '\u{2772}' | // ❲  [LIGHT LEFT TORTOISE SHELL BRACKET ORNAMENT]
        '\u{FF3B}' // ［  [FULLWIDTH LEFT SQUARE BRACKET]
        => Some("["),
        '\u{2046}' | // ⁆  [RIGHT SQUARE BRACKET WITH QUILL]
        '\u{2773}' | // ❳  [LIGHT RIGHT TORTOISE SHELL BRACKET ORNAMENT]
        '\u{FF3D}' // ］  [FULLWIDTH RIGHT SQUARE BRACKET]
        => Some("]"),
        '\u{207D}' | // ⁽  [SUPERSCRIPT LEFT PARENTHESIS]
        '\u{208D}' | // ₍  [SUBSCRIPT LEFT PARENTHESIS]
        '\u{2768}' | // ❨  [MEDIUM LEFT PARENTHESIS ORNAMENT]
        '\u{276A}' | // ❪  [MEDIUM FLATTENED LEFT PARENTHESIS ORNAMENT]
        '\u{FF08}' // （  [FULLWIDTH LEFT PARENTHESIS]
        => Some("("),
        '\u{2E28}' // ⸨  [LEFT DOUBLE PARENTHESIS]
        => Some("(("),
        '\u{207E}' | // ⁾  [SUPERSCRIPT RIGHT PARENTHESIS]
        '\u{208E}' | // ₎  [SUBSCRIPT RIGHT PARENTHESIS]
        '\u{2769}' | // ❩  [MEDIUM RIGHT PARENTHESIS ORNAMENT]
        '\u{276B}' | // ❫  [MEDIUM FLATTENED RIGHT PARENTHESIS ORNAMENT]
        '\u{FF09}' // ）  [FULLWIDTH RIGHT PARENTHESIS]
        => Some(")"),
        '\u{2E29}' // ⸩  [RIGHT DOUBLE PARENTHESIS]
        => Some("))"),
        '\u{276C}' | // ❬  [MEDIUM LEFT-POINTING ANGLE BRACKET ORNAMENT]
        '\u{2770}' | // ❰  [HEAVY LEFT-POINTING ANGLE BRACKET ORNAMENT]
        '\u{FF1C}' // ＜  [FULLWIDTH LESS-THAN SIGN]
        => Some("<"),
        '\u{276D}' | // ❭  [MEDIUM RIGHT-POINTING ANGLE BRACKET ORNAMENT]
        '\u{2771}' | // ❱  [HEAVY RIGHT-POINTING ANGLE BRACKET ORNAMENT]
        '\u{FF1E}' // ＞  [FULLWIDTH GREATER-THAN SIGN]
        => Some(">"),
        '\u{2774}' | // ❴  [MEDIUM LEFT CURLY BRACKET ORNAMENT]
        '\u{FF5B}' // ｛  [FULLWIDTH LEFT CURLY BRACKET]
        => Some("{"),
        '\u{2775}' | // ❵  [MEDIUM RIGHT CURLY BRACKET ORNAMENT]
        '\u{FF5D}' // ｝  [FULLWIDTH RIGHT CURLY BRACKET]
        => Some("}"),
        '\u{207A}' | // ⁺  [SUPERSCRIPT PLUS SIGN]
        '\u{208A}' | // ₊  [SUBSCRIPT PLUS SIGN]
        '\u{FF0B}' // ＋  [FULLWIDTH PLUS SIGN]
        => Some("+"),
        '\u{207C}' | // ⁼  [SUPERSCRIPT EQUALS SIGN]
        '\u{208C}' | // ₌  [SUBSCRIPT EQUALS SIGN]
        '\u{FF1D}' // ＝  [FULLWIDTH EQUALS SIGN]
        => Some("="),
        '\u{FF01}' // ！  [FULLWIDTH EXCLAMATION MARK]
        => Some("!"),
        '\u{203C}' // ‼  [DOUBLE EXCLAMATION MARK]
        => Some("!!"),
        '\u{2049}' // ⁉  [EXCLAMATION QUESTION MARK]
        => Some("!?"),
        '\u{FF03}' // ＃  [FULLWIDTH NUMBER SIGN]
        => Some("#"),
        '\u{FF04}' // ＄  [FULLWIDTH DOLLAR SIGN]
        => Some("$"),
        '\u{2052}' | // ⁒  [COMMERCIAL MINUS SIGN]
        '\u{FF05}' // ％  [FULLWIDTH PERCENT SIGN]
        => Some("%"),
        '\u{FF06}' // ＆  [FULLWIDTH AMPERSAND]
        => Some("&"),
        '\u{204E}' | // ⁎  [LOW ASTERISK]
        '\u{FF0A}' // ＊  [FULLWIDTH ASTERISK]
        => Some("*"),
        '\u{FF0C}' // ，  [FULLWIDTH COMMA]
        => Some(","),
        '\u{FF0E}' // ．  [FULLWIDTH FULL STOP]
        => Some("."),
        '\u{2044}' | // ⁄  [FRACTION SLASH]
        '\u{FF0F}' // ／  [FULLWIDTH SOLIDUS]
        => Some("/"),
        '\u{FF1A}' // ：  [FULLWIDTH COLON]
        => Some(":"),
        '\u{204F}' | // ⁏  [REVERSED SEMICOLON]
        '\u{FF1B}' // ；  [FULLWIDTH SEMICOLON]
        => Some(";"),
        '\u{FF1F}' // ？  [FULLWIDTH QUESTION MARK]
        => Some("?"),
        '\u{2047}' // ⁇  [DOUBLE QUESTION MARK]
        => Some("??"),
        '\u{2048}' // ⁈  [QUESTION EXCLAMATION MARK]
        => Some("?!"),
        '\u{FF20}' // ＠  [FULLWIDTH COMMERCIAL AT]
        => Some("@"),
        '\u{FF3C}' // ＼  [FULLWIDTH REVERSE SOLIDUS]
        => Some("\\"),
        '\u{2038}' | // ‸  [CARET]
        '\u{FF3E}' // ＾  [FULLWIDTH CIRCUMFLEX ACCENT]
        => Some("^"),
        '\u{FF3F}' // ＿  [FULLWIDTH LOW LINE]
        => Some("_"),
        '\u{2053}' | // ⁓  [SWUNG DASH]
        '\u{FF5E}' // ～  [FULLWIDTH TILDE]
        => Some("~"),
        _ => None
    }
}
