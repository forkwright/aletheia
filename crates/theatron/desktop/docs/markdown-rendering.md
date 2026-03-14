# Markdown Rendering for Dioxus Desktop

Research and design document for the Aletheia desktop markdown rendering pipeline.

Dioxus desktop uses wry/tao under the hood, embedding a platform webview. This means standard HTML elements with CSS classes render directly -- no custom paint or layout engine needed. The rendering strategy is therefore: parse markdown into an intermediate AST, then emit either Dioxus RSX components (production) or plain HTML (prototype/validation).

## 1. pulldown-cmark to Dioxus RSX Pipeline

The pipeline has three stages:

```
pulldown-cmark event stream  -->  MdNode AST  -->  Dioxus RSX / HTML
```

**Stage 1: pulldown-cmark event stream.** pulldown-cmark emits a flat stream of `Event` values (`Start(Tag)`, `End(TagEnd)`, `Text`, `Code`, `SoftBreak`, `HardBreak`, `Rule`). Events are processed with `ENABLE_STRIKETHROUGH | ENABLE_TABLES` options enabled.

**Stage 2: MdNode intermediate AST.** A stack-based parser converts the flat event stream into a tree of owned `MdNode` values. The parser maintains a `Vec<(BuilderKind, Vec<MdNode>)>` stack. On `Start`, a new frame is pushed; on `End`, the frame is popped and converted into an `MdNode` via `build_node()`, then appended to the parent frame's children. Text events inside `Image` frames accumulate into the alt-text field rather than creating child nodes.

`MdNode` enum variants:

| Category | Variants |
|----------|----------|
| Block-level | `Heading { level, children }`, `Paragraph(children)`, `CodeBlock { lang, code }`, `BlockQuote(children)`, `List { ordered, start, items }`, `Table { headers, rows }`, `ThematicBreak` |
| Inline | `Text(String)`, `Code(String)`, `Strong(children)`, `Emphasis(children)`, `Strikethrough(children)`, `Link { url, children }`, `Image { url, alt }`, `SoftBreak`, `HardBreak` |

Design constraints on `MdNode`:
- Owned data (no lifetime parameters) so nodes can be stored in Dioxus signals.
- `Clone + PartialEq` derived so Dioxus can skip re-renders when props are unchanged.
- Children are `Vec<MdNode>` -- small allocations, simple traversal.

**Stage 3: Dioxus RSX / HTML output.** For the prototype, `render_to_html()` validates tree completeness by emitting HTML with the CSS class conventions described below. In production, each `MdNode` variant maps to a Dioxus RSX component (see Component Architecture).

Reference implementation: `prototype.rs` in this directory.

## 2. Syntax Highlighting: syntect vs tree-sitter

**Decision: syntect for v1.**

Rationale:

| Factor | syntect | tree-sitter |
|--------|---------|-------------|
| Existing usage | `crates/theatron/tui/src/highlight.rs` already wraps syntect with `SyntaxSet::load_defaults_newlines()` and `ThemeSet::load_defaults()` | Not used in the project |
| Granularity needed | Code-block-level highlighting (complete blocks, not live editing) | Designed for incremental, keystroke-level parsing |
| Build dependencies | Pure Rust, no C/C++ toolchain required | Requires C compiler for grammar builds |
| Theme portability | Same `ThemeSet` (e.g. `base16-ocean.dark`) works in both TUI (ratatui spans) and desktop (CSS `<span>` with inline color) | Separate theme system |
| Performance | Sufficient for render-on-complete code blocks | Faster for incremental edits within a single file |

The TUI `Highlighter` struct loads `SyntaxSet` and `ThemeSet` once (they are expensive to construct) and resolves language by token then by extension, falling back to plain text. The desktop highlighter follows the same pattern but emits `<span style="color:...">` instead of ratatui `Span` values.

**Future consideration:** If the desktop UI adds a code editor with real-time syntax feedback, tree-sitter's incremental parsing becomes worthwhile. This would be a separate component, not a replacement of the markdown code-block highlighter.

## 3. Incremental Rendering During Streaming

LLM responses arrive as a stream of small text deltas. Re-parsing the entire accumulated text on every delta is `O(total_size)` and becomes expensive for long responses. The `IncrementalMarkdown` struct solves this with a frozen/active-tail split.

