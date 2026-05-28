# Ingest

`aletheia ingest <path>` loads files into the knowledge store as facts.
This document covers the supported formats, the per-format chunking
behavior, the JSON fact schema, and how directory ingest handles
errors. It is the authoritative reference for the file shapes the
command accepts.

## Synopsis

```
aletheia ingest <PATH> [--format <FORMAT>] [--nous-id <ID>] [--dry-run] [--url <URL>]
```

| Flag        | Default                       | Meaning                                                    |
| ----------- | ----------------------------- | ---------------------------------------------------------- |
| `--format`  | `auto`                        | One of `markdown`, `md`, `text`, `plain_text`, `json`, `jsonl`, `auto` |
| `--nous-id` | `default`                     | Owning nous (agent) for ingested facts                     |
| `--dry-run` | off                           | Parse + report but do not write to the store               |
| `--url`     | `http://127.0.0.1:18789`      | Forward to a running server instead of writing directly    |

If a server is reachable at `--url`, ingest forwards through the API.
Otherwise it writes directly to the local knowledge store under the
current instance root.

## Formats

### `markdown` / `md`

Files are split into sections by ATX-style headings (`#`, `##`, …,
`######`). Each section becomes one fact. YAML frontmatter at the
top of the file (between two `---` lines) is stripped before chunking.

A document like

```
# Section One
The sky is blue.

# Section Two
Water is wet.
```

produces two facts, one per heading. (Prior to #4164 only `##` and
deeper headings split; `#` was silently ignored and the whole file
became a single fact.)

Sections longer than ~2000 characters are further split at word
boundaries with a small overlap, so the chunk-to-fact mapping is
near-1:1 for typical documents.

### `text` / `plain_text`

The file is treated as one block of prose, chunked at word boundaries
into ~2000-character pieces, and each chunk becomes a heuristic fact
with confidence `0.7` and inferred epistemic tier.

### `json` / `jsonl`

Both formats parse directly into the internal `Fact` struct. Use
`json` for an array of facts (or a single object); use `jsonl` for one
fact per line. No heuristic synthesis happens — the input must already
be a complete `Fact`. This makes the two JSON formats useful for
**round-tripping exports**, not for hand-authoring knowledge.

#### Required schema

A fact is a flat JSON object with these keys (some have defaults that
serde will fill in for older exports, but a hand-authored file should
specify them all):

| Key                  | Type     | Required | Notes                                                  |
| -------------------- | -------- | -------- | ------------------------------------------------------ |
| `id`                 | string   | yes      | Stable fact identifier; should be unique per store     |
| `nous_id`            | string   | yes      | Owning nous (agent)                                    |
| `fact_type`          | string   | yes      | e.g. `observation`, `preference`, `procedure`          |
| `content`            | string   | yes      | The fact statement; max 100 KiB                        |
| `scope`              | enum     | no       | Memory sharing scope (`private`/`team`/…)              |
| `project_id`         | string   | no       | Git-remote-derived project partition                   |
| `sensitivity`        | enum     | no       | `public` (default), `internal`, `confidential`         |
| `visibility`         | enum     | no       | `private` (default), `shared`, `restricted`, `published` |
| `valid_from`         | RFC 3339 | yes      | When the fact became true in the domain                |
| `valid_to`           | RFC 3339 | yes      | When it ceased to be true (`9999-01-01T00:00:00Z` = currently valid) |
| `recorded_at`        | RFC 3339 | yes      | When the system learned about the fact                 |
| `confidence`         | float    | yes      | In `[0.0, 1.0]`                                        |
| `tier`               | enum     | yes      | `verified` / `inferred` / `assumed` / `derived`        |
| `source_session_id`  | string   | no       | Session that produced this fact, if any                |
| `stability_hours`    | float    | yes      | Base FSRS stability before tier multiplier             |
| `superseded_by`      | string   | no       | ID of the fact that replaced this one                  |
| `is_forgotten`       | bool     | yes      | Set to `false` for newly authored facts                |
| `forgotten_at`       | RFC 3339 | no       | Only set if `is_forgotten` is true                     |
| `forget_reason`      | enum     | no       | Only set if `is_forgotten` is true                     |
| `access_count`       | int      | yes      | Set to `0` for newly authored facts                    |
| `last_accessed_at`   | RFC 3339 | no       | Omit (or null) for newly authored facts                |

#### Example: minimal hand-authored fact

```json
{
  "id": "fact-01",
  "nous_id": "alice",
  "fact_type": "observation",
  "content": "The user prefers tabs over spaces.",
  "valid_from": "2026-05-28T00:00:00Z",
  "valid_to":   "9999-01-01T00:00:00Z",
  "recorded_at": "2026-05-28T00:00:00Z",
  "confidence": 0.9,
  "tier": "verified",
  "stability_hours": 240.0,
  "access_count": 0,
  "is_forgotten": false
}
```

A hand-written file like `[{"content": "a fact"}]` will be **rejected
with `missing field 'id'`** — there is no lightweight authoring
schema. To author knowledge by hand, prefer `--format markdown` and
let the chunker synthesize the surrounding metadata.

### `auto`

Format is detected from the file extension:

| Extension                   | Format    |
| --------------------------- | --------- |
| `.md` / `.markdown`         | markdown  |
| `.json`                     | json      |
| `.jsonl`                    | jsonl     |
| anything else (incl. `.txt`)| text      |

A directory has no extension and is treated as `text` when the API
path is taken (see *Directory ingest* below).

## Directory ingest

Pointing `ingest` at a directory walks it recursively and ingests every
file with a supported extension (`.md`, `.markdown`, `.txt`, `.text`,
`.json`, `.jsonl`).

Each file is parsed independently. If one file fails to parse — for
example, a malformed `.json` — the failure is **logged as a warning
and the file is recorded as errored**, but the loop continues with the
remaining files. The final summary reports `inserted / skipped /
errored` totals and lists each errored file's path and reason.

```
Total: inserted 12, skipped 0, errored 1 (of 4 files)

Files with errors:
  - /path/to/bad.json: failed to parse /path/to/bad.json
      Caused by: missing field `id`
```

This is a behavioral change in #4164: prior versions of `ingest`
aborted on the first parse failure, leaving the knowledge store in a
non-deterministic partial state.

Per-fact insert failures (e.g., a fact that fails admission control)
are tolerated the same way and counted in `skipped`.

### Note on the API path

When a server is running at `--url`, `ingest <dir>` forwards the
directory as a single concatenated payload and the server ingests it
as `text`. Recursion and per-file format detection only happen on the
direct (no-server) path. If you need the recursive, per-file behavior
documented above, stop the server before ingesting or ingest one file
at a time. (#4164/A tracks unifying the two paths.)
