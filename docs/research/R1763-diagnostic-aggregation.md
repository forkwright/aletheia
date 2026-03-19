# R1763: Evaluate Typst Diagnostic Aggregation Pattern for Pipeline Errors

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1763

---

## Executive Summary

Typst's diagnostic aggregation system provides two capabilities that aletheia currently lacks: (1) accumulating multiple errors without short-circuiting and (2) propagating non-fatal warnings alongside a successful result. The core mechanism — using `EcoVec<SourceDiagnostic>` as the error carrier and a `CollectCombinedResult` iterator extension for accumulation — is a pure-library pattern requiring no proc macros. It is **directly adoptable in aletheia** at the pipeline boundary where multiple independent validations run (tool input validation, batch workspace operations). The `Warned<T>` wrapper is equally applicable to `ToolResult` for advisory messages. No changes to snafu error enums are required; the patterns are additive.

**Recommendation: Adopt selectively.** The `CollectCombinedResult` pattern and a `Warned<ToolResult>` wrapper are worth adding now. The full span-based `SourceDiagnostic` system has no corresponding AST in aletheia and should not be ported.

---

## 1. Current Aletheia Pattern

All pipeline stages use snafu with `?` early-return:

```rust
// crates/organon/src/builtins/filesystem.rs — typical tool input extraction
let pattern = input.arguments["pattern"]
    .as_str()
    .ok_or_else(|| error::MissingArgumentSnafu { name: "pattern" }.build())?;

let path = input.arguments["path"]
    .as_str()
    .ok_or_else(|| error::MissingArgumentSnafu { name: "path" }.build())?;
```

If `pattern` is missing, `path` is never checked. The LLM receives one error at a time and must re-attempt to discover every problem. For tools with 5+ parameters this creates multi-turn latency.

The same pattern applies in the compilation-like stages: bootstrap assembly, recall, and tool dispatch each return the first error and discard remaining work.

---

## 2. Typst's Diagnostic System

### 2.1 Type hierarchy

```rust
// Tier 1 — bare string (no location)
pub type StrResult<T> = Result<T, EcoString>;

// Tier 2 — string + hints (compact: EcoVec where [0] = message, [1..] = hints)
pub type HintedStrResult<T> = Result<T, HintedString>;
pub struct HintedString(EcoVec<EcoString>);

// Tier 3 — spans + multi-diagnostic accumulation
pub type SourceResult<T> = Result<T, EcoVec<SourceDiagnostic>>;

pub struct SourceDiagnostic {
    pub severity: Severity,   // Error | Warning
    pub span: Span,
    pub message: EcoString,
    pub hints: EcoVec<Spanned<EcoString>>,
    pub trace: EcoVec<Spanned<Tracepoint>>,
}
```

The critical difference from aletheia's `Result<T, Error>` is that `SourceResult`'s error type is already a **vector**. Two error accumulations can be merged with `errors.extend(other_errors)`.

### 2.2 Promotion traits

```rust
// At<T>: StrResult → SourceResult by attaching a span
pub trait At<T> { fn at(self, span: Span) -> SourceResult<T>; }

// Hint<T>: adds a human hint to any Result
pub trait Hint<T> { fn hint(self, hint: impl Into<EcoString>) -> HintedStrResult<T>; }
```

Usage:
```rust
parse_int(s)
    .hint("values must be in range 0–255")
    .at(param_span)?;
```

### 2.3 Multi-error accumulation without short-circuiting

```rust
pub trait CollectCombinedResult {
    fn collect_combined_result<B>(self) -> SourceResult<B>
    where B: FromIterator<Self::Item>;
}
```

All errors from the iterator are collected before returning:

```rust
// Checks ALL items, accumulates ALL errors
let validated: SourceResult<Vec<_>> = items
    .iter()
    .map(|item| validate(item))   // each returns SourceResult<ValidItem>
    .collect_combined_result();
```

If any items fail, the error vector contains every failure. If all succeed, returns `Ok(Vec<ValidItem>)`.

### 2.4 `Warned<T>` — success with non-fatal advisories

```rust
pub struct Warned<T> {
    pub output: T,
    pub warnings: EcoVec<SourceDiagnostic>,
}
```

Compilation entry points return `Warned<Document>`. Warnings are pushed to a sink during execution without causing early return; the caller receives the completed output alongside any advisories.

---

## 3. Applicability to Aletheia

### 3.1 Tool input validation (highest value)

Current tools extract parameters one-by-one with early `?`. A validation accumulator would collect all missing/invalid parameters and return them in a single error message:

