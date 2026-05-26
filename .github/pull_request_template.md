## What

One-paragraph summary of what this PR does.

## Why

Problem this solves, or spec phase this implements.

## Changes

-

## Spec

<!-- Delete if not applicable -->
Spec: NN Phase N

## Testing

How this was tested. Include relevant commands or test output.

### Topology check (for new components)

If this PR adds a new module, function, or abstraction:
- What edge does it create between existing components?
- Could the same capability be added by extending an existing edge instead?

## Checklist

- [ ] `cargo test -p <affected-crate>` passes
- [ ] `cargo clippy --workspace` passes with zero warnings
- [ ] `kanon lint --summary --writing` and `kanon lint --summary --workflow` pass (workspace-level gates)
- [ ] For affected crates at zero rust violations: `kanon lint --summary --rust crates/<name>` passes (crate-scoped gate)
- [ ] New functionality has tests
- [ ] No secrets or credentials in the diff
- [ ] Commit message follows convention (`type(scope): description`)
- [ ] Binary decisions preserve informative tension (see `docs/ARCHITECTURE.md#preserving-informative-tension`)
- [ ] Architecture facts touched (list fact IDs below; if none, confirm via `kanon mcp architecture_fact list --scope <crate>`)

<!-- List any architecture fact IDs added or updated, e.g.: aletheia.spawn.model, aletheia.eidos.dependency-direction -->
