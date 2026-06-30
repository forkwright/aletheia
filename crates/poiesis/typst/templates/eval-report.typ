// poiesis-typst eval-report template.
//
// Renders eval framework benchmark results to a structured PDF report.
// Expected JSON schema:
// {
//   "summary": { "passed": n, "failed": n, "skipped": n, "total_duration_ms": n },
//   "benchmarks": [
//     { "id": "test-id", "category": "category", "outcome": "passed|failed|skipped",
//       "duration_ms": n_or_null, "error": "msg_or_null", "skip_reason": "msg_or_null" }
//   ]
// }
// Also accepts full memory BenchmarkReport JSON with benchmark, scored,
// statistics, publishability, and comparisons fields.

#let data = json("data.json")

#set page(paper: "us-letter", margin: 0.75in)
#set text(font: "Liberation Sans", size: 10pt)
#set par(leading: 0.65em, spacing: 0.8em)

// Title block
#align(left)[
  #text(18pt, weight: "bold")[Evaluation Report]
]

#v(16pt)

// Memory benchmark report
#if "benchmark" in data [
  #text(12pt, weight: "bold")[Memory Benchmark]
  #v(4pt)
  #text(10pt)[
    Benchmark: #data.benchmark \
    Total: #data.total | Scored: #data.scored | Errors: #data.errors | Timeouts: #data.timeouts | No answer: #data.no_answers
  ]
  #v(8pt)

  #if "statistics" in data [
    #let st = data.statistics
    #text(11pt, weight: "bold")[Statistical Summary]
    #v(3pt)
    #text(10pt)[
      EM 95% CI: #str(st.em_ci_low) – #str(st.em_ci_high) \
      F1 95% CI: #str(st.f1_ci_low) – #str(st.f1_ci_high) \
      Resamples: #st.n_resamples \
      Method: #st.method
    ]
    #v(8pt)
  ] else [
    #text(10pt, fill: rgb("#aa5500"))[Statistical Summary: unavailable]
    #v(8pt)
  ]

  #if "publishability" in data [
    #let p = data.publishability
    #let label = if p.publishable { "publishable" } else { "not publishable" }
    #text(11pt, weight: "bold")[Publishability]
    #v(3pt)
    #text(10pt)[Status: #label]
    #if p.publishable == false [
      #v(3pt)
      #for reason in p.reasons [
        - #reason
      ]
    ]
    #v(8pt)
  ]

  #if "comparisons" in data and data.comparisons.len() > 0 [
    #text(11pt, weight: "bold")[Baseline/Candidate Comparisons]
    #v(3pt)
    #for c in data.comparisons [
      #if "statistics" in c [
        #let s = c.statistics
        #text(10pt)[
          #c.metric: baseline #str(s.mean_a), candidate #str(s.mean_b), d #str(s.effect.d),
          p(raw) #str(s.p_raw), p(FDR) #str(s.p_adjusted)
        ]
      ] else [
        #text(10pt)[#c.metric: #c.status (#c.reason)]
      ]
      #v(3pt)
    ]
    #v(8pt)
  ]
]

// Summary section
#if "summary" in data [
  #let s = data.summary
  #text(12pt, weight: "bold")[Summary]
  #v(4pt)
  #text(10pt)[
    Passed: #s.passed | Failed: #s.failed | Skipped: #s.skipped \
    Total Duration: #(s.total_duration_ms)ms
  ]
  #v(12pt)
]

// Benchmark results table
#if "benchmarks" in data [
  #let benchmarks = data.benchmarks
  #text(12pt, weight: "bold")[Results]
  #v(4pt)

  #table(
    columns: (2fr, 1fr, 1fr),
    fill: (_, y) => if y == 0 { rgb("#333333") } else { none },
    stroke: 0.5pt + rgb("#cccccc"),
    inset: 6pt,
    text(weight: "bold", fill: white, "Test ID"),
    text(weight: "bold", fill: white, "Outcome"),
    text(weight: "bold", fill: white, "Duration"),
    ..benchmarks.map(b => {
      let color = if b.outcome == "passed" {
        rgb("#00aa00")
      } else if b.outcome == "failed" {
        rgb("#ff0000")
      } else {
        rgb("#ffaa00")
      }
      let duration_text = if b.duration_ms != none {
        str(b.duration_ms) + "ms"
      } else {
        "—"
      }
      (b.id, text(fill: color, b.outcome), duration_text)
    }).flatten(),
  )
]
