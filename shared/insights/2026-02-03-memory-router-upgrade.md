# Memory Router Upgrade - 2026-02-03

## What Changed

The `memory-router` tool has been upgraded to v2 with iterative retrieval.

## New Features

| Feature | Description |
|---------|-------------|
| **Synonym expansion** | "morning" → ["am", "early", "dawn", "wake up"] |
| **Concept clustering** | Groups related terms (communication, time, performance) |
| **Multi-strategy** | Exact match + semantic matching in parallel |
| **Iterative refinement** | 2-3 passes, learns from initial results |
| **Early stopping** | Stops when confidence plateaus |

## Usage

```bash
# Default (iterative mode)
memory-router "query text"

# With options
memory-router "query" --domains chiron    # Specific domain
memory-router "query" --debug             # Show expansion/iterations
memory-router "query" --json              # JSON output
memory-router "query" --no-iterative      # Single pass (old behavior)
memory-router "query" --max-iter 5        # More iterations
```

## When to Use

- **Use iterative (default)** for most queries - better recall
- **Use --no-iterative** if you need exact matches only
- **Use --debug** to understand what expansions are happening

## Old Version

The original single-pass router is preserved as `memory-router-v1` if needed.

---
*Upgrade performed by Syn, 2026-02-03*
