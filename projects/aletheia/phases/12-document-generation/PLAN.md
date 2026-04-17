# Phase 12: Document generation

## Goal
Operators can generate structured documents (spreadsheets, presentations, text) from agent outputs and knowledge graphs.

## Success criteria
- Poiesis crate produces valid ODS, ODP, and ODT files
- Document templates can reference knowledge graph entities
- Generation latency under 2s for 10-page documents
- Generated documents open correctly in LibreOffice and Microsoft Office

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Poiesis crate produces valid ODS, ODP, and ODT files | Validator (ooxml/odf validator) reports structural errors |
| Document templates can reference knowledge graph entities | Template with entity reference produces blank or incorrect value |
| Generation latency under 2s for 10-page documents | Benchmark shows mean generation time >= 2s |
| Generated documents open correctly in LibreOffice and Microsoft Office | Manual open test shows corruption or missing content |

## Scope

### In scope
- poiesis crate: core, text, sheet, slides subcrates
- Template engine with knowledge graph variable substitution
- ODF and OOXML output formats

### Out of scope
- PDF generation (deferred)
- Real-time collaborative editing

## Requirements
- REQ-01: Templates are defined in TOML with markdown content blocks
- REQ-02: Entity references use `{{entity.name}}` syntax
- REQ-03: Spreadsheets support formulas, charts, and styling
- REQ-04: Presentations support slides, layouts, and images

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Format | ODF over proprietary | Open standard, easier to debug |
| XML builder | custom over zip+template | Full control over output structure |

## Open questions
- Should we support LaTeX export for academic use cases? (Deferred)

## Dependencies
- Phase 11 complete
- Knowledge graph populated with entities
