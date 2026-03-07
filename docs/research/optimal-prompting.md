# Optimal Prompting & Context Injection Strategies

Research document for Aletheia's context engineering and prompt design.

---

## 1. Executive Summary

Five evidence-backed recommendations for Aletheia's prompting architecture:

1. **Put long-form data first, instructions last.** Anthropic's own testing shows queries at the end of long prompts improve response quality by up to 30%. System prompts should place reference material (identity, knowledge) above directives and task instructions. [Confirmed — Anthropic docs]

2. **Keep CLAUDE.md under 200 lines; use progressive disclosure for everything else.** Claude Code reliably follows ~100-150 custom instructions after accounting for its own system prompt (~50 instructions). Beyond that, instruction-following quality degrades uniformly. Move domain knowledge into `.claude/rules/` with path scoping and skills with on-demand loading. [Confirmed — Anthropic docs + community consensus]

3. **Use XML tags as structural boundaries, not decoration.** XML tags (`<instructions>`, `<context>`, `<identity>`) are Claude's preferred parsing mechanism for disambiguating prompt sections. They outperform markdown headers for structural separation when mixing instructions, context, and examples. Markdown works within sections; XML works between them. [Confirmed — Anthropic docs]

4. **Sub-agents should receive minimal context: task + CLAUDE.md + relevant rules only.** Anthropic's context engineering research shows sub-agents that explore extensively (tens of thousands of tokens) should return condensed summaries (1,000-2,000 tokens) to the coordinator. Aletheia should inject identity context only for persistent agents, not ephemeral sub-agents. [Confirmed — Anthropic engineering blog]

5. **Model selection by task complexity, not by default.** Opus 4.6 is for long-horizon reasoning, deep research, and extended autonomous work. Sonnet 4.6 at medium effort handles most agentic coding. Haiku 4.5 works for classification, filtering, and simple extraction. Prompting strategies are largely portable across tiers, but Opus/Sonnet 4.6 require dialing back aggressive tool-use encouragement that older models needed. [Confirmed — Anthropic docs]

---

## 2. Detailed Findings

### 2.1 System Prompt Best Practices

#### Structure and ordering

Anthropic's prompting best practices document (the single canonical reference for Claude 4.6 models) provides clear guidance on prompt structure:

**Long-form data goes at the top.** "Place your long documents and inputs near the top of your prompt, above your query, instructions, and examples. This can significantly improve performance across all models." Anthropic's testing shows "queries at the end can improve response quality by up to 30% in tests, especially with complex, multi-document inputs."

Source: [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts) [Confirmed]

**Recommended ordering for system prompts:**

```
1. Role / identity statement (1-2 sentences)
2. Long-form reference data (documents, knowledge, context)
   - Wrapped in XML tags: <documents>, <context>, <identity>
3. Instructions and rules
   - Wrapped in XML tags: <instructions>, <rules>
4. Examples (few-shot)
   - Wrapped in <example> / <examples> tags
5. Output format specification
6. Current task / query
```

This ordering exploits the U-shaped attention curve: identity and reference material benefit from primacy (beginning of context), while instructions and the active query benefit from recency (end of context).

**Confidence: Confirmed** — directly from Anthropic documentation.

#### XML tags vs markdown

XML tags are Claude's preferred structural mechanism. Anthropic states: "XML tags help Claude parse complex prompts unambiguously, especially when your prompt mixes instructions, context, examples, and variable inputs. Wrapping each type of content in its own tag (e.g. `<instructions>`, `<context>`, `<input>`) reduces misinterpretation."

Best practices:
- Use consistent, descriptive tag names across prompts
- Nest tags for natural hierarchies (`<documents>` containing `<document index="n">`)
- Use `<example>` tags to separate examples from instructions
- XML works between sections; markdown headers work within sections

Source: [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts) [Confirmed]

#### System prompt length impact

There are no published Anthropic numbers on exact performance degradation curves by system prompt length. However, multiple sources converge:

- Claude Code's system prompt consumes ~50 instruction slots, leaving ~100-150 for custom instructions
- "As instruction count increases, instruction-following quality decreases uniformly" — instruction adherence is not selective; all instructions degrade equally
- Frontier thinking LLMs can follow approximately 150-200 instructions with reasonable consistency; smaller and non-thinking models attend to fewer
- The SPRIG paper (arXiv:2410.14826) found that a single optimized system prompt performs on par with task-specific prompts across 47 task types, and combining system + task optimization produces further improvement

