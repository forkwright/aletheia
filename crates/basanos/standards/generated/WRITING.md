# Writing Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [WRITING/derivable-count](#writingderivable-count)
- [WRITING/double-space-after-period](#writingdouble-space-after-period)
- [WRITING/em-dash](#writingem-dash)
- [WRITING/purpose-in-technical-doc](#writingpurpose-in-technical-doc)
- [WRITING/reference-must-compress](#writingreference-must-compress)
- [WRITING/stale-date](#writingstale-date)
- [WRITING/uniform-paragraph-length](#writinguniform-paragraph-length)
- [WRITING/uniform-sentence-length](#writinguniform-sentence-length)

## `WRITING/derivable-count` {#writingderivable-count}

- Severity: `info`
- Scope: `language:markdown`
- Enforcer: `WRITING/derivable-count`

Markdown prose should not freeze precise counts that tooling can derive. Embedded counts drift as repositories evolve; cite the command, round the value, or remove the number when it is not analytically important.

### Examples

**Good:** Refer readers to the derived source instead of freezing a count.

```text
The workspace member count comes from cargo metadata.
```

**Bad:** Embed a precise count that tooling can derive.

```text
The workspace has 42 crates.
```

## `WRITING/double-space-after-period` {#writingdouble-space-after-period}

- Severity: `warning`
- Scope: `language:markdown`
- Enforcer: `WRITING/double-space-after-period`

Markdown prose uses one space after a period before the next sentence. Double spaces add noisy typography and produce inconsistent wrapped output.

### Examples

**Good:** Separate sentences with one space after the period.

```text
The parser returns a value. It logs failures.
```

**Bad:** Use two spaces after a period inside prose.

```text
The parser returns a value.  It logs failures.
```

## `WRITING/em-dash` {#writingem-dash}

- Severity: `warning`
- Scope: `language:markdown`
- Enforcer: `WRITING/em-dash`

Markdown prose should use space-hyphen-space instead of em dash characters. ASCII punctuation renders consistently across terminals, fonts, and review surfaces.

### Examples

**Good:** Use space-hyphen-space for portable punctuation.

```text
The deployment - including rollback - is documented.
```

**Bad:** Use an em dash character in Markdown prose.

```text
The deployment—including rollback—is documented.
```

## `WRITING/purpose-in-technical-doc` {#writingpurpose-in-technical-doc}

- Severity: `warning`
- Scope: `universal`
- Enforcer: `WRITING/purpose-in-technical-doc`
- See also: `WRITING/reference-must-compress`

Technical documentation describes capability, not purpose or aspiration. Vision docs may explain why work matters; README, ARCHITECTURE, CLAUDE, and module docs must state concrete behavior, inputs, outputs, and guarantees.

### Examples

**Good:** Describe concrete capability in technical documentation.

```text
The scheduler accepts cron expressions and guarantees at-most-once execution per trigger window.
```

**Bad:** Use purpose or marketing language in technical documentation.

```text
The scheduler empowers operators to manage long-running tasks elegantly.
```

## `WRITING/reference-must-compress` {#writingreference-must-compress}

- Severity: `warning`
- Scope: `universal`
- Enforcer: `WRITING/reference-must-compress`

References must compress context for an L1 reader. A citation, issue number, or document link is acceptable only when the surrounding sentence explains why the reference matters.

### Examples

**Good:** Explain why the reference matters in the sentence.

```text
Per #NNN, the registry owns generated standards views, so this prose must be derived.
```

**Bad:** Force the reader to chase a reference before the sentence has meaning.

```text
See #NNN.
```

## `WRITING/stale-date` {#writingstale-date}

- Severity: `warning`
- Scope: `language:markdown`
- Enforcer: `WRITING/stale-date`
- See also: `WRITING/derivable-count`

Markdown prose should not carry stale date-stamped claims. `as of` or `dated` assertions older than the refresh window must be verified, refreshed, or replaced with a link to the live source.

### Examples

**Good:** Point readers to the live source instead of freezing a dated claim.

```text
Current release status comes from `gh release list`.
```

**Bad:** Leave an old dated assertion in Markdown prose.

```text
As of 2026-03-01, the workspace has 42 crates.
```

## `WRITING/uniform-paragraph-length` {#writinguniform-paragraph-length}

- Severity: `warning`
- Scope: `language:markdown`
- Enforcer: `WRITING/uniform-paragraph-length`
- See also: `WRITING/uniform-sentence-length`

Markdown prose should vary paragraph length to match emphasis and explanation. Uniform paragraph size flattens rhythm and makes generated text easier to spot than the underlying idea.

### Examples

**Good:** Vary paragraph size to match emphasis and explanation.

```text
Short note.

Longer paragraph explains context with enough detail for reviewers to compare the claim, evidence, and consequence.

One sentence for emphasis.
```

**Bad:** Keep every paragraph at the same mechanical length.

```text
Each paragraph has six words here.

Every paragraph has six words here.

Another paragraph has six words here.

Final paragraph has six words here.
```

## `WRITING/uniform-sentence-length` {#writinguniform-sentence-length}

- Severity: `warning`
- Scope: `language:markdown`
- Enforcer: `WRITING/uniform-sentence-length`

Markdown prose should vary sentence length deliberately. Uniform sentence shape flattens emphasis and makes generated text easier to spot than the underlying idea.

### Examples

**Good:** Mix short emphasis with longer explanatory sentences.

```text
Short checks fail fast. Longer sentences carry the context reviewers need before they decide whether to keep reading.
```

**Bad:** Keep every sentence at the same mechanical length.

```text
The parser reads the file. The parser validates the file. The parser writes the file.
```