### Algorithm

```
IncrementalMarkdown {
    full_text: String,
    frozen_boundary: usize,      // byte offset into full_text
    frozen_nodes: Vec<MdNode>,    // complete, stable blocks
    active_tail_nodes: Vec<MdNode>, // re-parsed on each delta
}
```

On each `push_delta(delta)`:

1. Append `delta` to `full_text`.
2. **Freeze detection:** Scan the unfrozen region for the last `"\n\n"` block boundary (skipping trailing boundaries to avoid freezing incomplete blocks). Parse the newly-frozen text into `MdNode` values and append to `frozen_nodes`.
3. **Tail reparse:** Parse only the text after `frozen_boundary` into `active_tail_nodes`.

This gives `O(tail_size)` work per delta. Frozen nodes are stable and can be memoized by Dioxus (props unchanged = skip re-render).

### Existing TUI approach

The TUI streaming handler (`crates/theatron/tui/src/update/streaming.rs`) uses a simpler heuristic: re-render cached markdown when the accumulated delta reaches 64 bytes or the delta contains a newline character. This works for the TUI where rendering is cheap (terminal cells), but the desktop needs the frozen/tail split because HTML DOM updates are more expensive and memoization requires stable node identity.

### Finalization

`finalize()` parses any remaining tail text, appends to `frozen_nodes`, and clears the active tail. Called when the stream completes.

## 4. Thinking Panel, Tool Call, Table, and Image Rendering

### Thinking panels

Thinking content arrives either as `<think>` tags within markdown text or as separate API-level thinking blocks (via `handle_stream_thinking_delta`). Detection strategy:

- **API-level:** Thinking deltas are accumulated in a separate buffer (`streaming_thinking`), not mixed into the markdown text. Rendered as a distinct collapsible panel above the main response.
- **Inline `<think>` tags:** Detected during post-parse processing. Content between `<think>` and `</think>` is extracted and rendered identically to API-level thinking.

Rendering: collapsible `<blockquote>` with CSS class `md-thinking-panel`. Distinguished from regular blockquotes by a left-border accent color and italic header ("Thinking..."). Collapsed by default once the response completes; expanded during streaming.

```html
<blockquote class="md-thinking-panel">
  <details open>
    <summary>Thinking...</summary>
    <p>reasoning content here</p>
  </details>
</blockquote>
```

### Tool calls

Tool calls are not markdown -- they arrive as structured stream events (`tool_start`, `tool_result`). Rendered as status cards adjacent to the markdown content:

```html
<div class="md-tool-card">
  <span class="md-tool-name">read_file</span>
  <span class="md-tool-status md-tool-status--complete">done (150ms)</span>
  <div class="md-tool-result">...</div>
</div>
```

States: `running` (spinner), `complete` (checkmark + duration), `error` (error icon + message). The TUI already tracks these states via `ToolCallInfo { name, duration_ms, is_error }`.

### Tables

Parsed via `MdNode::Table { headers, rows }` where `headers: Vec<Vec<MdNode>>` (cells of the header row) and `rows: Vec<Vec<Vec<MdNode>>>` (rows of cells). Rendered as full HTML table structure:

```html
<table class="md-table">
  <thead><tr><th>...</th></tr></thead>
  <tbody><tr><td>...</td></tr></tbody>
</table>
```

Cell contents are recursively rendered inline nodes, supporting bold/italic/code/links within cells.

### Images

Parsed via `MdNode::Image { url, alt }`. Rendered as:

```html
<img src="..." alt="..." loading="lazy" class="md-image" />
```

`loading="lazy"` is native browser lazy loading, supported by the webview. Image proxy and caching are open questions (see below).

## 5. Working Markdown Component Prototype

The prototype lives at `prototype.rs` in this directory and is included as a `#[cfg(test)]` module in `theatron-core`:

```rust
// crates/theatron/core/src/lib.rs
#[cfg(test)]
mod md_prototype;  // includes prototype.rs
```

Tests run via `cargo test -p theatron-core`.

### Test coverage (25 tests)

