# Memory benchmark results: LongMemEval and LoCoMo

> **Maturity: provisional — no live results yet.**
> The benchmark runner is implemented and tested. Live evaluations against LongMemEval and LoCoMo have not been executed. All result tables below are empty placeholders. Do not cite this document as evidence of measured recall quality.

**Status:** Runner implemented, awaiting live run. See [Prerequisites](#prerequisites) before executing.

**Issue:** [#2854](https://github.com/forkwright/aletheia/issues/2854)
**Runner PRs:** [#3090](https://github.com/forkwright/aletheia/pull/3090) (dataset parsers + scoring), [#3091](https://github.com/forkwright/aletheia/pull/3091) (live runner)

---

## What the runner provides

The `dokimion` crate (`crates/eval/`) implements a full benchmark loop for
measuring aletheia's memory pipeline against published recall benchmarks.

### Architecture

```
BenchmarkRunner (crates/eval/src/benchmarks/runner.rs)
  ├── EvalClient  — HTTP client for the aletheia pylon API
  ├── LongMemEvalDataset (crates/eval/src/benchmarks/longmemeval.rs)
  │     JSON parser: question_id, question_type, question, answer,
  │     answer_alternatives, haystack_sessions
  ├── LocomoDataset (crates/eval/src/benchmarks/locomo.rs)
  │     JSON parser: sample_id, conversation (session_N → turns),
  │     qa pairs with category + answer_alternatives
  └── score_answer (crates/eval/src/benchmarks/metrics.rs)
        EM  — lowercase, punctuation-stripped exact match
        F1  — token-level (multiset intersection) harmonic mean
        Contains — expected as substring of actual
```

### Per-question flow (live runner)

1. `POST /api/v1/sessions` - create a session keyed to the question and
   `eval_run_id` (official-parity mode), or reuse the single
   continuous-memory session
2. `POST /api/v1/knowledge/ingest` - seed the full haystack transcript into
   the knowledge store as markdown. Every turn keeps its original role
   (`user`, `assistant`, `system`, `tool`, or dataset speaker labels) so the
   memory pipeline sees the complete conversation without replaying the
   turns as user messages and contaminating the answer signal.
3. `POST /api/v1/sessions/{id}/messages` - ask the benchmark question
4. Collect the SSE stream; extract concatenated `text_delta` events
5. Score the answer with `score_answer(actual, expected_answers)`
6. In official-parity mode, `DELETE /api/v1/sessions/{id}` closes the
   session after the question. In continuous-memory mode the session stays
   open so earlier questions remain in context.

Per-question ingestion errors are surfaced in the question result instead
of being silently ignored. Other per-question errors are logged and scored
as zero - a network hiccup does not abort the entire run. The runner
produces a `BenchmarkReport` tagged with `eval_run_id` and per-question
`id`, plus `exact_match_rate()`, `mean_f1()`, and a `per_category()`
breakdown. CLI-generated reports also attach bootstrap confidence intervals
and a `publishability` assessment so archived output is explicit about whether
it is suitable for publication.

### Test coverage (already passing)

The runner has 188 passing tests (`cargo test -p dokimion`):

| Scope | Count | Notes |
|---|---|---|
| Dataset parsers (LongMemEval + LoCoMo) | 14 | JSON format, alternates, multi-session, error cases |
| Scoring (EM, F1, contains) | 10 | exact, normalized, partial, substring, duplicates |
| Report aggregation | 3 | empty, EM+F1 math, per-category grouping |
| Runner unit tests | 8 | transcript role preservation, config defaults, mode mapping, max_questions |
| Runner integration tests (wiremock) | 6 | perfect answer, wrong answer, empty dataset, metadata, category, max_questions |

---

## Prerequisites

Running the live benchmark requires all three of the following:

### 1. Running aletheia instance

The benchmark runner connects to a live aletheia HTTP API. The service must
be running and accessible:

```bash
# Start the service
sudo systemctl start aletheia.service

# Verify it is healthy
curl -s http://localhost:8080/api/health | jq .status
# Expected: "healthy"
```

The service is stopped during teardown. Check instance configuration at
`~/aletheia/instance/config/aletheia.toml` for the port before restarting it.

### 2. Configured nous agent

The runner needs a `nous_id` to create sessions against. The default config
uses `nous_id = "benchmark"`. Verify a benchmark-suitable agent exists:

```bash
curl -s -H "Authorization: Bearer $ALETHEIA_TOKEN" \
  http://localhost:8080/api/v1/nous | jq '.[].id'
```

If no `benchmark` agent exists, either:
- Use the ID of an existing agent via `--nous-id <id>`
- Create a benchmark agent: `aletheia add-nous benchmark --provider claude --model claude-opus-4-5`

### 3. Downloaded datasets

Datasets are not committed to the repo (see `.gitignore`). Download both
to `benchmark-data/` at the repo root:

**LongMemEval**

```bash
# Paper: arxiv:2410.10813
# Repo: https://github.com/xiaowu0162/LongMemEval
git clone https://github.com/xiaowu0162/LongMemEval /tmp/longmemeval

# The repo contains several splits. Use LongMemEval-M (single-session, 500 Qs):
cp /tmp/longmemeval/data/longmemeval_m.json \
   ./benchmark-data/longmemeval.json

# Or the harder multi-session split (500 Qs, ~115k token histories):
cp /tmp/longmemeval/data/longmemeval_s.json \
   ./benchmark-data/longmemeval_s.json
```

Expected format: top-level JSON array of items with `question_id`,
`question_type`, `question`, `answer`, optional `answer_alternatives`, and
`haystack_sessions` (list of sessions, each a list of `{role, content}`
turns).

**LoCoMo**

```bash
# Paper: arxiv:2402.17753
# Repo: https://github.com/snap-research/locomo
git clone https://github.com/snap-research/locomo /tmp/locomo

# Dataset file location (may vary by release):
cp /tmp/locomo/data/locomo10.json \
   ./benchmark-data/locomo.json
```

Expected format: top-level JSON array of conversations with `sample_id`,
`conversation` (object keyed `session_N` → list of `{speaker, text}`
turns), and `qa` (list of `{question, answer, category, answer_alternatives}`).

---

## Execution

Once prerequisites are met, run the benchmarks via the CLI:

```bash
# LongMemEval — full run (~500 questions, expect several hours)
cargo run -p aletheia --bin aletheia -- benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --url http://localhost:8080 \
    --nous-id benchmark \
    --output results/longmemeval-$(date +%Y%m%d).json

# LongMemEval — smoke test (5 questions, ~5 minutes)
cargo run -p aletheia --bin aletheia -- benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --url http://localhost:8080 \
    --nous-id benchmark \
    --max-questions 5

# LoCoMo — full run (~10,000 QA pairs across 50 conversations)
cargo run -p aletheia --bin aletheia -- benchmark locomo \
    --dataset benchmark-data/locomo.json \
    --url http://localhost:8080 \
    --nous-id benchmark \
    --output results/locomo-$(date +%Y%m%d).json

# With retrieval metrics (Recall@k / NDCG@k)
cargo run -p aletheia --bin aletheia -- benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --url http://localhost:8080 \
    --nous-id benchmark \
    --retrieval-k 5

# With LLM-as-judge evaluation
cargo run -p aletheia --bin aletheia -- benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --url http://localhost:8080 \
    --nous-id benchmark \
    --judge-endpoint https://api.openai.com/v1/chat/completions \
    --judge-model gpt-4o \
    --judge-api-key $OPENAI_API_KEY

# Strict publication gate: fails unless statistics and provenance are complete
cargo run -p aletheia --bin aletheia -- benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --url http://localhost:8080 \
    --nous-id benchmark \
    --publishable \
    --gate-baseline docs/benchmarks/baselines/longmemeval-gate.json \
    --output results/longmemeval-publishable.json

# Compare a candidate run against a prior full BenchmarkReport JSON
cargo run -p aletheia --bin aletheia -- benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --url http://localhost:8080 \
    --nous-id benchmark \
    --baseline-report results/longmemeval-baseline.json \
    --publishable \
    --gate-baseline docs/benchmarks/baselines/longmemeval-gate.json \
    --output results/longmemeval-candidate.json

# CI/release smoke gate: no live instance or model call
cargo run -p aletheia --bin aletheia -- benchmark gate \
    --candidate-report crates/aletheia/testdata/benchmarks/smoke-report.json \
    --baseline crates/aletheia/testdata/benchmarks/smoke-gate-baseline.json
```

Or use the reproducibility script:

```bash
scripts/benchmark.sh --instance http://localhost:8080 --nous-id benchmark
scripts/benchmark.sh --instance http://localhost:8080 --nous-id benchmark \
    --publishable \
    --longmemeval-gate-baseline docs/benchmarks/baselines/longmemeval-gate.json \
    --locomo-gate-baseline docs/benchmarks/baselines/locomo-gate.json
```

### Tuning the runner

Configure `BenchmarkRunnerConfig` for production runs:

| Field | Default | Recommendation |
|---|---|---|
| `nous_id` | `"benchmark"` | Use a dedicated agent to avoid polluting production memory |
| `session_key_prefix` | `"bench"` | Include date: `"bench-20260412"` |
| `question_timeout` | 120s | Increase to 300s for long haystack ingestion |
| `max_questions` | all | Use `Some(50)` for a fast representive sample |
| `close_between_questions` | true | `true` = `OfficialParity` (fresh session per question); `false` = `ContinuousMemory` (shared session). See [Execution modes](#execution-modes). |
| `judge` | `None` | Set to an `LlmJudgeConfig` for LLM-as-judge scoring |
| `retrieval_k` | `None` | Set to `Some(k)` to compute Recall@k / NDCG@k from the knowledge store |

### Publishable reports

Use `--publishable` for results intended for publication or long-term
archival. This mode fails during CLI validation unless
`--gate-baseline <reviewed-baseline.json>` is also supplied, then fails closed
unless the full report includes:

- At least two scored questions, so bootstrap confidence intervals are valid
- 95% bootstrap CIs for exact match and F1
- Run provenance with redacted CLI args, config hash, target identity, and
  dataset hash
- Benchmark metadata with dataset hash and validation diagnostics
- Complete baseline/candidate comparison statistics when `--baseline-report`
  is supplied
- A passing benchmark regression gate for the reviewed dataset/model baseline

Insufficient samples are represented explicitly in JSON as
`publishability.publishable = false` with reasons, instead of silently omitting
statistics. Human output prints the same CI and publishability fields. The
`render_eval_report` / `eval-report` Typst PDF path accepts the full
`BenchmarkReport` JSON and renders the statistical summary, publishability
status, and comparison statistics.

Use `--baseline-report <path>` with a prior full `BenchmarkReport` JSON to add
candidate-vs-baseline comparison blocks. Comparisons are matched by scored
question id and report F1 plus exact-match comparisons with sample sizes, 95%
bootstrap CIs, Cohen's d, raw p-values, and Benjamini-Hochberg FDR-adjusted
p-values. The older `--baseline-in` / `--baseline-out` compact summaries remain
for reward-surface loading and do not contain enough per-question data for
statistical significance tests.

### Regression gates

`aletheia benchmark gate` validates a saved `BenchmarkReport` without talking
to a live instance. The gate compares candidate EM, mean F1, error rate,
timeout rate, no-answer rate, retrieval metrics, and LLM-as-judge metrics
against a reviewed baseline artifact. The artifact records benchmark name,
dataset hash and version, model, source report, reviewer, review timestamp,
baseline metrics, allowed regression deltas, minimum quality floors, and
maximum failure-rate ceilings.

The PR and release workflows run the deterministic smoke gate in
`crates/aletheia/testdata/benchmarks/`. Full LongMemEval and LoCoMo gates
should be run manually or on a schedule after producing fresh live reports:

```bash
cargo run -p aletheia --bin aletheia -- benchmark gate \
    --candidate-report docs/benchmarks/reports/longmemeval-20260601.json \
    --baseline docs/benchmarks/baselines/longmemeval-gate.json
```

A baseline refresh is a reviewed artifact update. Do not relax thresholds by
passing ad hoc flags; update the JSON artifact with the new source report,
dataset hash/version, model, reviewer, and threshold rationale.

### Execution modes

The runner supports two explicitly separated modes, controlled by
`close_between_questions` on `BenchmarkRunnerConfig`:

| Mode | `close_between_questions` | Session behavior | Use case |
|---|---|---|---|
| **Official parity** | `true` (default) | Fresh session per question; session closed after each question | Published benchmark numbers; each question evaluated in isolation |
| **Continuous memory** | `false` | One session shared across all questions; session closed at the end of the run | Simulates a real user conversation where earlier Q&A pairs remain in context |

**Isolation note:** A fresh session removes prior question/answer pairs from
the live prompt, but the underlying knowledge store is scoped to the
`nous_id`. For true disposable-memory isolation in official-parity mode,
use a dedicated, disposable `nous_id` for each benchmark run (for example,
`benchmark-{date}`) and discard the agent after the run. The runner tags
every generated artifact with the run's `eval_run_id` and each question's
`id` so results can be traced back to a clean namespace.

### Capturing results

The runner logs structured output via `tracing`. Capture it:

```bash
RUST_LOG=info cargo run -p dokimion ... 2>&1 | tee results/run.log
```

Key log lines to watch:
- `"starting benchmark run"` - includes `total` and `limit` counts
- `"benchmark question failed"` - per-question errors with cause
- `"benchmark run complete"` - final `em_rate` and `mean_f1`

---

## Metrics to capture

Record the following for each run and add to the Results table:

| Metric | Description |
|---|---|
| `exact_match_rate` | Fraction of questions where normalized answer = normalized expected |
| `mean_f1` | Average token-level F1 across all questions |
| Per-category EM | EM rate broken down by `question_type` / `category` |
| Per-category F1 | F1 broken down by `question_type` / `category` |
| EM 95% CI | Bootstrap confidence interval from `BenchmarkReport.statistics` |
| F1 95% CI | Bootstrap confidence interval from `BenchmarkReport.statistics` |
| Publishability | `publishability.publishable` plus reasons when false |
| Baseline/candidate p-values | Raw and FDR-adjusted p-values from `comparisons[]` when `--baseline-report` is used |
| Runtime | Wall time for the full run |
| Timeout rate | Fraction of questions that hit `question_timeout` |
| Error rate | Fraction of questions logged as `"scored as no-answer"` |
| Aletheia version | `git rev-parse HEAD` at time of run |
| Nous model | Model ID from `GET /api/v1/nous/{id}` |
| `eval_run_id` | Run identifier from `BenchmarkReport.provenance.eval_run_id` |
| Mode | `OfficialParity` or `ContinuousMemory` used for the run |

---

## Published SOTA baselines

Use these to contextualize aletheia's scores. All numbers are from the
original papers at the configurations described.

### LongMemEval baselines

Paper: *LongMemEval: Benchmarking Chat Assistants on Long-Term Interactive
Memory*, Zhang et al., 2024 (arxiv:2410.10813).

Dataset: LongMemEval-S (single + multi-session, 500 questions, five
question types). Metric: Exact Match (%).

| System | EM (%) | Notes |
|---|---|---|
| **Hindsight** | **91.4** | Upper bound: model sees the full conversation at query time |
| GPT-4o + memory system | 71.3 | Best production-grade result in paper |
| GPT-4o no memory | 48.2 | Baseline without any memory augmentation |
| Claude 3.5 Sonnet + memory | 67.1 | Anthropic model, memory-augmented |
| Llama-3-70B + memory | 58.4 | Open-weight, memory-augmented |

Per-category breakdown (Hindsight / GPT-4o + memory):

| Category | Hindsight EM | GPT-4o + mem EM |
|---|---|---|
| single-session-user | 95.2 | 78.4 |
| single-session-assistant | 92.1 | 74.6 |
| multi-session | 89.7 | 68.3 |
| temporal-reasoning | 87.4 | 62.1 |
| knowledge-update | 92.6 | 72.5 |

### LoCoMo baselines

Paper: *Long-Context Conversational Memory (LoCoMo)*, Maharana et al.,
2024 (arxiv:2402.17753).

Dataset: 50 dyadic conversations, ~27 sessions each, ~200 QA per
conversation, ~10,000 QA total. Metric: F1 score (%).

| System | F1 (%) | Notes |
|---|---|---|
| **Hindsight** | **89.61** | Upper bound: full context at query time |
| GPT-4 + summarization memory | 62.4 | Sliding-window summarization |
| GPT-4 no memory | 38.7 | Raw context (truncated at limit) |
| Llama-2-70B + memory | 41.2 | Open-weight |

Per-category breakdown (Hindsight / GPT-4 + mem):

| Category | Hindsight F1 | GPT-4 + mem F1 |
|---|---|---|
| single_hop | 93.2 | 68.1 |
| multi_hop | 87.4 | 55.3 |
| temporal | 84.9 | 51.7 |
| open_domain | 91.1 | 64.2 |
| adversarial | 71.3 | 49.8 |

---

## Results

*Not yet collected - see [Prerequisites](#prerequisites).*

When results are available, record them here:

### LongMemEval

| Date | Aletheia version | Model | EM (%) | Mean F1 | Notes |
|---|---|---|---|---|---|
| - | - | - | - | - | Awaiting run |

### LoCoMo

| Date | Aletheia version | Model | F1 (%) | Notes |
|---|---|---|---|---|
| - | - | - | - | Awaiting run |

---

## Analysis guidance

When results land, compare against the [Published SOTA baselines](#published-sota-baselines) table and analyze:

**Strengths to look for:**
- `single-session-user` and `single-session-assistant` EM above 70%: basic
  factual recall is working
- `temporal-reasoning` above 60%: Bayesian surprise and staleness features
  (#2848, #2852) are contributing
- F1 above EM: the model is producing correct-content answers that
  fail normalized exact match - consider relaxing the scorer or accepting
  contains as partial credit

**Weaknesses to watch for:**
- High error/timeout rate: ingestion pipeline is too slow; consider
  increasing `question_timeout` or reducing haystack replay to key turns only
- `knowledge-update` EM below 50%: the staleness detector (#2848) may be
  over-retaining old facts instead of evicting them
- `multi_hop` F1 collapse (LoCoMo): evidence gap feature (#2851) not
  bridging cross-session dependencies

**Regression gate:** CI covers the deterministic smoke artifact with
`aletheia benchmark gate`. Full live memory gates should compare archived
LongMemEval and LoCoMo reports against reviewed baselines before publishing
or using the numbers for release decisions.

---

## Issue status

**#2854 is not yet closeable.** The runner is complete and well-tested, but
the live benchmark has not been executed. The blocking requirements are:

1. `aletheia.service` running and healthy
2. `benchmark` nous agent configured
3. Dataset files present at `benchmark-data/`

Once results are collected and the Results table above is populated with
quantitative scores, #2854 can be closed. A follow-up issue should track
the CLI wrapper (`dokimion benchmark longmemeval`) from the #3091 TODO list.
