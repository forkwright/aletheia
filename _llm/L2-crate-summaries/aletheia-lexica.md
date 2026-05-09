# aletheia-lexica

**Purpose:** Static lexicon and data constants for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `UNFALSIFIABLE_ADJECTIVES` | Current public type or boundary; see L3/source for exact fields |
| `CODING_KEYWORDS` | Current public type or boundary; see L3/source for exact fields |
| `RESEARCH_KEYWORDS` | Current public type or boundary; see L3/source for exact fields |
| `PLANNING_KEYWORDS` | Current public type or boundary; see L3/source for exact fields |
| `CONVERSATION_KEYWORDS` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `aletheia-lexica::adjectives` - public items from `src/adjectives.rs`
- `aletheia-lexica::keywords` - public items from `src/keywords.rs`
- `aletheia-lexica::prefixes` - public items from `src/prefixes.rs`
- `aletheia-lexica::stopwords` - public items from `src/stopwords.rs`

## When to look here

- When work touches `crates/aletheia-lexica` or downstream imports from `aletheia-lexica`.
- For exact signatures, load `_llm/L3-api-index/aletheia-lexica.md` if present, then source.

## Recent changes

Coherence work aligned static lexicon data with poiesis intake consumers.
