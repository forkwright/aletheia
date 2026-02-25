# Svelte Rules

Agent-action rules for Svelte 5 components in `ui/`.

---

## Runes Only

Use Svelte 5 rune syntax exclusively. Never use legacy reactive declarations.

Compliant:
```svelte
<script lang="ts">
  let count = $state(0);
  let doubled = $derived(count * 2);
  let { label, onSubmit } = $props<{ label: string; onSubmit: () => void }>();

  $effect(() => {
    document.title = label;
  });
</script>
```

Non-compliant:
```svelte
<script lang="ts">
  export let label: string;     // legacy prop
  let count = 0;
  $: doubled = count * 2;       // legacy reactive declaration
</script>
```

Never use `$storeName` auto-subscription syntax inside `<script>` blocks in runes mode.

See: docs/STANDARDS.md#rule-svelte-5-runes-only-no-legacy-reactive-syntax

---

## No @html with Unsanitized Content

Never use `{@html}` with user-supplied or externally-sourced content without explicit sanitization.

Compliant:
```svelte
<!-- Static, developer-controlled markdown -->
{@html marked(staticMarkdown)}

<!-- User content — sanitized first -->
{@html DOMPurify.sanitize(userInput)}
```

Non-compliant:
```svelte
{@html userMessage}
{@html apiResponse.content}
```

Prefer template expressions over `{@html}` wherever possible.

See: docs/STANDARDS.md#rule-no-xss-via-html

---

## Typed Props

Type all component props via `$props<{ ... }>()`. Never use untyped or implicitly-any props.

Compliant:
```svelte
<script lang="ts">
  interface Props {
    agentId: string;
    isLoading?: boolean;
    onSubmit: (message: string) => void;
  }
  let { agentId, isLoading = false, onSubmit } = $props<Props>();
</script>
```

Non-compliant:
```svelte
<script lang="ts">
  let { agentId, isLoading, onSubmit } = $props(); // no type parameter
</script>
```

See: docs/STANDARDS.md#rule-typed-component-props

---

## svelte-check is Blocking

Treat all `svelte-check` warnings as errors. Never suppress with `@ts-ignore` without a documented reason.

Compliant: zero `svelte-check` warnings in CI (`npx svelte-check --fail-on-warnings`).

Non-compliant:
```svelte
// @ts-ignore
let { undocumentedProp } = $props();
```

See: docs/STANDARDS.md#rule-svelte-check-warnings-are-errors
