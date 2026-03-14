# Standards

Universal coding standards for all code, all languages, all projects. Language-specific rules live in separate files in this directory — they are **additive** to these documents. Read this file first.

---

## Index

| File | Scope |
|------|-------|
| [DOCUMENTATION.md](DOCUMENTATION.md) | Comment system, doc comment rules, banned comment patterns |
| [NAMING.md](NAMING.md) | Identifiers, file names, directory organization, project structure |
| [TESTING.md](TESTING.md) | Test philosophy, property-based testing, test data policy |
| [ERRORS.md](ERRORS.md) | Error handling rules, fail-fast policy, resource lifecycle |
| [CONCURRENCY.md](CONCURRENCY.md) | Ownership, shared state, thread safety contracts, testing concurrent code |
| [SECURITY.md](SECURITY.md) | Secrets, input validation, output encoding, least privilege |
| [LOGGING.md](LOGGING.md) | Structured logging, log levels, what to log |
| [GIT.md](GIT.md) | Conventional commits, branching, worktrees, PR discipline |
| [DEPENDENCIES.md](DEPENDENCIES.md) | Dependency policy, auditing, banned packages |
| [WRITING.md](WRITING.md) | Prose style for docs, READMEs, PR descriptions, comments |
| [RUST.md](RUST.md) | Rust-specific: toolchain, type system, async, error handling |
| [PYTHON.md](PYTHON.md) | Python-specific standards |
| [TYPESCRIPT.md](TYPESCRIPT.md) | TypeScript-specific standards |
| [SHELL.md](SHELL.md) | Shell script standards |
| [SQL.md](SQL.md) | SQL standards |
| [KOTLIN.md](KOTLIN.md) | Kotlin-specific standards |
| [CSHARP.md](CSHARP.md) | C#/.NET-specific standards |
| [CPP.md](CPP.md) | C++-specific standards |

---

## Philosophy

**Code is the documentation.** Names, types, and structure carry meaning. If code needs a comment to explain what it does, rewrite the code. Comments explain *why*, never *what*.

**Fail fast, fail loud.** Crash on invariant violations. No defensive fallbacks for impossible states. Sentinel values and silent degradation are bugs. Surface errors at the point of origin with full context.

**Parse, don't validate.** Invalid data cannot exist past the point of construction. Newtypes, validation constructors, and type-level guarantees enforce invariants at the boundary — HTTP handlers, config loading, deserialization, CLI argument parsing. Once a value is constructed, its validity is a compile-time or construction-time guarantee. Deserialization must route through the parser: derive-based frameworks (`serde`, `System.Text.Json`, `encoding/json`) bypass constructors by default.

**Prefer immutable.** Default to immutable data. Require explicit justification for mutability. Mutable shared state is the root of most concurrency bugs and a common source of aliasing surprises.

