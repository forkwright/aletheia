# TypeScript Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [TYPESCRIPT/strict-mode-required](#typescriptstrict-mode-required)

## `TYPESCRIPT/strict-mode-required` {#typescriptstrict-mode-required}

- Severity: `warning`
- Scope: `language:typescript`

All fleet TypeScript projects must enable strict mode in tsconfig.json. Strict mode activates noImplicitAny, strictNullChecks, and strictFunctionTypes, which together eliminate large classes of runtime type errors. Turning off strict mode to skip fixing errors is not acceptable.

### Examples

**Good:** Enable strict mode in tsconfig.json and resolve every resulting error.

```text
// tsconfig.json
{ "compilerOptions": { "strict": true } }
```

**Bad:** Disable strict mode to avoid fixing implicit-any errors.

```text
// tsconfig.json
{ "compilerOptions": { "strict": false } }
```

