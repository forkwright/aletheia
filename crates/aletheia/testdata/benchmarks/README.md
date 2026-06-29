Benchmark regression smoke artifacts.

`smoke-longmemeval.json` is a tiny LongMemEval-shaped deterministic dataset for
release-critical smoke provenance. `smoke-report.json` is the saved
`BenchmarkReport` used to exercise the regression gate in CI.
`smoke-gate-baseline.json` is the reviewed gate artifact for that report shape.
Full LongMemEval and LoCoMo datasets are not committed; live full-run reports
remain under `docs/benchmarks/reports/`.
