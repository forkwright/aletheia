# L3 API Index: aletheia-lexica

Crate path: `crates/aletheia-lexica`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/adjectives.rs`

> Adjectives that are unfalsifiable without measurement context.
> 
> These appear in planning documents and vision statements but cannot be
> tested without concrete metrics.
```rust
pub const UNFALSIFIABLE_ADJECTIVES: &[&str] = &[
    "world-class",
    "production-grade",
    "best-in-class",
    "robust",
    "scalable",
];
```

## `src/keywords.rs`

> Keywords that suggest a coding or implementation task.
> 
> Sourced from `nous/src/bootstrap/mod.rs`.
```rust
pub const CODING_KEYWORDS: &[&str] = &[
    "write",
    "create",
    "generate",
    "code",
    "implement",
    "fix",
    "bug",
    "compile",
    "test",
    "refactor",
    "debug",
    "build",
    "error",
    "function",
    "struct",
    "deploy",
    "lint",
];
```

> Keywords that suggest a research or investigation task.
> 
> Sourced from `nous/src/bootstrap/mod.rs`.
```rust
pub const RESEARCH_KEYWORDS: &[&str] = &[
    "what",
    "research",
    "find",
    "search",
    "investigate",
    "analyze",
    "review",
    "compare",
    "evaluate",
    "explain",
    "understand",
    "tell me",
];
```

> Keywords that suggest a planning or design task.
> 
> Sourced from `nous/src/bootstrap/mod.rs`.
```rust
pub const PLANNING_KEYWORDS: &[&str] = &[
    "plan",
    "design",
    "architect",
    "strategy",
    "roadmap",
    "organize",
    "coordinate",
    "priority",
    "prioritize",
    "goal",
    "milestone",
];
```

> Keywords that suggest a casual conversation rather than a task.
> 
> Sourced from `nous/src/bootstrap/mod.rs`.
```rust
pub const CONVERSATION_KEYWORDS: &[&str] = &[
    "hello",
    "hi",
    "hey",
    "thanks",
    "thank you",
    "ok",
    "okay",
    "yes",
    "no",
    "sure",
    "bye",
];
```

> Keywords that map to an analysis intake request.
> 
> Sourced from `poiesis/intake/src/lib.rs`.
```rust
pub const INTAKE_ANALYSIS_KEYWORDS: &[&str] = &[
    "analyze",
    "analysis",
    "analyse",
    "investigate",
    "study",
    "evaluate",
    "assess",
    "compare",
    "review",
    "break down",
    "breakdown",
];
```

> Keywords that map to a report intake request.
> 
> Sourced from `poiesis/intake/src/lib.rs`.
```rust
pub const INTAKE_REPORT_KEYWORDS: &[&str] = &[
    "report",
    "write",
    "document",
    "summary",
    "prose",
    "narrative",
    "brief",
    "whitepaper",
    "white paper",
];
```

> Keywords that map to a dashboard intake request.
> 
> Sourced from `poiesis/intake/src/lib.rs`.
```rust
pub const INTAKE_DASHBOARD_KEYWORDS: &[&str] = &[
    "dashboard",
    "panel",
    "visual",
    "chart",
    "metric",
    "kpi",
    "graph",
    "tableau",
];
```

## `src/prefixes.rs`

> Phrases that indicate the user is issuing a behavioral correction.
> 
> Simple keyword matching is intentionally conservative. False negatives
> (missed corrections) are preferable to false positives (storing random
> sentences as corrections).
> 
> Sourced from `nous/src/hooks/builtins/correction.rs`.
```rust
pub const CORRECTION_PREFIXES: &[&str] = &[
    "don't ",
    "do not ",
    "stop ",
    "never ",
    "always ",
    "from now on",
    "remember to ",
    "make sure to ",
    "please don't ",
    "please do not ",
    "please always ",
    "please never ",
    "you should always ",
    "you should never ",
    "you must always ",
    "you must never ",
    "i need you to always ",
    "i need you to never ",
];
```

## `src/stopwords.rs`

> English stopwords for terminology discovery and text filtering.
> 
> Covers prepositions, pronouns, auxiliary verbs, determiners, and common
> conjunctions. Sourced from `nous/src/recall/reranking.rs`.
```rust
pub const ENGLISH_STOPWORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "and",
    "but",
    "or",
    "nor",
    "for",
    "yet",
    "so",
    "in",
    "on",
    "at",
    "to",
    "from",
    "by",
    "with",
    "about",
    "into",
    "through",
    "during",
    "before",
    "after",
    "above",
    "below",
    "between",
    "out",
    "off",
    "over",
    "under",
    "again",
    "further",
    "then",
    "once",
    "is",
    "am",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "have",
    "has",
    "had",
    "having",
    "do",
    "does",
    "did",
    "doing",
    "will",
    "would",
    "shall",
    "should",
    "may",
    "might",
    "must",
    "can",
    "could",
    "need",
    "dare",
    "ought",
    "used",
    "i",
    "me",
    "my",
    "myself",
    "we",
    "our",
    "ours",
    "ourselves",
    "you",
    "your",
    "yours",
    "yourself",
    "yourselves",
    "he",
    "him",
    "his",
    "himself",
    "she",
    "her",
    "hers",
    "herself",
    "it",
    "its",
    "itself",
    "they",
    "them",
    "their",
    "theirs",
    "themselves",
    "what",
    "which",
    "who",
    "whom",
    "this",
    "that",
    "these",
    "those",
    "here",
    "there",
    "when",
    "where",
    "why",
    "how",
    "all",
    "each",
    "every",
    "both",
    "few",
    "more",
    "most",
    "other",
    "some",
    "such",
    "only",
    "own",
    "same",
    "than",
    "too",
    "very",
    "just",
    "also",
    "not",
    "no",
];
```

> Smaller stopword list for probe token-overlap comparison.
> 
> Focused on high-frequency function words. Sourced from
> `melete/src/probe.rs`.
```rust
pub const ENGLISH_PROBE_STOP_WORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was", "one",
    "our", "out", "has", "his", "how", "its", "may", "new", "now", "old", "see", "way", "who",
    "did", "get", "let", "say", "she", "too", "use", "will", "with", "this", "that", "from",
    "have", "been", "some", "they", "were", "what", "when", "your", "each", "make", "like", "into",
    "just", "over", "such", "than", "them", "then", "also", "more", "should",
];
```
