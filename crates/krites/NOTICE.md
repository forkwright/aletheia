# Third-party notice — krites

`krites` is substantially derived from **CozoDB** (`cozo-core`), copyright the CozoDB authors,
licensed under the **Mozilla Public License 2.0**. A copy of that license sits beside this file at
[LICENSE-MPL-2.0](LICENSE-MPL-2.0); upstream is <https://github.com/cozodb/cozo>.

## What is derived

The derivation covers the Datalog engine and the data layer rather than a few borrowed helpers, so
the honest summary is that most of this crate descends from `cozo-core`:

| Evidence | Measure |
|---|---|
| `cozo-core/src` files present here at the same relative path | 78 of 104 |
| `src/datalog.pest` against upstream `cozoscript.pest` | 251 of 252 non-blank lines identical |
| Upstream lines preserved verbatim across a 14-file sample of `data/` | 67.7% |

Aletheia's own additions are real and sit alongside it — `async_surface`, `counterfactual`,
`hot_reload`, `query_cache`, the HNSW and FTS work, and the split of `data/aggr` and `data/expr`
into modules. They do not change the provenance of the code they extend.

## What that requires

Under MPL §3.1 every file in this crate that is derived from `cozo-core`, **including our
modifications to it**, stays governed by the MPL. That is file-level copyleft: it binds these files
and reaches no further into aletheia.

Aletheia distributes the whole as a Larger Work under AGPL-3.0-or-later. MPL §3.3 permits exactly
that, because CozoDB does not attach Exhibit B and so is not Incompatible With Secondary Licenses,
and AGPL-3.0 is a Secondary License under §1.12. A recipient may therefore take the covered files
under either license, at their option. The crate's `license` field records the combination.

## Why this notice exists

Upstream identifiers were renamed during the migration and no attribution was recorded, which left
the crate carrying MPL-covered code with its notices removed — the one thing §3.1 does not permit,
independent of which license the Larger Work ships under. Renaming symbols does not change
authorship of the expression. This file restores the notice.

The related trap, since it is what produced the gap: `docs/HUBS.md` asks memory documentation to
describe the current architecture as Krites/Datalog/Fjall rather than CozoDB. That is sound naming
hygiene and it explicitly does not reach attribution. Provenance and licensing statements name
CozoDB because they are claims about authorship, not about architecture.
