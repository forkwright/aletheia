# Aletheia `lopdf` security fork

Source: `J-F-Liu/lopdf` commit `1e3d646ca249ebf1a6ff479278c07e9c0f9377a8`
(2026-08-24), carrying the unreleased post-0.44 content-parser fix for upstream
issue #535. The upstream package license is MIT; its unmodified `LICENSE` is
retained beside this record.

This fork is intentionally narrow. Relative to that source it adds:

- `DecompressionBudget`, an explicit shared conservative reservation budget;
- `LoadOptions::decompression_budget`, charged by eager xref/object-stream
  decode before allocation, once for every bounded filter layer;
- `LoadOptions::max_objects` and `reject_encrypted`. Classic tables enforce
  their declared row counts in the actual CR/LF/CRLF parser, merged xrefs keep
  one bounded unique-ID set, and object streams must match that final xref's
  container/index membership before allocating their object maps. `/Encrypt`
  is rejected before password authentication or decryption;
- `Document::extract_text_chunks_with_limit_and_budget`, which charges the
  same decompression budget for every page-content and `/ToUnicode` filter
  layer and one `ToUnicodeMappingBudget` across all retained font encodings;
- bounded `/ToUnicode` range expansion/target sequences and predictor
  dimensions, preventing attacker-controlled iteration and auxiliary vectors;
  and
- checked hostile-input arithmetic for xref streams, object streams, CMap
  targets, inline images, and recovered stream offsets.

The upstream development-only assets, examples, integration tests, CI files,
and auxiliary `pdfutil` package are deliberately absent. They are excluded by
the upstream crate manifest and are not needed to build this runtime patch;
Aletheia's deterministic PDF tests cover the maintained behavior instead.

`poiesis-inspect` creates one budget per hostile PDF and passes it through both
loading and extraction. Reservations charge the bounded decoder maximum for
each filter layer, not post-hoc observed output: this is deliberately
conservative, but makes the aggregate refusal invariant safe under lopdf's
parallel object loading. Its text policy additionally derives the remaining
page decode allowance from a finite ToUnicode expansion factor, so the
aggregate text cap is admitted before per-page strings are built.

The public `poiesis-inspect` PDF functions retain a panic boundary around this
fork. Deployed async callers execute inspection on blocking workers; a parser
panic is therefore a typed per-document refusal rather than a process or async
executor unwind. Workspace-member adversarial tests exercise these fork APIs,
because the intentionally patched (non-member) crate's own unit tests are not
part of the workspace gate.

Remove this patch when a released upstream `lopdf` exposes equivalent shared
aggregate accounting, after re-running the PDF adversarial tests.
