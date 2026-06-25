# Writing v2.0 Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [WORKFLOW/dispatch-prompt-missing-writing-wrap](#workflowdispatch-prompt-missing-writing-wrap)
- [WRITING/markdown-hard-break-form](#writingmarkdown-hard-break-form)

## `WORKFLOW/dispatch-prompt-missing-writing-wrap` {#workflowdispatch-prompt-missing-writing-wrap}

- Severity: `warning`
- Scope: `project:dispatch-prompt`
- Enforcer: `WORKFLOW/dispatch-prompt-missing-writing-wrap`

Outward dispatch prompts must carry the WRITING.v2.0 prevention layer before they are sent to another agent. Wrapping the prompt gives the receiver the same principle-based constraints as the originating context.

### Examples

**Good:** Wrap an outward dispatch prompt before sending it to another agent.

```text
kanon dispatch wrap standards-registry.md
```

**Bad:** Send an outward dispatch prompt without the prevention layer.

```text
dispatch-send < standards-registry.md
```

## `WRITING/markdown-hard-break-form` {#writingmarkdown-hard-break-form}

- Severity: `warning`
- Scope: `language:markdown`
- Enforcer: `WRITING/markdown-hard-break-form`
- See also: `WRITING/em-dash`

Markdown hard breaks in new prose should use the visible trailing backslash form instead of two trailing spaces. Invisible whitespace is easy to strip in editors and hard to audit in review.

### Examples

**Good:** Use a visible backslash when a Markdown line break is intentional.

```text
First line\
second line
```

**Bad:** Use invisible trailing spaces to force a Markdown hard break.

```text
First line  
second line
```

