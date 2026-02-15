<script lang="ts">
  import { renderMarkdown } from "../../lib/markdown";

  let { content }: { content: string } = $props();

  let html = $derived(renderMarkdown(content));
</script>

<div class="markdown-body">
  {@html html}
</div>

<style>
  .markdown-body {
    font-size: 14px;
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
    border-radius: 4px;
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
  .markdown-body :global(pre code) {
    padding: 0;
    background: none;
    font-size: 13px;
    line-height: 1.5;
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
  .markdown-body :global(table) {
    width: 100%;
    border-collapse: collapse;
    margin: 8px 0;
    font-size: 13px;
  }
  .markdown-body :global(th),
  .markdown-body :global(td) {
    padding: 6px 12px;
    border: 1px solid var(--border);
    text-align: left;
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
</style>