**Parser tests (14):** `parses_heading`, `parses_multiple_heading_levels`, `parses_paragraph_with_inline_formatting`, `parses_code_block_with_lang`, `parses_code_block_no_lang`, `parses_inline_code`, `parses_unordered_list`, `parses_ordered_list`, `parses_blockquote`, `parses_link`, `parses_image`, `parses_strikethrough`, `parses_thematic_break`, `parses_table`.

**HTML renderer tests (4):** `renders_heading_html`, `renders_code_block_html`, `renders_table_html`, `html_escapes_text`.

**Incremental streaming tests (5):** `incremental_basic_streaming`, `incremental_preserves_content`, `incremental_code_block_stays_in_tail`, `incremental_empty_deltas_are_safe`, `incremental_finalize_collects_all`.

**Real LLM output tests (2):** `parses_real_llm_output` (full parse + HTML render of a realistic multi-block assistant response), `incremental_with_real_llm_streaming` (simulated chunked delivery converges to same result as direct parse).

## Component Architecture

```
MdView (top-level)
  |-- iterates over Vec<MdNode>
  |-- dispatches to MdBlock
  |
  MdBlock (block-level dispatch)
    |-- MdHeading       (h1..h6)
    |-- MdParagraph      (inline children)
    |-- MdCodeBlock      (pre/code + syntect highlighting)
    |-- MdBlockQuote     (recursive MdBlock children)
    |-- MdList           (ol/ul with MdBlock items)
    |-- MdTable          (thead/tbody with MdInline cells)
    |-- MdThematicBreak  (hr)
    |
    MdInline (inline dispatch, used by paragraph/heading/list-item/cell)
      |-- MdText, MdCode, MdStrong, MdEmphasis
      |-- MdStrikethrough, MdLink, MdImage
      |-- MdSoftBreak, MdHardBreak
```

Each component receives its `MdNode` data as props. Because `MdNode` derives `PartialEq`, Dioxus memoization (`#[component]` with `PartialEq` props) skips re-rendering unchanged subtrees during streaming. Frozen nodes from `IncrementalMarkdown` are stable across deltas, so only the active tail re-renders.

`MdView` also renders non-markdown elements (thinking panels, tool cards) interleaved with the markdown blocks based on stream event ordering.

## CSS Theme Structure

Since Dioxus desktop renders in a webview, all styling uses CSS classes. The class naming convention:

```css
/* Headings */
.md-h1, .md-h2, .md-h3, .md-h4, .md-h5, .md-h6

/* Block elements */
.md-blockquote
.md-table
.md-table th, .md-table td
.md-thinking-panel

/* Inline elements */
.md-inline-code

/* Code blocks */
pre > code.language-rust
pre > code.language-python
pre > code.language-javascript
/* ... language-* for any fenced language tag */

/* Tool cards */
.md-tool-card
.md-tool-name
.md-tool-status
.md-tool-status--running
.md-tool-status--complete
.md-tool-status--error
.md-tool-result

/* Images */
.md-image
```

Theme values (colors, fonts, spacing) are set via CSS custom properties on the root element, allowing runtime theme switching without re-parsing. The TUI theme system (`crates/theatron/tui/src/theme.rs`) defines the color palette; the desktop maps these to CSS custom properties.

## Open Questions

- **Code block max height.** Long code blocks may need a max-height with scroll, or a "show more" toggle. The webview handles overflow natively, but UX for 500+ line blocks needs design work.
- **Image proxy/caching.** LLM responses may reference external image URLs. Options: (a) load directly in webview (security risk, privacy leak), (b) proxy through the Aletheia server with caching, (c) download and cache locally at render time. Leaning toward (b) or (c) for privacy.
- **Math/LaTeX support.** Common in technical LLM output. Options: KaTeX (fast, CSS-only rendering) or MathJax (more complete, heavier). Neither is integrated yet. Would require detecting `$...$` and `$$...$$` delimiters during parsing, either as a pulldown-cmark extension or as a post-parse pass.
- **Custom emoji rendering.** Slack-style `:emoji_name:` shortcodes appear in some LLM output. Low priority -- standard Unicode emoji render natively in the webview.
- **Accessibility.** Semantic HTML (headings, lists, tables) provides baseline a11y. Code blocks need `aria-label` with the language name. Thinking panels need appropriate ARIA roles for the collapsible disclosure pattern.
