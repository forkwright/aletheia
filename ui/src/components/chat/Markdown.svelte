<script lang="ts">
  import { renderMarkdown } from "../../lib/markdown";

  let { content }: { content: string } = $props();

  let safeContent = $derived(typeof content === "string" ? content : String(content ?? ""));
  let html = $derived(renderMarkdown(safeContent));
  let container = $state<HTMLDivElement | null>(null);

  // Add copy buttons to code blocks after render
  $effect(() => {
    void html;
    if (!container) return;

    requestAnimationFrame(() => {
      const pres = container!.querySelectorAll("pre");
      for (const pre of pres) {
        // Skip if already has a copy button
        if (pre.querySelector(".copy-btn")) continue;

        const btn = document.createElement("button");
        btn.className = "copy-btn";
        btn.textContent = "Copy";
        btn.setAttribute("aria-label", "Copy code to clipboard");
        btn.addEventListener("click", () => {
          const code = pre.querySelector("code")?.textContent ?? pre.textContent ?? "";
          navigator.clipboard.writeText(code).then(() => {
            btn.textContent = "Copied!";
            btn.classList.add("copied");
            setTimeout(() => {
              btn.textContent = "Copy";
              btn.classList.remove("copied");
            }, 2000);
          });
        });
        pre.style.position = "relative";
        pre.appendChild(btn);
      }
    });
  });
</script>

<div class="markdown-body" bind:this={container}>
  {@html html}
</div>

