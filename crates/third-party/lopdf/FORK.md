# Aletheia `lopdf` security fork

Source: `J-F-Liu/lopdf` commit `1e3d646ca249ebf1a6ff479278c07e9c0f9377a8`
(2026-08-24), carrying the unreleased post-0.44 content-parser fix for upstream
issue #535. The upstream package license is MIT; its unmodified `LICENSE` is
retained beside this record.

This fork is intentionally narrow. Relative to that source it adds:

- `DecompressionBudget`, an explicit shared conservative reservation budget;
- `LoadOptions::decompression_budget`, charged by eager xref/object-stream
  decode before allocation; and
- `Document::extract_text_chunks_with_limit_and_budget`, which charges the
  same budget for each page content and `/ToUnicode` decoder.

The upstream development-only assets, examples, integration tests, CI files,
and auxiliary `pdfutil` package are deliberately absent. They are excluded by
the upstream crate manifest and are not needed to build this runtime patch;
Aletheia's deterministic PDF tests cover the maintained behavior instead.

`poiesis-inspect` creates one budget per hostile PDF and passes it through both
loading and extraction. Reservations charge the bounded decoder maximum, not
post-hoc observed output: this is deliberately conservative, but makes the
aggregate refusal invariant safe under lopdf's parallel object loading.

Remove this patch when a released upstream `lopdf` exposes equivalent shared
aggregate accounting, after re-running the PDF adversarial tests.