```rust
// Proposed: ValidationAccumulator collects errors without short-circuiting
let mut acc = ValidationAccumulator::new();
let pattern = acc.require_str(&input.arguments, "pattern");
let path     = acc.optional_str(&input.arguments, "path");
let limit    = acc.optional_u64(&input.arguments, "limit", 100);
acc.finish()?;  // returns Err with ALL missing params, or Ok(())
```

This is a pure-library addition to `crates/organon/src/types.rs` — no proc macros, no change to `ToolExecutor`.

### 3.2 Batch operations

Any future endpoint that processes N items (e.g., bulk-validate workspace paths, seed multiple skills) can use a `collect_combined_result`-style accumulator to report all failures in one response rather than stopping at item 1.

### 3.3 `Warned<ToolResult>` for advisory messages

Tools that succeed but have conditions worth surfacing (truncated output, sandbox restriction on one of several paths, deprecated parameter) currently have no channel other than embedding the advisory in the result text. A `Warned<ToolResult>` wrapper lets `dispatch.rs` collect these and attach them to the SSE stream as distinct `ToolWarning` events visible to the TUI without polluting the tool content.

```rust
pub struct WarnedToolResult {
    pub result: ToolResult,
    pub warnings: Vec<ToolWarning>,
}

pub struct ToolWarning {
    pub code: &'static str,  // e.g., "output_truncated", "path_blocked"
    pub message: String,
}
```

### 3.4 What NOT to adopt

- **`Span`-based tracing**: Typst's `Span` is an AST node identifier. Aletheia has no AST. Snafu's `Location` (file:line:col of the Rust call site) already serves the stack-trace use case.
- **The full `SourceDiagnostic` struct**: Too closely tied to typst's compiler model. Aletheia only needs the accumulation and warning separation.
- **`EcoString`/`EcoVec`**: These are typst-specific compact string types. Use `String`/`Vec` in aletheia.

---

## 4. Implementation Plan

### Phase 1: `ValidationAccumulator` (low effort, high LLM UX improvement)

Add `crates/organon/src/validation.rs`:

```rust
pub struct ValidationAccumulator {
    errors: Vec<String>,
}

impl ValidationAccumulator {
    pub fn require_str<'a>(&mut self, args: &'a Value, name: &str) -> Option<&'a str> {
        match args[name].as_str() {
            Some(v) => Some(v),
            None => { self.errors.push(format!("missing required parameter: {name}")); None }
        }
    }
    pub fn finish(self) -> Result<()> {
        if self.errors.is_empty() { Ok(()) }
        else { Err(error::InvalidArgumentsSnafu { errors: self.errors }.build()) }
    }
}
```

Migrate the 5+ parameter tools in `builtins/` to use it.

### Phase 2: `WarnedToolResult` (medium effort)

Add the `WarnedToolResult` type to `crates/organon/src/types.rs`. Update `ToolExecutor::execute` return type or add a parallel `execute_warned` method. Thread warnings through `dispatch.rs` into a new `TurnStreamEvent::ToolWarning` variant.

### Phase 3: Batch `collect_errors` combinator (future, on demand)

Add when a batch-processing endpoint is introduced. Not needed today.

---

## 5. Evidence

- **Typst source**: `crates/typst-library/src/diag.rs` — `HintedString`, `SourceDiagnostic`, `CollectCombinedResult`, `Warned<T>`, `At<T>`, `Hint<T>` traits
- **Accumulation pattern used in practice**: typst compiler's `WorldCompiledPdf::compile()` collects diagnostics from parallel document processing
- **Aletheia tool extraction pattern**: `crates/organon/src/builtins/filesystem.rs:374–433` (grep tool def, 59 lines of manual schema construction + early-exit parameter extraction)

---

## 6. Recommendation

| Action | Effort | Value |
|---|---|---|
| Add `ValidationAccumulator` to `organon` | Low (1–2 days) | High — immediate LLM UX improvement for multi-param tools |
| Add `WarnedToolResult` and `TurnStreamEvent::ToolWarning` | Medium (3–5 days) | Medium — enables advisory channel without polluting tool content |
| Batch `collect_errors` combinator | Low (0.5 days) | Low now — high if batch endpoints are added |

The typst pattern is a concrete, well-tested template. The `CollectCombinedResult` implementation in typst is ~30 lines using only `std` traits and is directly portable. Adopt Phase 1 in the next tool-quality pass; Phase 2 when the TUI work resumes.