Source: [HumanLayer](https://www.humanlayer.dev/blog/writing-a-good-claude-md), [SPRIG paper](https://arxiv.org/abs/2410.14826) [Likely — community consensus + academic]

#### Claude 4.6-specific changes

Opus 4.6 and Sonnet 4.6 are significantly more responsive to system prompts than previous models. Key migration notes from Anthropic:

- **Dial back aggressive prompting.** "If your prompts were designed to reduce undertriggering on tools or skills, these models may now overtrigger. The fix is to dial back any aggressive language. Where you might have said 'CRITICAL: You MUST use this tool when...', you can use more normal prompting like 'Use this tool when...'"
- **Opus 4.6 does more upfront exploration.** It may gather extensive context or pursue multiple research threads unprompted. Replace blanket defaults ("Default to using [tool]") with targeted instructions ("Use [tool] when it would enhance understanding")
- **Adaptive thinking is promptable.** Large or complex system prompts may trigger excessive thinking. Add guidance like: "Extended thinking adds latency and should only be used when it will meaningfully improve answer quality"

Source: [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts) [Confirmed]

#### Published system prompt patterns

Claude Code's own system prompt is the most relevant reference. Key patterns observed:

- Role statement first ("You are Claude Code, an interactive agent...")
- Tool descriptions embedded in the system prompt as tool definitions
- Behavioral instructions use imperative mood, not suggestions
- Anti-patterns are listed explicitly ("Do NOT use the Bash to run commands when a relevant dedicated tool is provided")
- Safety and autonomy guidelines use concrete examples, not abstract principles
- `<system-reminder>` tags for runtime-injected context (context budget updates, todo reminders)

Claude's context awareness system injects token budget information via XML:
```xml
<budget:token_budget>200000</budget:token_budget>
```
And updates after each tool call:
```xml
<system_warning>Token usage: 35000/200000; 165000 remaining</system_warning>
```

Source: Claude Code system prompt (observable), [Context windows docs](https://platform.claude.com/docs/en/build-with-claude/context-windows) [Confirmed]

---

### 2.2 Optimal CLAUDE.md Structure

#### Official Anthropic guidance

Anthropic provides specific, actionable guidance for CLAUDE.md:

**Size target: under 200 lines per file.** "Longer files consume more context and reduce adherence. If your instructions are growing large, split them using imports or `.claude/rules/` files." The first 200 lines of auto-memory MEMORY.md are loaded; the rest is truncated. CLAUDE.md files are loaded in full regardless of length, but shorter files produce better adherence.

**Structure: markdown headers and bullets.** "Claude scans structure the same way readers do: organized sections are easier to follow than dense paragraphs."

**Specificity: concrete enough to verify.**
- Good: "Use 2-space indentation"
- Bad: "Format code properly"
- Good: "Run `npm test` before committing"
- Bad: "Test your changes"
- Good: "API handlers live in `src/api/handlers/`"
- Bad: "Keep files organized"

Source: [Claude Code memory docs](https://code.claude.com/docs/en/memory) [Confirmed]

#### What to include vs exclude

| Include | Exclude |
|---------|---------|
| Bash commands Claude can't guess | Anything Claude can figure out by reading code |
| Code style rules that differ from defaults | Standard language conventions Claude already knows |
| Testing instructions and preferred test runners | Detailed API documentation (link to docs instead) |
| Repository etiquette (branch naming, PR conventions) | Information that changes frequently |
| Architectural decisions specific to the project | Long explanations or tutorials |
| Developer environment quirks (required env vars) | File-by-file descriptions of the codebase |
| Common gotchas or non-obvious behaviors | Self-evident practices like "write clean code" |

Source: [Claude Code best practices](https://code.claude.com/docs/en/best-practices) [Confirmed]

**Style guidelines specifically should NOT go in CLAUDE.md.** "Never send an LLM to do a linter's job." Style rules "will inevitably add a bunch of instructions and mostly-irrelevant code snippets into your context window," degrading performance. Use hooks to run formatters/linters instead.

Source: [HumanLayer](https://www.humanlayer.dev/blog/writing-a-good-claude-md) [Likely]

#### How Claude Code processes CLAUDE.md

CLAUDE.md files are loaded at the start of every session by walking up the directory tree from the current working directory. The loading cascade:

1. Managed policy CLAUDE.md (org-level, cannot be excluded)
2. User-level `~/.claude/CLAUDE.md` (all projects)
3. Project-level `./CLAUDE.md` or `./.claude/CLAUDE.md`
4. Parent directory CLAUDE.md files (walking up from cwd)
5. Local overrides `./CLAUDE.local.md`
6. `.claude/rules/*.md` without `paths` frontmatter (loaded at startup)
7. `.claude/rules/*.md` with `paths` frontmatter (loaded on demand when matching files are opened)
8. Subdirectory CLAUDE.md files (loaded on demand)

More specific locations take precedence over broader ones. User-level rules load first; project rules load second with higher priority.

**CLAUDE.md fully survives compaction.** "After `/compact`, Claude re-reads your CLAUDE.md from disk and re-injects it fresh into the session."

**CLAUDE.md is treated as advisory context, not enforcement.** The system includes a reminder: "this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant." This means irrelevant instructions are actively deprioritized by the model.

Source: [Claude Code memory docs](https://code.claude.com/docs/en/memory) [Confirmed]

#### Progressive disclosure architecture

The recommended architecture for managing instruction volume:

**Tier 1 — CLAUDE.md (always loaded, ~60-200 lines):**
- Project identity and purpose
- Build/test commands
- Critical conventions that differ from defaults
- Pointers to other docs via `@path` imports

**Tier 2 — `.claude/rules/` (loaded at startup or on-demand with path scoping):**
- Domain-specific coding standards (scoped to file types)
- Architecture rules (scoped to directories)
- Testing conventions (scoped to test files)

**Tier 3 — Skills (loaded only when invoked or auto-triggered):**
- Repeatable workflows
- Task-specific instructions
- Domain knowledge bundles

**Tier 4 — External files (read on demand):**
- Detailed reference docs
- API specifications
- Extended examples

Source: [Claude Code docs](https://code.claude.com/docs/en/memory), [claudefa.st rules guide](https://claudefa.st/blog/guide/mechanics/rules-directory) [Confirmed + Likely]

#### Instruction budget math

- Claude Code system prompt: ~50 instructions
- Available custom instruction slots: ~100-150
- CLAUDE.md and always-on rules share this budget
- Path-scoped rules only consume budget when triggered
- Skills only consume budget when invoked

This means: CLAUDE.md should be a "slim constitution — short directives and pointers, not comprehensive documentation." Everything else goes in rules or skills.

Source: [claudefa.st](https://claudefa.st/blog/guide/mechanics/rules-directory) [Likely]

---

### 2.3 Context Injection Strategies

#### Inline vs reference

Anthropic's guidance distinguishes three strategies:

1. **Inline in system prompt**: For information needed on every turn (identity, critical rules, tool definitions). Survives compaction. Limited by instruction budget.

2. **`@path` imports in CLAUDE.md**: For reference docs that should be available at session start but are too large for CLAUDE.md proper. Expanded at launch. Good for README, package.json, architecture docs.

3. **Read-on-demand**: For detailed context only needed sometimes. Claude reads files using its tools when needed. More token-efficient but adds latency. Preferred for API docs, large reference files, domain-specific details.

**Anthropic's recommendation**: Use a "just-in-time" approach. "Rather than pre-loading all data, maintain lightweight identifiers (file paths, URLs, queries) and dynamically load via tools—mirroring human cognition of external organization systems." Claude Code demonstrates this hybrid model: upfront context (CLAUDE.md) combined with runtime exploration (glob, grep).

Source: [Context engineering blog](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) [Confirmed]

**Pointer over copy**: "Don't include code snippets in these files if possible - they will become out-of-date quickly. Instead, include `file:line` references."

Source: [HumanLayer](https://www.humanlayer.dev/blog/writing-a-good-claude-md) [Likely]

#### Priority ordering within system prompts

Based on Anthropic's guidance and the "lost in the middle" research:

**Position 1 (strongest attention — beginning):**
- Role/identity statement
- Long-form reference documents
- Static knowledge that must be reliably recalled

**Position 2 (weakest attention — middle):**
- Conversation history (managed by compaction)
- Tool results from earlier turns
- Background context that's useful but not critical

**Position 3 (strong attention — end):**
- Active instructions and rules
- Current task specification
- Output format requirements
- The user's actual query

The "lost in the middle" effect (Liu et al., 2023) demonstrated a U-shaped attention curve: LLMs attend strongly to the beginning and end of context, with a blind spot in the middle. This has been confirmed across multiple model families.

Source: [Lost in the Middle](https://direct.mit.edu/tacl/article/doi/10.1162/tacl_a_00638/119630/Lost-in-the-Middle-How-Language-Models-Use-Long) [Confirmed — academic]

**Claude-specific finding**: Claude achieves "state-of-the-art results on long-context retrieval benchmarks like MRCR and GraphWalks," but Anthropic acknowledges "these gains depend on what's in context, not just how much fits." Context quality matters more than quantity.

Source: [Context windows docs](https://platform.claude.com/docs/en/build-with-claude/context-windows) [Confirmed]

#### Token budget allocation

No published allocation percentages exist from Anthropic. Based on observed Claude Code behavior and engineering blog guidance, a practical allocation framework:

| Category | Budget Share | Rationale |
|----------|-------------|-----------|
| System prompt (identity + rules + tools) | 15-25% | Must be stable across all turns. Cached via prompt caching (0.1x read cost). |
| Skill descriptions | 2% | Hard CC limit. Overridable via `SLASH_COMMAND_TOOL_CHAR_BUDGET`. |
| Conversation history | 40-60% | Grows linearly. Managed by compaction. |
| Working context (file reads, tool results) | 20-30% | Volatile. Cleared by context editing (`clear_tool_uses_20250919`). |
| Output generation + thinking | 10-20% | Varies by task. Adaptive thinking auto-calibrates. |

**Claude Code working buffer**: Community analysis estimates a working buffer of 33K-45K tokens after system prompt, tools, and CLAUDE.md are loaded. This is the effective space for conversation + tool results.

Source: [claudefa.st context buffer analysis](https://claudefa.st/blog/guide/mechanics/context-buffer-management), Anthropic docs [Likely]

#### Diminishing returns threshold

**Context rot is real and documented.** "As token count grows, accuracy and recall degrade, a phenomenon known as context rot. This makes curating what's in context just as important as how much space is available."

Specific signals:
- Community analysis found performance degradation starting around 130K tokens (at 200K window)
- When context exceeds 50% capacity, earliest tokens begin to drop from effective attention
- Below 50%, middle-position information is most at risk
- No production model has fully eliminated position bias as of 2026

**Practical threshold**: Keep total context utilization below 60-70% of the window for reliable performance. Use compaction aggressively to maintain this.

Source: [dasroot.net analysis](https://dasroot.net/posts/2026/02/context-window-scaling-200k-tokens-help/), [Context windows docs](https://platform.claude.com/docs/en/build-with-claude/context-windows) [Likely + Confirmed]

#### Prompt caching for static context

Prompt caching is critical for Aletheia's system prompts:

- **5-minute cache**: 1.25x write, 0.1x read. Good for sessions with rapid turn-taking
- **1-hour cache**: 2x write, 0.1x read. Good for sessions with gaps between turns
- **Cache minimum tokens**: Opus 4.6 requires 4,096 tokens; Sonnet 4.6 requires 2,048; Haiku 4.5 requires 4,096
- **Cache strategy**: Place static content (identity, rules, tools) at the beginning of system prompt. These get cached on first request and read at 10% cost on subsequent turns
- **Cache invalidation**: Changing tool definitions, tool choice, images, or extended thinking settings invalidates the cache
- **Automatic caching**: For multi-turn conversations, use top-level `cache_control` and the breakpoint moves forward automatically

**Impact for Aletheia**: With SOUL.md, GOALS.md, and tool definitions potentially consuming 10-20K tokens per turn, prompt caching would reduce input costs by ~90% for repeated turns.

Source: [Prompt caching docs](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) [Confirmed]

---

### 2.4 Skill/Instruction Formatting

#### Format effectiveness

Anthropic's guidance on format:

**Numbered steps for sequential tasks.** "Provide instructions as sequential steps using numbered lists or bullet points when the order or completeness of steps matters." Numbered lists outperform prose for procedural tasks because they create explicit checkpoints.

**Prefer general instructions over prescriptive steps for reasoning.** "A prompt like 'think thoroughly' often produces better reasoning than a hand-written step-by-step plan. Claude's reasoning frequently exceeds what a human would prescribe." This means skills should specify what to achieve, not exactly how to reason about it.

**Tell Claude what to do, not what not to do.** Instead of "Do not use markdown in your response," try "Your response should be composed of smoothly flowing prose paragraphs." Positive instructions produce more reliable behavior than negations.

**Degrees of freedom framework.** Anthropic's skill best practices introduce a calibration model:
- **High freedom** (text instructions): When multiple approaches are valid. Example: code review checklists.
- **Medium freedom** (pseudocode with parameters): When a preferred pattern exists but variation is acceptable.
- **Low freedom** (exact scripts, no parameters): When operations are fragile. Example: database migrations.

The analogy: "Think of Claude as a robot exploring a path. Narrow bridge with cliffs: provide specific guardrails (low freedom). Open field: give general direction (high freedom)."

Source: [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts), [Skill best practices](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices) [Confirmed]

#### Skill description quality

Skill matching in Claude Code is pure LLM reasoning — no embeddings, classifiers, or pattern matching. The `name` and `description` fields are formatted into an `<available_skills>` XML section injected into the `Skill` meta-tool's prompt. Claude's forward pass decides whether any skill is relevant.

This means:
- **Description quality is the entire routing mechanism.** A bad description = a skill that never triggers
- Write descriptions in third person ("Processes Excel files" not "I process Excel files")
- Keep descriptions single-line — CC's indexer doesn't parse YAML multiline correctly
- Descriptions must be under 1,024 characters, no XML tags
- Skill descriptions share a 2% context budget (~16,000 chars). Excess skills get dropped

Source: [CC Skill Format research](cc-skill-format.md), [CC Skills docs](https://code.claude.com/docs/en/skills) [Confirmed]

#### Few-shot examples within skills

Anthropic confirms examples are "one of the most reliable ways to steer Claude's output format, tone, and structure." Guidelines:

- **Include 3-5 examples for best results**
- Make them relevant (mirror actual use case), diverse (cover edge cases), and structured (wrap in `<example>` tags)
- Examples inside `<thinking>` tags within few-shot prompts teach Claude reasoning patterns that generalize to its own extended thinking
- **Token cost tradeoff**: For skills loaded on-demand, the token cost of examples is paid only when the skill is invoked. For always-loaded instructions, examples are expensive. Use examples in skills, not in CLAUDE.md.

Source: [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts) [Confirmed]

#### Conditional skill activation

Two mechanisms for conditional skills:

1. **Path-scoped rules** (`.claude/rules/` with `paths` frontmatter): Load only when Claude works with matching files. Best for conventions that apply to specific file types or directories.

2. **Skill descriptions with trigger words**: Write descriptions that contain the terms a user would naturally use. "Diagnose and fix clippy lint errors in a Rust crate" naturally activates on prompts containing "clippy," "lint," or "cargo clippy."

3. **`disable-model-invocation: true`**: Prevents automatic triggering; requires explicit `/skill-name` invocation. Use for workflows with side effects.

Source: [CC Skills docs](https://code.claude.com/docs/en/skills), [CC memory docs](https://code.claude.com/docs/en/memory) [Confirmed]

#### Skill file size

Anthropic recommends keeping SKILL.md under 500 lines. Move detailed content to supporting files referenced via markdown links. Keep references one level deep from SKILL.md (avoid nested chains). Claude reads files on-demand when referenced.

Source: [Anthropic best practices](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices) [Confirmed]

---

### 2.5 Multi-Agent Context Strategies

#### Minimum viable context for sub-agents

Anthropic's context engineering blog establishes the pattern: sub-agents explore extensively (tens of thousands of tokens) but return condensed summaries (1,000-2,000 tokens) to the lead coordinator. This provides "clear separation of concerns."

For Claude Code sub-agents, the minimum viable context is:
1. The task prompt (the delegating message)
2. CLAUDE.md (automatically loaded)
3. Preloaded skills (if specified in agent definition)
4. Auto memory MEMORY.md (first 200 lines, if enabled)

Sub-agents do NOT receive:
- Parent conversation history
- Parent's loaded skills (must be listed explicitly via `skills` field)
- Parent's file reads or tool results

Source: [Context engineering blog](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents), [CC Subagents docs](https://code.claude.com/docs/en/sub-agents) [Confirmed]

#### Role-specific context structuring

Anthropic's long-running agents research uses two distinct prompt types:

1. **Initializer agent**: Environment setup prompt. Creates `init.sh`, progress tracking file, feature specification. Focused on scaffolding.

2. **Coding agent**: Implementation prompt. Reads progress files and git logs, implements features incrementally, commits work.

For Aletheia's sub-agent roles, this maps to:

| Role | Context Profile | What to Include | What to Exclude |
|------|----------------|-----------------|-----------------|
| **Researcher** (Explore agent) | Read-only, broad scope | Task question, relevant file paths, search hints | Implementation details, editing instructions |
| **Coder** (general-purpose) | Full tools, focused scope | Task spec, affected files, test commands, coding standards | Unrelated project context, research results beyond what's needed |
| **Reviewer** (Explore/Plan agent) | Read-only, critical eye | Code diff, acceptance criteria, relevant standards | Implementation history, alternative approaches |
| **Planner** (Plan agent) | Read-only, architectural | System architecture, constraints, prior decisions | Implementation details, test fixtures |

Source: [Long-running agents blog](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) [Confirmed]

#### Sub-agent orchestration patterns

Opus 4.6 has a "strong predilection for subagents and may spawn them in situations where a simpler, direct approach would suffice." Anthropic recommends explicit guidance:

"Use subagents when tasks can run in parallel, require isolated context, or involve independent workstreams that don't need to share state. For simple tasks, sequential operations, single-file edits, or tasks where you need to maintain context across steps, work directly rather than delegating."

Source: [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts) [Confirmed]

#### Context passing without token bloat

Five strategies for efficient parent-to-child context transfer:

1. **Structured task files**: Write task specification to a file (JSON preferred over markdown — "the model is less likely to inappropriately change or overwrite JSON files compared to Markdown files"). Sub-agent reads the file at start.

2. **Git as state**: Use git commit logs + progress files. Sub-agent reads these to understand current state without inheriting full conversation history.

3. **Condensed summaries**: Lead agent summarizes findings before delegating. The summary, not the raw research, goes to the implementer.

4. **Filesystem discovery**: Claude 4.6 models are "extremely effective at discovering state from the local filesystem." Rather than passing context through messages, let sub-agents discover it from CLAUDE.md, code, and git logs. This is fundamentally cheaper than message-based context transfer.

5. **Persistent sub-agent memory**: The `memory` field on sub-agents enables cross-session learning without re-passing context. The sub-agent gets a persistent directory (e.g., `~/.claude/agent-memory/<name>/`) with first 200 lines of `MEMORY.md` auto-injected into its system prompt each session.

**Chain of Agents pattern** (Google, NeurIPS 2024): For long-context processing, split text into chunks processed by sequential worker agents, each passing a summary to the next. A manager agent synthesizes the final result. Outperformed both RAG and long-context LLMs. Relevant for Aletheia's cross-agent topology.

**Anti-pattern — full trace sharing**: Cognition's engineering team warns that copying entire conversation traces to sub-agents is impractical in production. Sub-agents need visibility into prior decisions but not raw traces. Use structured summaries or filesystem state instead.

Source: [Long-running agents blog](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents), [Chain of Agents (NeurIPS 2024)](https://research.google/blog/chain-of-agents-large-language-models-collaborating-on-long-context-tasks/), [Cognition](https://cognition.ai/blog/dont-build-multi-agents) [Confirmed + Likely]

---

### 2.6 Model-Specific Differences

#### Capability tiers

| Dimension | Opus 4.6 | Sonnet 4.6 | Haiku 4.5 |
|-----------|----------|------------|-----------|
| **Best for** | Long-horizon reasoning, deep research, extended autonomous work | Most agentic coding, fast turnaround, cost-efficient tasks | Classification, filtering, simple extraction, fast codebase search |
| **Context window** | 200K (1M beta) | 200K (1M beta) | 200K |
| **Max output tokens** | 128K | 64K | 64K |
| **Thinking** | Adaptive only (manual deprecated) | Adaptive + manual + interleaved | Manual with budget_tokens |
| **Context awareness** | No | Yes | Yes |
| **Prompt caching min** | 4,096 tokens | 2,048 tokens | 4,096 tokens |
| **Input price (per 1M)** | $5 | $3 | $1 |
| **Output price (per 1M)** | $25 | $15 | $5 |

Source: [Anthropic model overview](https://platform.claude.com/docs/en/about-claude/models/overview) [Confirmed]

#### Prompting differences by model

**Prompting strategies are largely portable.** The SPRIG paper found that optimized system prompts "generalize effectively across model families, parameter sizes, and languages." Anthropic's prompting best practices document applies to all three tiers.

Key model-specific adjustments:

**Opus 4.6:**
- Most responsive to system prompt. Overtriggers on aggressive language. Use natural phrasing.
- Does extensive upfront exploration. May need "choose an approach and commit to it" guidance.
- Adaptive thinking only — manual `budget_tokens` is deprecated.
- Has a tendency to over-engineer. Needs explicit "keep solutions minimal" guidance.

**Sonnet 4.6:**
- Defaults to `high` effort (may cause higher latency). Explicitly set effort for your use case.
- Most versatile: supports adaptive thinking, manual extended thinking, and interleaved thinking.
- Best cost-performance tradeoff for agentic coding at `medium` effort.
- "For the hardest, longest-horizon problems... Opus 4.6 remains the right choice."

**Haiku 4.5:**
- Supports extended thinking with manual `budget_tokens`.
- Has context awareness (like Sonnet 4.6, unlike Opus 4.6).
- Best for Explore agent type (fast codebase search/analysis).
- Thinking block summarization behaves the same as other Claude 4 models.

Source: [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts), [Model overview](https://platform.claude.com/docs/en/about-claude/models/overview) [Confirmed]

#### When Opus is worth the cost

Anthropic's explicit guidance: use Opus 4.6 for "the hardest, longest-horizon problems (large-scale code migrations, deep research, extended autonomous work)."

Evidence-based thresholds for model selection:

| Task Type | Recommended Model | Rationale |
|-----------|-------------------|-----------|
| Multi-file refactoring across a codebase | Opus 4.6 | Long-horizon reasoning, state tracking across many files |
| Single-file feature implementation | Sonnet 4.6 (medium effort) | Sufficient capability, 40% cheaper |
| Code review and analysis | Sonnet 4.6 (medium effort) | Read-heavy, no extended thinking needed |
| Codebase exploration / search | Haiku 4.5 | Speed matters more than depth; CC uses Haiku for Explore agents |
| Classification / filtering | Haiku 4.5 | Simple task, 5x cheaper than Opus |
| Research synthesis across many sources | Opus 4.6 | Requires maintaining coherence across large context |
| Bug fix with clear reproduction | Sonnet 4.6 (low-medium effort) | Scoped task, doesn't need maximum reasoning |

Source: [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts) [Confirmed + Likely]

#### Context degradation by model

No published data on model-specific degradation curves. However:
- All three models share the same architecture family and are subject to the same "lost in the middle" attention patterns
- Haiku 4.5's context awareness feature (shared with Sonnet 4.6) gives it explicit token budget tracking, which may help it manage context more efficiently than Opus 4.6 (which lacks this feature)
- Anthropic's decision to give Haiku and Sonnet context awareness but not Opus suggests Opus may handle long contexts better natively

**Confidence: Speculative** — no published evidence on differential degradation.

---

## 3. Concrete Recommendations for Aletheia

### 3.1 System prompt structure for nous agents

Aletheia's `nous::bootstrap` should assemble system prompts in this order:

```
<identity>
  [SOUL.md content — personality, principles, name]
  [IDENTITY.md — name, emoji, model string]
</identity>

<knowledge>
  [GOALS.md — active objectives]
  [MEMORY.md — persistent knowledge, first 200 lines]
  [Domain pack context from thesauros]
  [Knowledge graph recall results from mneme]
</knowledge>

<instructions>
  [Core behavioral rules]
  [Tool usage guidelines]
  [Output formatting preferences]
</instructions>

<context>
  [Current session state from CONTEXT.md]
  [Distillation summary from melete, if any]
  [Active conversation context]
</context>
```

**Key decisions:**
- Identity and knowledge go first (primacy benefit)
- Instructions go after knowledge (recency benefit over middle)
- Session context goes last (most recent, strongest attention)
- Use XML tags for section boundaries, markdown within sections
- Cache breakpoint after `</instructions>` — everything above is static within a session

### 3.2 CLAUDE.md structure for CC sub-agents

When Aletheia spawns CC sub-agents, generate a CLAUDE.md tailored to the task:

```markdown
# [Agent Name] — [Role Description]

## Identity
[1-2 sentences: who this agent is, what it serves]

## Task Context
[What the agent needs to know about the current task]

## Standards
@.claude/rules/rust.md

## Commands
- Build: `cargo build`
- Test: `cargo test -p <crate>`
- Lint: `cargo clippy --workspace`

## Constraints
[3-5 specific rules that must not be violated]
```

Target: 40-80 lines. Everything else goes in rules files or skills.

### 3.3 Token budget allocation

For a 200K token context window:

| Component | Tokens | % | Caching |
|-----------|--------|---|---------|
| System prompt (identity + rules + tools) | 15-25K | 8-12% | Yes (5-min or 1-hour) |
| Skill descriptions | ~4K | 2% | Yes (part of system prompt) |
| CLAUDE.md + rules | 3-8K | 2-4% | Yes (survives compaction) |
| Active skill content (when invoked) | 2-10K | 1-5% | No |
| Conversation history | 80-120K | 40-60% | Yes (automatic) |
| Tool results (file reads, search) | 30-50K | 15-25% | Cleared by context editing |
| Output + thinking | 20-40K | 10-20% | N/A |

**Trigger compaction** when conversation history exceeds 60% of window (roughly 120K tokens at 200K window).

### 3.4 Model selection policy

| Agent Type | Model | Effort | Thinking |
|------------|-------|--------|----------|
| Core nous (Syn, persistent agents) | Opus 4.6 | high | adaptive |
| Task execution sub-agent | Sonnet 4.6 | medium | adaptive |
| Code review sub-agent | Sonnet 4.6 | medium | adaptive |
| Codebase exploration | Haiku 4.5 | — | enabled, budget 8K |
| Fact extraction (mneme) | Sonnet 4.6 | low | enabled, budget 4K |
| Classification / filtering | Haiku 4.5 | — | disabled |

### 3.5 Skill definition format for Aletheia

```yaml
---
name: <kebab-case-name>
description: <single-line, third person, under 1024 chars, trigger-rich>
allowed-tools: Read, Grep, Edit, Bash
model: inherit
context: fork  # or inline
agent: general-purpose  # or Explore, Plan
---

## Purpose
[1-2 sentences: what this skill does and when to use it]

## Steps
1. [Concrete action]
2. [Concrete action]
   a. [Sub-step if needed]
3. [Verification step]

## Parameters
- `$ARGUMENTS` — [what the user passes]
- `${CLAUDE_SKILL_DIR}` — [if skill needs to reference its own files]

## Examples
<examples>
<example>
Input: [sample input]
Expected behavior: [what the agent should do]
</example>
</examples>

## Anti-patterns
- [What NOT to do, with rationale]
```

Keep under 500 lines. Reference supporting files via relative markdown links.

---

## 4. Example Templates

### 4.1 Core agent CLAUDE.md (for persistent nous like Syn)

```markdown
# Aletheia Agent Configuration

## Identity
You are [name], a persistent agent in the Aletheia distributed cognition system.
Your operator is [operator_name]. Your relationship is [relationship_type].

## Architecture
This is a Rust workspace with 17 crates. Key crates:
- nous: agent pipeline and actor model
- mneme: knowledge graph and memory
- hermeneus: LLM provider integration
- organon: tool registry

## Commands
- Build: `cargo build`
- Test: `cargo test -p <crate>`
- Lint: `cargo clippy --workspace`
- Single crate test: `cargo test -p aletheia-<crate>`

## Standards
@.claude/rules/rust.md
@.claude/rules/git-workflow.md

## Conventions
- Greek naming for modules/crates (docs/gnomon.md)
- snafu for error handling, not thiserror
- pub(crate) by default, pub only for cross-crate API
- #[expect(lint, reason)] over #[allow]

## Constraints
- Never use unwrap() in library code
- Never push to upstream — only push to origin
- All test data uses synthetic identities
```

### 4.2 Ephemeral sub-agent prompt

```markdown
# Task: [Brief Description]

## Context
[2-3 sentences about what this sub-agent needs to know]

## Deliverable
[Exactly what to produce]

## Constraints
- [Scope limitation]
- [Quality requirement]
- Return findings as a condensed summary (under 2000 tokens)

## Files to examine
- `path/to/relevant/file.rs`
- `path/to/another/file.rs`
```

### 4.3 Research sub-agent prompt

```markdown
# Research: [Topic]

You are a researcher. Investigate thoroughly and return structured findings.
Do not modify any files.

## Questions
1. [Specific question]
2. [Specific question]

## Sources to check
- [File paths]
- [URLs if web search available]

## Output format
For each finding:
- Claim: [what you found]
- Source: [where you found it]
- Confidence: [confirmed/likely/speculative]
```

---

## 5. Confidence Levels

| Recommendation | Confidence | Evidence Basis |
|---------------|------------|----------------|
| Long-form data first, instructions last | **Confirmed** | Anthropic docs: "queries at the end can improve response quality by up to 30%" |
| CLAUDE.md under 200 lines | **Confirmed** | Anthropic docs: explicit size target |
| XML tags for section boundaries | **Confirmed** | Anthropic docs: recommended for "complex prompts" |
| ~100-150 custom instruction budget | **Likely** | Community analysis (HumanLayer, claudefa.st), consistent with observed behavior |
| Sub-agents return 1K-2K token summaries | **Confirmed** | Anthropic context engineering blog |
| Context degradation at 130K+ tokens | **Likely** | Community analysis (dasroot.net), consistent with academic research |
| 60-70% context utilization target | **Likely** | Derived from degradation research, not explicitly stated by Anthropic |
| Prompt caching 90% cost reduction | **Confirmed** | Anthropic docs: cache reads at 0.1x base price |
| Model-specific context degradation curves | **Speculative** | No published data comparing degradation across tiers |
| Style guidelines don't belong in CLAUDE.md | **Likely** | Community consensus (HumanLayer), supported by instruction budget reasoning |
| SPRIG: one optimized system prompt = task-specific prompts | **Confirmed** | Academic paper, tested on 47 tasks |
| Examples most effective at 3-5 count | **Confirmed** | Anthropic docs: explicit recommendation |
| Lost-in-the-middle U-shaped curve | **Confirmed** | Academic (Liu et al. 2023), confirmed across model families |
| Opus 4.6 overtriggers with aggressive prompting | **Confirmed** | Anthropic docs: explicit migration guidance |
| Haiku for Explore agents | **Confirmed** | CC default behavior: Explore agent type uses Haiku |
| JSON preferred over markdown for state files | **Confirmed** | Anthropic long-running agents blog |

---

## 6. Raw Source Links

### Anthropic Official Documentation

| Source | Content |
|--------|---------|
| [Prompting best practices](https://platform.claude.com/docs/en/docs/build-with-claude/prompt-engineering/system-prompts) | Canonical prompt engineering reference for Claude 4.6 models. Covers clarity, XML tags, roles, long context, tool use, thinking, agentic systems. |
| [Context windows](https://platform.claude.com/docs/en/build-with-claude/context-windows) | Context window mechanics, 1M beta, context awareness, compaction, context rot acknowledgment. |
| [Prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) | Cache tiers, pricing, breakpoint strategies, minimum token requirements, invalidation rules. |
| [CC Memory docs](https://code.claude.com/docs/en/memory) | CLAUDE.md structure, loading cascade, auto memory, rules directory, progressive disclosure. |
| [CC Best practices](https://code.claude.com/docs/en/best-practices) | CLAUDE.md writing, session management, subagent usage, context management, common failure patterns. |
| [CC Skills docs](https://code.claude.com/docs/en/skills) | Skill format, frontmatter fields, matching algorithm, context budget, dynamic context injection. |
| [CC Subagents docs](https://code.claude.com/docs/en/sub-agents) | Subagent creation, memory, tool access, skill preloading. |
| [Model overview](https://platform.claude.com/docs/en/about-claude/models/overview) | Model capabilities, pricing, context windows, thinking support. |

### Anthropic Engineering Blog

| Source | Content |
|--------|---------|
| [Effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) | Context as finite resource, just-in-time loading, sub-agent summary patterns, compaction strategies. |
| [Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) | Multi-session design, progress tracking, initializer vs coder agents, JSON state files, testing integration. |

### Academic & Community

| Source | Content |
|--------|---------|
| [Lost in the Middle (Liu et al.)](https://direct.mit.edu/tacl/article/doi/10.1162/tacl_a_00638/119630/Lost-in-the-Middle-How-Language-Models-Use-Long) | U-shaped attention curve in LLMs. Primacy/recency effects on retrieval accuracy. |
| [SPRIG (arXiv:2410.14826)](https://arxiv.org/abs/2410.14826) | Optimized system prompts generalize across models, tasks, and languages. |
| [HumanLayer: Writing a good CLAUDE.md](https://www.humanlayer.dev/blog/writing-a-good-claude-md) | Instruction budget analysis, progressive disclosure, pointer-over-copy principle. |
| [claudefa.st: Rules directory guide](https://claudefa.st/blog/guide/mechanics/rules-directory) | Path-scoped rules, instruction budget math, CLAUDE.md as slim constitution. |
| [claudefa.st: Context buffer management](https://claudefa.st/blog/guide/mechanics/context-buffer-management) | 33K-45K token working buffer estimate for CC sessions. |
| [dasroot.net: Context window scaling](https://dasroot.net/posts/2026/02/context-window-scaling-200k-tokens-help/) | Performance degradation starting at 130K+ tokens. |
| [Chain of Agents (NeurIPS 2024)](https://research.google/blog/chain-of-agents-large-language-models-collaborating-on-long-context-tasks/) | Sequential worker agents with summary passing. Outperforms RAG and long-context LLMs. |
| [Cognition: Don't Build Multi-Agents](https://cognition.ai/blog/dont-build-multi-agents) | Anti-pattern warnings for multi-agent context sharing. Filesystem discovery over trace copying. |
| [Skill authoring best practices](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices) | Degrees of freedom framework, progressive disclosure, description optimization, anti-patterns. |
| [CC Skill Format research](cc-skill-format.md) | Internal research doc — skill matching, invocation flow, context budget, CC-native export requirements. |
| [Model Capability Audit](model-capability-audit.md) | Internal research doc — native vs built capabilities, redundancy analysis. |