**Minimize surface area.** Private by default. Every public item is a commitment. Expose the smallest API that serves the need. `pub(crate)` (Rust), `internal` (C#), unexported (Kotlin/TS), `_prefix` (Python).

**No dead weight.** No dead code. No commented-out blocks. No unused imports. Delete it — git has history. No hardcoded IDs, dates, or magic numbers. Parameterize or reference a constant.

**No shortcuts.** Build the right thing from the start. If the SDK is better than the CLI wrapper, build the SDK. If the architecture needs three crates, build three crates. Don't ship a "quick version" you know you'll replace — time spent on throwaway work is stolen from the real thing. MVPs are for validating markets, not for code you're certain about.

**Best tool for the job.** Every decision — language, library, architecture, data structure — is made on merit. No defaults by inertia. No "we've always done it this way." If the current tool is wrong, replace it. If a better option exists and the migration cost is justified, migrate. Comfort with a tool is not a reason to use it; fitness for the problem is.

**No compromise on quality.** Every PR should be clean, tested, and reviewed before merge. Fix issues immediately, don't defer. "Good enough" is not a standard. The goal is code you'd be confident handing to a stranger with zero context — they should be able to read it, understand it, and trust it. Cutting corners creates debt that compounds faster than the time it "saved."

**Format at the boundary.** Percentages as decimals (0.42), currency as numbers, dates as timestamps internally. Format when rendering for display, not in queries or transforms.

**Idempotent by design.** Operations that may be retried, replayed, or delivered more than once must produce the same result regardless of repetition. Use idempotency keys for API mutations. Design event handlers to tolerate duplicate delivery. Message processing, webhook handlers, and state transitions are the primary risk areas. If replaying an operation would corrupt state, the operation is broken.

---

## Information Hierarchy

This principle governs everything: documentation, configuration, standards, code architecture, API design. Not just docs.

### Single Source of Truth

Every fact, rule, or definition lives in exactly one place. Everything else points to it.

When information exists at multiple levels (universal standards, language addenda, repo docs, module docs), it belongs at the **lowest common ancestor**: the most general file where it's universally true. Children inherit; they never restate.

This standards package itself follows this rule:
- `README.md` holds universal principles (you're reading it)
- Topic files (`TESTING.md`, `NAMING.md`, etc.) hold only what's topic-specific
- Language files (`RUST.md`, `PYTHON.md`, etc.) hold only what's language-specific
- Language files don't repeat anything from this file
- If a principle applies to two or more languages, it moves here

The same applies to code:
- Shared types live in the lowest common crate/module
- Config defaults live at the most general level; overrides at the specific level
- Error types are defined per crate boundary, not duplicated across crates
- A helper function used in two places gets extracted, not copied

### Rules

- **Don't duplicate down.** If a rule applies everywhere, it goes in the shared file. Children inherit silently.
- **Don't duplicate up.** If a rule is specific to one context, it stays there. The parent doesn't mention it.
- **Pointers, not copies.** When a child needs to reference a parent rule: `See README.md`. Don't paste content.
- **One update, one file.** If changing a fact requires editing multiple files, the hierarchy is wrong. Fix the hierarchy.
- **Delete redirects.** If a file exists only to say "moved to X", delete it. Git has history.

### Document Lifecycle

Documentation follows the code it describes. When code is deleted, moved, or substantially refactored, update or remove its documentation in the same change. Orphaned docs — documentation for code that no longer exists — are worse than no docs because they actively mislead.

### Litmus Test

Before writing anything (doc, config, code), ask:
1. Does this fact already exist somewhere? → Point to it.
2. Is this true for more than one context? → Move it up.
3. Will someone need to update this in two places? → Wrong level.

---

## Configuration

- **Config in environment, not code.** Values that vary between deploys — credentials, hostnames, feature flags — live in environment variables or external config stores, never compiled in.
- **No hardcoded secrets.** Connection strings, API keys, and passwords never in source. Not in config files committed to git. Use secret stores or environment injection.
- **Inject inward, never fetch.** Configuration values are pushed from the outermost layer (main, entry point) and injected into inner modules. Inner code receives config — it never reads environment variables or config files directly.
- **Fail on invalid config at startup.** Validate all configuration during initialization. Don't discover bad config at 3 AM when the code path first executes.

---

## Module Boundaries & API Design

### Dependency Direction

Imports flow from higher layers to lower layers only. No dependency cycles. Adding a cross-module import requires verifying the dependency graph.

### Explicit Public Surface

Each module declares its public surface explicitly. Consumers import from the public API, not internal files.

### API Principles

- **Return empty collections, not null.** Callers should not need null checks for collection returns.
- **Return values over output parameters.** Data flows through return values, not side-effect mutation of passed-in references.
- **Validate parameters at public boundaries.** Public functions validate their arguments. Private functions may rely on invariants established by callers.
- **Defensive copy at API boundaries.** Copy mutable data received from and returned to callers. Never let callers alias internal mutable state.

### Deprecation

Mark deprecated code with the language's mechanism (`#[deprecated]`, `@Deprecated`, `@warnings.deprecated`). Document the replacement. Set a removal version or date. Remove it when the time comes. Dead deprecation warnings that persist indefinitely are noise.

---

## Code Review

### What Reviewers Check

1. **Does it do what the PR says?** Read the description, read the diff. Do they match?
2. **Error handling.** Are errors propagated with context? Any silent catches? Any unwraps in library code?
3. **Naming.** Do names describe what things are? Would a reader unfamiliar with the PR understand the code?
4. **Tests.** Does the change have tests? Do the tests test behavior, not implementation?
5. **Scope.** Does the PR do one thing? Unrelated changes get their own PR.
6. **Information hierarchy.** Is new code in the right place? Shared logic in the right module? No duplication?

### How to Give Feedback

- **Be specific.** "This name is unclear" is useless. "Rename `proc` to `process_session` since it handles session lifecycle" is actionable.
- **Distinguish blocking from suggestion.** "Nit:" for style preferences. No prefix for things that must change.
- **Explain why.** "Add `.context()` here because bare `?` loses the file path" teaches. "Add context" doesn't.
- **Don't bikeshed.** If the formatter doesn't catch it, it's probably not worth a comment.

---

## AI Agent Guidance

Patterns that AI agents (Claude Code, Copilot, Cursor) consistently get wrong, validated against 2025 empirical research:

1. **Over-engineering** — wrapper types with no value, trait abstractions with one implementation, premature generalization
2. **Outdated patterns** — using deprecated libraries, old language features, patterns from 3 years ago
3. **Hallucinated APIs** — method signatures that don't exist. Always `cargo check` / compile / type-check.
4. **Clone/copy to satisfy type system** — restructure ownership instead of papering over it
5. **Comments restating code** — the code is the documentation. Delete the comment.
6. **Inconsistent error handling** — mixing error strategies within a codebase
7. **Test names like `test_add` or `it_works`** — names must describe behavior
8. **Suppressing warnings** — fix the warning, don't suppress it. `#[allow]` / `@SuppressWarnings` require justification.
9. **Adding dependencies for trivial functionality** — if it's 10 lines, write it
10. **Performing social commentary in code comments** — no "this is a great pattern" or "elegant solution". Just the code.
11. **Silent failure** — removing safety checks, swallowing errors, or generating plausible-looking but incorrect output to avoid crashing. AI produces code that *runs* but silently does the wrong thing. Worse than a crash.
12. **Hallucinated dependencies** — referencing packages that don't exist. 20% of AI code samples reference nonexistent packages. Attackers register these names (slopsquatting). Verify every dependency.
13. **Code duplication over refactoring** — generating new code blocks rather than reusing existing functions. AI doesn't propose "use the existing function at line 340." Extract and reuse.
14. **Context drift in multi-file changes** — patterns applied consistently to early files but drifting in later files as context fills. Renaming a type in 30 of 50 files. Validate consistency post-refactor.
15. **Tautological tests** — mocking the system under test, asserting on values constructed inside the test, achieving 100% coverage with near-zero defect detection. If the test can't fail when the code is wrong, it's not a test.
16. **Concurrency errors** — naive locking, missing synchronization, holding locks across await points. AI can describe race conditions but cannot diagnose them because bugs live in interleavings, not in text.
17. **Stripping existing safety checks** — removing input validation, authentication checks, rate limiting, or error boundaries during refactoring because it doesn't understand *why* they were there. Preserve every guard unless you can explain why it's unnecessary.
18. **Adding unrequested features** — padding implementations with config options, extra error variants, helper functions, and generalization nobody asked for. Implement exactly what was specified. Extra code is extra maintenance, extra surface area, and extra merge conflicts.
19. **Refactoring adjacent code** — renaming variables in untouched files, reorganizing imports in modules that aren't part of the task, adding docstrings to functions that weren't changed. Diff noise kills parallel work and obscures the actual change. Touch only what the task requires.
20. **Happy-path-only tests** — writing tests for the success case and ignoring error paths, boundary conditions, and edge cases. If every test passes a valid input and asserts on the expected output, the test suite is decorative.
