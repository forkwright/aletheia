# Agentic Pipeline Standard

> Canonical description of the aletheia dispatch pipeline: the closed loop from
> planner through dispatcher, workers, QA, CI, and triage. Read this before
> working on energeia, the orchestrator, or the steward.

---

## The loop

```
planner
  │  execution plan (DAG, ordered batches)
  ▼
dispatcher (energeia)
  │  prompts → sessions
  ▼
workers (CC agents)
  │  PRs
  ▼
QA gate (energeia qa)
  │  verdict
  ▼
CI (GitHub Actions)
  │  status
  ▼
steward (energeia steward)
  │  merge / fix / escalate
  ▼
triage
  │  issues, lessons, next plan
  └──────────────────────────► planner
```

Each arrow is a hand-off. Each box is a module boundary with typed inputs and
outputs. The loop closes at triage: unresolved failures become issues that feed
the next planning cycle.

---

## Prompt as atomic unit

A prompt is a self-contained work order. The worker that picks it up has zero
prior context beyond what the prompt supplies. Every prompt must include:

- **Task** — what to build or fix (last, after all context).
- **Blast radius** — which files and crates are in scope. This is the
  authorization boundary, not an advisory: the worker stays inside it.
- **Acceptance criteria** — machine-verifiable conditions. Each criterion
  maps to a QA check. Vague criteria produce vague verdicts.
- **Standards reference** — which standards govern this work. At minimum
  `standards/STANDARDS.md`, `standards/RUST.md`, and `AGENTS.md`.
- **Dependencies** — prompt numbers this prompt depends on. Unmet
  dependencies block dispatch.

Prompts live in YAML frontmatter files. See `workflow/prompts/` for the
canonical location and `crates/energeia/src/prompt.rs` for the loader.

---

## Execution plan as dependency DAG

Plans are not flat lists. Prompts form a DAG: some work must finish before
other work can start. The dispatcher computes the topological frontier (the
set of prompts whose dependencies are all satisfied) and dispatches them in
parallel, then advances to the next frontier batch.

Rules for plan authors:

- **Partition by blast radius.** No two prompts in the same batch touch the
  same files, because parallel workers will conflict.
- **Keep batches small.** 4–6 prompts per batch is the working limit for a
  single dispatch run. Larger batches increase conflict surface and QA cost.
- **Order by dependency, not by preference.** If prompt B reads output from
  prompt A, declare the dependency. The DAG enforces order; the author
  should not.

---

## Blast radius as authorization boundary

The blast radius in a prompt is not a suggestion. It defines what the worker
is authorized to touch. Files outside the blast radius are off-limits.

Why this matters:

- Parallel agents working on overlapping files will create merge conflicts
  that block the entire queue.
- QA mechanical checks flag blast-radius violations as hard failures
  (`MechanicalIssueKind::BlastRadiusViolation`).
- Steward will not merge a PR with an unresolved blast-radius violation.

When a task genuinely requires changes beyond the declared blast radius,
the prompt is wrong. Fix the plan, not the boundary.

---

## Verdict types as control signals

QA produces one of three verdicts. The dispatcher uses verdicts as control
signals, not just labels.

| Verdict | Meaning | Dispatcher action |
|---------|---------|-------------------|
| `Pass` | All criteria met, no mechanical issues | Steward merges |
| `Partial` | Some criteria met, some failed | Escalate to operator for review |
| `Fail` | Hard failure: mechanical issues or all criteria failed | Re-queue with corrective context, or file issue |

`Partial` is not a slow path to `Pass`. If a prompt consistently produces
`Partial`, the acceptance criteria or the prompt itself is ambiguous.
Fix the prompt.

---

## Observations: ephemeral to tracked

During a dispatch run, workers and QA produce observations: findings,
anomalies, patterns, friction. These are ephemeral until triage.

At triage, observations are classified:

- **Structural fix needed** → GitHub issue + follow-up prompt.
- **Lesson learned** → entry in `docs/LESSONS-LEARNED.md`.
- **False positive** → note in QA calibration log.

Observations that are never triaged become noise. Triage is not optional;
it is the step that closes the loop and prevents the same failure from
recurring.

See `docs/LESSONS-LEARNED.md` for the lessons capture format.

---

## Stage-by-stage reference

The energeia pipeline (`crates/energeia/src/pipeline/`) runs five stages:

| Stage | Module | What it does |
|-------|--------|-------------|
| Validation | `validation.rs` | Preflight: non-empty prompt list, unique numbers, valid DAG |
| Preparation | `preparation.rs` | Build DAG, compute first frontier, initialize shared state |
| Health check | `health_check.rs` | Probe backend reachability before spawning sessions |
| Execution | `execution.rs` | Drive frontier group loop, collect session outcomes |
| Post-processing | `post_processing.rs` | Record metrics, assemble result, persist store record |

QA runs after execution as a separate gate, not as a pipeline stage, because
QA cost is per-prompt and may be skipped for sessions that did not produce a PR.

---

## Model tier selection

Match the model tier to the task complexity. Using Opus for mechanical work
wastes budget; using Haiku for architecture produces low-quality output.

| Tier | Model | When |
|------|-------|------|
| Architecture | Opus 4 | System design, cross-crate refactors, plan authoring |
| Execution | Sonnet 4.6 | Standard feature work, bug fixes, documentation |
| Mechanical | Haiku 4.5 | Lint fixes, formatting, dependency bumps |

The dispatcher reads the model tier from the prompt's YAML frontmatter
(`model_tier` field). Absent the field, it defaults to Sonnet.

---

## QA sub-agent requirement

Every dispatch prompt should specify at least one QA criterion. Prompts with
zero acceptance criteria produce `Pass` verdicts vacuously — the QA gate
has nothing to evaluate.

For complex PRs (cross-crate, new subsystems, security-sensitive), a
semantic QA sub-agent review is required before merge. The `semantic_evaluated`
flag in `QaResult` records whether this ran.

---

## See also

- `crates/energeia/src/types.rs` — DispatchSpec, QaResult, QaVerdict, Budget
- `crates/energeia/src/pipeline/` — concrete stage implementations
- `crates/energeia/src/qa/` — QaGate trait and mechanical checks
- `crates/energeia/src/steward/` — CI management and merge pipeline
- `docs/LESSONS-LEARNED.md` — lessons capture format
- `standards/PROMPTING.md` — API-level prompt construction (system prompts, XML tags, caching)
- `workflow/prompts/templates/` — concrete agent-role prompt templates
