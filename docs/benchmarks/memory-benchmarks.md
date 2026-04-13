# Memory Benchmark Results: LongMemEval and LoCoMo

**Status:** Harness implemented, awaiting live run. See [Prerequisites](#prerequisites) before executing.

**Issue:** [#2854](https://github.com/forkwright/aletheia/issues/2854)
**Harness PRs:** [#3090](https://github.com/forkwright/aletheia/pull/3090) (dataset parsers + scoring), [#3091](https://github.com/forkwright/aletheia/pull/3091) (live runner)

---

## What the harness provides

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

1. `POST /api/v1/sessions` — create a fresh session keyed to the question
2. Replay every **user turn** from the haystack sessions as messages
   (assistant turns are skipped to avoid contaminating the answer signal)
3. `POST /api/v1/sessions/{id}/messages` — ask the benchmark question
4. Collect the SSE stream; extract concatenated `text_delta` events
5. Score the answer with `score_answer(actual, expected_answers)`
6. Optionally `DELETE /api/v1/sessions/{id}` to reset memory between questions

Per-question errors are logged and scored as zero — a network hiccup does
not abort the entire run. The runner produces a `BenchmarkReport` with
`exact_match_rate()`, `mean_f1()`, and a `per_category()` breakdown.

### Test coverage (already passing)

The harness has 188 passing tests (`cargo test -p dokimion`):

| Scope | Count | Notes |
|---|---|---|
| Dataset parsers (LongMemEval + LoCoMo) | 14 | JSON format, alternates, multi-session, error cases |
| Scoring (EM, F1, contains) | 10 | exact, normalized, partial, substring, duplicates |
| Report aggregation | 3 | empty, EM+F1 math, per-category grouping |
| Runner unit tests | 7 | role filtering, config defaults, max_questions |
| Runner integration tests (wiremock) | 6 | perfect answer, wrong answer, empty dataset, metadata, category, max_questions |

---

## Prerequisites

Running the live benchmark requires all three of the following:

### 1. Running aletheia instance

The benchmark runner connects to a live aletheia HTTP API. The service must
be running and accessible:

```bash
# Start the service (passwordless sudo on menos)
sudo systemctl start aletheia.service

# Verify it is healthy
curl -s http://localhost:8080/api/health | jq .status
# Expected: "healthy"
```

The service was stopped as of 2026-04-12. Check instance configuration at
`~/aletheia/instance/config/aletheia.toml` for the port.

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
   /home/ck/menos-ops/repos/aletheia/benchmark-data/longmemeval.json

# Or the harder multi-session split (500 Qs, ~115k token histories):
cp /tmp/longmemeval/data/longmemeval_s.json \
   /home/ck/menos-ops/repos/aletheia/benchmark-data/longmemeval_s.json
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
   /home/ck/menos-ops/repos/aletheia/benchmark-data/locomo.json
```

Expected format: top-level JSON array of conversations with `sample_id`,
`conversation` (object keyed `session_N` → list of `{speaker, text}`
turns), and `qa` (list of `{question, answer, category, answer_alternatives}`).

---

## Execution

Once prerequisites are met, run the benchmarks via the eval CLI:

```bash
# LongMemEval — full run (~500 questions, expect several hours)
cargo run -p dokimion --bin dokimion -- benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --instance http://localhost:8080 \
    --nous-id benchmark \
    --output results/longmemeval-$(date +%Y%m%d).json

# LongMemEval — smoke test (5 questions, ~5 minutes)
cargo run -p dokimion --bin dokimion -- benchmark longmemeval \
    --dataset benchmark-data/longmemeval.json \
    --instance http://localhost:8080 \
    --nous-id benchmark \
    --max-questions 5

# LoCoMo — full run (~10,000 QA pairs across 50 conversations)
cargo run -p dokimion --bin dokimion -- benchmark locomo \
    --dataset benchmark-data/locomo.json \
    --instance http://localhost:8080 \
    --nous-id benchmark \
    --output results/locomo-$(date +%Y%m%d).json
```

Note: The CLI wrapper (`dokimion benchmark`) is listed as a follow-on in
PR #3091. Until it lands, use the runner directly from an integration test
or a small Rust harness. See `crates/eval/tests/benchmark_runner.rs` for
the exact API pattern.

### Tuning the runner

Configure `BenchmarkRunnerConfig` for production runs:

| Field | Default | Recommendation |
|---|---|---|
| `nous_id` | `"benchmark"` | Use a dedicated agent to avoid polluting production memory |
| `session_key_prefix` | `"bench"` | Include date: `"bench-20260412"` |
| `question_timeout` | 120s | Increase to 300s for long haystack ingestion |
| `max_questions` | all | Use `Some(50)` for a fast representive sample |
| `close_between_questions` | true | Keep true — resets memory between questions for clean isolation |

### Capturing results

The runner logs structured output via `tracing`. Capture it:

```bash
RUST_LOG=info cargo run -p dokimion ... 2>&1 | tee results/run.log
```

Key log lines to watch:
- `"starting benchmark run"` — includes `total` and `limit` counts
- `"benchmark question failed"` — per-question errors with cause
- `"benchmark run complete"` — final `em_rate` and `mean_f1`

---

## Metrics to capture

Record the following for each run and add to the Results table below:

| Metric | Description |
|---|---|
| `exact_match_rate` | Fraction of questions where normalized answer = normalized expected |
| `mean_f1` | Average token-level F1 across all questions |
| Per-category EM | EM rate broken down by `question_type` / `category` |
| Per-category F1 | F1 broken down by `question_type` / `category` |
| Runtime | Wall time for the full run |
| Timeout rate | Fraction of questions that hit `question_timeout` |
| Error rate | Fraction of questions logged as `"scored as no-answer"` |
| Aletheia version | `git rev-parse HEAD` at time of run |
| Nous model | Model ID from `GET /api/v1/nous/{id}` |

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

*Not yet collected — see Prerequisites above.*

When results are available, record them here:

### LongMemEval

| Date | Aletheia version | Model | EM (%) | Mean F1 | Notes |
|---|---|---|---|---|---|
| — | — | — | — | — | Awaiting run |

### LoCoMo

| Date | Aletheia version | Model | F1 (%) | Notes |
|---|---|---|---|---|
| — | — | — | — | Awaiting run |

---

## Analysis guidance

When results land, compare against the SOTA table above and analyze:

**Strengths to look for:**
- `single-session-user` and `single-session-assistant` EM above 70%: basic
  factual recall is working
- `temporal-reasoning` above 60%: Bayesian surprise and staleness features
  (#2848, #2852) are contributing
- F1 notably above EM: the model is producing correct-content answers that
  fail normalized exact match — consider relaxing the scorer or accepting
  contains as partial credit

**Weaknesses to watch for:**
- High error/timeout rate: ingestion pipeline is too slow; consider
  increasing `question_timeout` or reducing haystack replay to key turns only
- `knowledge-update` EM below 50%: the staleness detector (#2848) may be
  over-retaining old facts instead of evicting them
- `multi_hop` F1 collapse (LoCoMo): evidence gap feature (#2851) not
  bridging cross-session dependencies

**Regression gate:**
Once a baseline run is recorded, add a CI check that runs `--max-questions 20`
and asserts `em_rate >= baseline - 0.05`. The wiremock-based integration
tests already validate the scoring pipeline; this would validate the live
memory pipeline.

---

## Issue status

**#2854 is not yet closeable.** The harness is complete and well-tested, but
the live benchmark has not been executed. The blocking requirements are:

1. `aletheia.service` running and healthy
2. `benchmark` nous agent configured
3. Dataset files present at `benchmark-data/`

Once results are collected and the Results table above is populated with
quantitative scores, #2854 can be closed. A follow-up issue should track
the CLI wrapper (`dokimion benchmark longmemeval`) from the #3091 TODO list.
