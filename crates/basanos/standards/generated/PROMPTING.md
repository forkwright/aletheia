# Prompting Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [PROMPTING/provider-shape-required](#promptingprovider-shape-required)
- [PROMPTING/why-on-every-rule](#promptingwhy-on-every-rule)

## `PROMPTING/provider-shape-required` {#promptingprovider-shape-required}

- Severity: `warning`
- Scope: `project:kanon`
- See also: `PROMPTING/why-on-every-rule`

API prompt construction must use a typed `PromptSpec`-shaped value, not raw string concatenation. The schema separates model selection, system prompt, constraint clauses, and user content so each can be validated, versioned, and routed independently. Raw string prompts with embedded model names cannot be audited, replaced, or sovereignty-checked.

### Examples

**Good:** Declare the model, role, and constraint clauses separately in the prompt schema.

```text
PromptSpec {
    model: "current-sonnet",
    system: "You are a code reviewer...",
    constraint_clauses_load_bearing: true,
    user: "Review this diff for...",
}
```

**Bad:** Embed model selection and role in a single unstructured string with no schema.

```text
let prompt = format!("You are claude-sonnet-4-6, a code reviewer. Review this diff: {diff}");
```

## `PROMPTING/why-on-every-rule` {#promptingwhy-on-every-rule}

- Severity: `warning`
- Scope: `universal`
- See also: `PROMPTING/provider-shape-required`

Every behavioral rule in a system prompt must carry a WHY explanation. Rules without WHY are followed rigidly but not understood: the model applies them only in the literal case and fails on variants the author did not anticipate. A WHY turns a rule into a principle the model can generalize.

### Examples

**Good:** Accompany every behavioral rule with a WHY so the model can generalize it.

```text
// WHY: The model must emit structured JSON here because the downstream consumer
// parses the output with serde_json. Prose output fails the parse and drops the result.
```

**Bad:** List rules with no explanation so the model follows them rigidly but cannot apply them to novel cases.

```text
Rules:
- Always emit JSON.
- Never use markdown.
- Use snake_case keys.
```