<style>
  .markdown-body {
    font-size: var(--text-base);
    line-height: 1.6;
    word-wrap: break-word;
    overflow-wrap: break-word;
  }
  .markdown-body :global(p) {
    margin: 0 0 8px;
  }
  .markdown-body :global(p:last-child) {
    margin-bottom: 0;
  }
  .markdown-body :global(h1),
  .markdown-body :global(h2),
  .markdown-body :global(h3) {
    margin: 16px 0 8px;
    font-weight: 600;
  }
  .markdown-body :global(h1) { font-size: 1.4em; }
  .markdown-body :global(h2) { font-size: 1.2em; }
  .markdown-body :global(h3) { font-size: 1.1em; }
  .markdown-body :global(code) {
    font-family: var(--font-mono);
    font-size: 0.9em;
    padding: 2px 6px;
    background: var(--surface);
    border-radius: var(--radius-sm);
  }
  .markdown-body :global(pre) {
    margin: 8px 0;
    padding: 12px 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow-x: auto;
    position: relative;
  }
  .markdown-body :global(pre .code-lang) {
    position: absolute;
    top: 4px;
    left: 12px;
    font-size: var(--text-2xs);
    font-family: var(--font-sans);
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    pointer-events: none;
    opacity: 0.6;
  }
  .markdown-body :global(pre code) {
    padding: 0;
    background: none;
    font-size: var(--text-sm);
    line-height: 1.5;
  }
  /* highlight.js token colors — Aletheia warm syntax */
  .markdown-body :global(.hljs-keyword) { color: var(--syntax-keyword); }
  .markdown-body :global(.hljs-string),
  .markdown-body :global(.hljs-regexp) { color: var(--syntax-string); }
  .markdown-body :global(.hljs-number),
  .markdown-body :global(.hljs-literal) { color: var(--syntax-number); }
  .markdown-body :global(.hljs-comment) { color: var(--syntax-comment); font-style: italic; }
  .markdown-body :global(.hljs-function),
  .markdown-body :global(.hljs-title) { color: var(--syntax-function); }
  .markdown-body :global(.hljs-built_in) { color: var(--syntax-builtin); }
  .markdown-body :global(.hljs-type),
  .markdown-body :global(.hljs-class) { color: var(--syntax-type); }
  .markdown-body :global(.hljs-attr),
  .markdown-body :global(.hljs-attribute) { color: var(--syntax-attr); }
  .markdown-body :global(.hljs-variable),
  .markdown-body :global(.hljs-template-variable) { color: var(--syntax-builtin); }
  .markdown-body :global(.hljs-property) { color: var(--syntax-property); }
  .markdown-body :global(.hljs-tag) { color: var(--syntax-tag); }
  .markdown-body :global(.hljs-name) { color: var(--syntax-tag); }
  .markdown-body :global(.hljs-selector-class),
  .markdown-body :global(.hljs-selector-id),
  .markdown-body :global(.hljs-selector-tag) { color: var(--syntax-tag); }
  .markdown-body :global(.hljs-meta) { color: var(--syntax-meta); }
  .markdown-body :global(.hljs-addition) { color: var(--syntax-inserted); background: rgba(74, 154, 91, 0.15); }
  .markdown-body :global(.hljs-deletion) { color: var(--syntax-deleted); background: rgba(199, 84, 80, 0.15); }
  .markdown-body :global(.hljs-punctuation) { color: var(--text-secondary); }
  .markdown-body :global(pre .copy-btn) {
    position: absolute;
    top: 6px;
    right: 6px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    color: var(--text-muted);
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-family: var(--font-sans);
    opacity: 0;
    transition: opacity var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);
    cursor: pointer;
    z-index: 1;
  }
  .markdown-body :global(pre:hover .copy-btn) {
    opacity: 1;
  }
  .markdown-body :global(pre .copy-btn:hover) {
    color: var(--text);
    border-color: var(--accent);
  }
  .markdown-body :global(pre .copy-btn.copied) {
    color: var(--status-success);
    border-color: var(--status-success);
    opacity: 1;
  }
  .markdown-body :global(ul),
  .markdown-body :global(ol) {
    margin: 4px 0 8px;
    padding-left: 24px;
  }
  .markdown-body :global(li) {
    margin: 2px 0;
  }
  .markdown-body :global(blockquote) {
    margin: 8px 0;
    padding: 4px 16px;
    border-left: 3px solid var(--border);
    color: var(--text-secondary);
  }
  /* Wrap tables in scrollable container for mobile */
  .markdown-body :global(.table-wrapper) {
    width: 100%;
    overflow-x: auto;
    -webkit-overflow-scrolling: touch;
    margin: 8px 0;
  }
  .markdown-body :global(table) {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  .markdown-body :global(th),
  .markdown-body :global(td) {
    padding: 6px 12px;
    border: 1px solid var(--border);
    text-align: left;
    white-space: nowrap;
  }
  .markdown-body :global(th) {
    background: var(--surface);
    font-weight: 600;
  }
  .markdown-body :global(a) {
    color: var(--accent);
  }
  .markdown-body :global(hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 12px 0;
  }
  .markdown-body :global(img) {
    max-width: 100%;
    border-radius: var(--radius-sm);
  }
  .markdown-body :global(strong) {
    font-weight: 600;
  }

  @media (max-width: 768px) {
    .markdown-body :global(pre) {
      padding: 10px 12px;
      font-size: var(--text-sm);
      border-radius: var(--radius-sm);
      /* Ensure horizontal scroll works on touch */
      -webkit-overflow-scrolling: touch;
    }
    .markdown-body :global(pre code) {
      font-size: var(--text-sm);
    }
    .markdown-body :global(pre .copy-btn) {
      /* Always visible on mobile — no hover */
      opacity: 0.7;
      padding: 4px 10px;
      font-size: var(--text-sm);
    }
    .markdown-body :global(th),
    .markdown-body :global(td) {
      padding: 4px 8px;
      font-size: var(--text-sm);
    }
    .markdown-body :global(ul),
    .markdown-body :global(ol) {
      padding-left: 20px;
    }
  }
  /* Task list checkboxes (GFM) */
  .markdown-body :global(li:has(> input[type="checkbox"])) {
    list-style: none;
    margin-left: -20px;
  }
  .markdown-body :global(input[type="checkbox"]) {
    appearance: none;
    width: 14px;
    height: 14px;
    border: 1.5px solid var(--text-muted);
    border-radius: var(--radius-sm);
    background: transparent;
    vertical-align: middle;
    margin-right: 6px;
    position: relative;
    top: -1px;
    cursor: default;
  }
  .markdown-body :global(input[type="checkbox"]:checked) {
    background: var(--accent);
    border-color: var(--accent);
  }
  .markdown-body :global(input[type="checkbox"]:checked::after) {
    content: "\2713";
    display: block;
    color: #0f1114;
    font-size: var(--text-2xs);
    font-weight: 700;
    text-align: center;
    line-height: 14px;
  }
</style>
