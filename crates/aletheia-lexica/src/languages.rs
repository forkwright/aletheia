//! ISO 639-1 language codes with human-readable names.
//!
//! No inline language lists were found in the initial migration pass
//! (`nous`, `melete`, `basanos`, `graphe`). This module provides a
//! curated subset of the most common codes.

/// ISO 639-1 two-letter language code paired with its English name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// ISO 639-1 two-letter code (e.g., `"en"`).
    pub code: &'static str,
    /// Human-readable English name (e.g., `"English"`).
    pub name: &'static str,
}

/// Common ISO 639-1 language codes.
pub const COMMON_LANGUAGES: &[Language] = &[
    Language {
        code: "ab",
        name: "Abkhazian",
    },
    Language {
        code: "af",
        name: "Afrikaans",
    },
    Language {
        code: "am",
        name: "Amharic",
    },
    Language {
        code: "ar",
        name: "Arabic",
    },
    Language {
        code: "az",
        name: "Azerbaijani",
    },
    Language {
        code: "be",
        name: "Belarusian",
    },
    Language {
        code: "bg",
        name: "Bulgarian",
    },
    Language {
        code: "bn",
        name: "Bengali",
    },
    Language {
        code: "bs",
        name: "Bosnian",
    },
    Language {
        code: "ca",
        name: "Catalan",
    },
    Language {
        code: "cs",
        name: "Czech",
    },
    Language {
        code: "cy",
        name: "Welsh",
    },
    Language {
        code: "da",
        name: "Danish",
    },
    Language {
        code: "de",
        name: "German",
    },
    Language {
        code: "el",
        name: "Greek",
    },
    Language {
        code: "en",
        name: "English",
    },
    Language {
        code: "eo",
        name: "Esperanto",
    },
    Language {
        code: "es",
        name: "Spanish",
    },
    Language {
        code: "et",
        name: "Estonian",
    },
    Language {
        code: "eu",
        name: "Basque",
    },
    Language {
        code: "fa",
        name: "Persian",
    },
    Language {
        code: "fi",
        name: "Finnish",
    },
    Language {
        code: "fr",
        name: "French",
    },
    Language {
        code: "ga",
        name: "Irish",
    },
    Language {
        code: "gd",
        name: "Scottish Gaelic",
    },
    Language {
        code: "gl",
        name: "Galician",
    },
    Language {
        code: "gu",
        name: "Gujarati",
    },
    Language {
        code: "he",
        name: "Hebrew",
    },
    Language {
        code: "hi",
        name: "Hindi",
    },
    Language {
        code: "hr",
        name: "Croatian",
    },
    Language {
        code: "ht",
        name: "Haitian Creole",
    },
    Language {
        code: "hu",
        name: "Hungarian",
    },
    Language {
        code: "hy",
        name: "Armenian",
    },
    Language {
        code: "id",
        name: "Indonesian",
    },
    Language {
        code: "is",
        name: "Icelandic",
    },
    Language {
        code: "it",
        name: "Italian",
    },
    Language {
        code: "ja",
        name: "Japanese",
    },
    Language {
        code: "ka",
        name: "Georgian",
    },
    Language {
        code: "kk",
        name: "Kazakh",
    },
    Language {
        code: "km",
        name: "Khmer",
    },
    Language {
        code: "kn",
        name: "Kannada",
    },
    Language {
        code: "ko",
        name: "Korean",
    },
    Language {
        code: "ku",
        name: "Kurdish",
    },
    Language {
        code: "ky",
        name: "Kyrgyz",
    },
    Language {
        code: "la",
        name: "Latin",
    },
    Language {
        code: "lb",
        name: "Luxembourgish",
    },
    Language {
        code: "lo",
        name: "Lao",
    },
    Language {
        code: "lt",
        name: "Lithuanian",
    },
    Language {
        code: "lv",
        name: "Latvian",
    },
    Language {
        code: "mg",
        name: "Malagasy",
    },
    Language {
        code: "mi",
        name: "Maori",
    },
    Language {
        code: "mk",
        name: "Macedonian",
    },
    Language {
        code: "ml",
        name: "Malayalam",
    },
    Language {
        code: "mn",
        name: "Mongolian",
    },
    Language {
        code: "mr",
        name: "Marathi",
    },
    Language {
        code: "ms",
        name: "Malay",
    },
    Language {
        code: "mt",
        name: "Maltese",
    },
    Language {
        code: "my",
        name: "Burmese",
    },
    Language {
        code: "ne",
        name: "Nepali",
    },
    Language {
        code: "nl",
        name: "Dutch",
    },
    Language {
        code: "no",
        name: "Norwegian",
    },
    Language {
        code: "ny",
        name: "Chichewa",
    },
    Language {
        code: "pa",
        name: "Punjabi",
    },
    Language {
        code: "pl",
        name: "Polish",
    },
    Language {
        code: "ps",
        name: "Pashto",
    },
    Language {
        code: "pt",
        name: "Portuguese",
    },
    Language {
        code: "ro",
        name: "Romanian",
    },
    Language {
        code: "ru",
        name: "Russian",
    },
    Language {
        code: "sd",
        name: "Sindhi",
    },
    Language {
        code: "si",
        name: "Sinhala",
    },
    Language {
        code: "sk",
        name: "Slovak",
    },
    Language {
        code: "sl",
        name: "Slovenian",
    },
    Language {
        code: "so",
        name: "Somali",
    },
    Language {
        code: "sq",
        name: "Albanian",
    },
    Language {
        code: "sr",
        name: "Serbian",
    },
    Language {
        code: "su",
        name: "Sundanese",
    },
    Language {
        code: "sv",
        name: "Swedish",
    },
    Language {
        code: "sw",
        name: "Swahili",
    },
    Language {
        code: "ta",
        name: "Tamil",
    },
    Language {
        code: "te",
        name: "Telugu",
    },
    Language {
        code: "tg",
        name: "Tajik",
    },
    Language {
        code: "th",
        name: "Thai",
    },
    Language {
        code: "tl",
        name: "Tagalog",
    },
    Language {
        code: "tr",
        name: "Turkish",
    },
    Language {
        code: "uk",
        name: "Ukrainian",
    },
    Language {
        code: "ur",
        name: "Urdu",
    },
    Language {
        code: "uz",
        name: "Uzbek",
    },
    Language {
        code: "vi",
        name: "Vietnamese",
    },
    Language {
        code: "xh",
        name: "Xhosa",
    },
    Language {
        code: "yi",
        name: "Yiddish",
    },
    Language {
        code: "yo",
        name: "Yoruba",
    },
    Language {
        code: "zh",
        name: "Chinese",
    },
    Language {
        code: "zu",
        name: "Zulu",
    },
];
