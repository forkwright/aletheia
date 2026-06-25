# Agent Documentation Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [WORKFLOW/llm-corpus-required](#workflowllm-corpus-required)
- [WORKFLOW/llm-schema-valid](#workflowllm-schema-valid)

## `WORKFLOW/llm-corpus-required` {#workflowllm-corpus-required}

- Severity: `warning`
- Scope: `project:repository`
- Enforcer: `WORKFLOW/llm-corpus-required`
- See also: `CONTEXT/preamble-required`

Repository roots must expose a structured `_llm/` corpus and a root `llms.txt` discovery file that points agents to it. Machine-readable LLM reference data keeps agents from rediscovering repository topology every session.

### Examples

**Good:** Expose structured LLM reference data through the repository discovery file.

```text
llms.txt
# LLM corpus
See _llm/index.toml
```

**Bad:** Publish a repository without the machine-readable LLM corpus entrypoint.

```text
README.md
# Project Overview
```

## `WORKFLOW/llm-schema-valid` {#workflowllm-schema-valid}

- Severity: `error`
- Scope: `language:toml`
- Enforcer: `WORKFLOW/llm-schema-valid`
- See also: `WORKFLOW/llm-corpus-required`

`_llm/*.toml` files must conform to their basanos LLM corpus schema. Malformed files, missing schemas, or absent required fields make the corpus unreliable for agents that depend on structured repository context.

### Examples

**Good:** Keep LLM corpus TOML aligned with its embedded schema.

```text
_llm/architecture.toml
[schema]
version = 1
```

**Bad:** Publish an LLM corpus file with required schema fields missing.

```text
_llm/architecture.toml
summary = "System map"
```

