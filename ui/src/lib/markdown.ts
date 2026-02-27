import { Marked, type Tokens } from "marked";
import DOMPurify from "dompurify";

// CodeMirror-based syntax highlighting — replaces highlight.js with Lezer parsers
// that are already bundled for the file editor. Zero additional dependencies.
import { highlightCode } from "@lezer/highlight";
import { tagHighlighter, tags } from "@lezer/highlight";
import { javascriptLanguage, typescriptLanguage } from "@codemirror/lang-javascript";
import { pythonLanguage } from "@codemirror/lang-python";
import { jsonLanguage } from "@codemirror/lang-json";
import { yamlLanguage } from "@codemirror/lang-yaml";
import { cssLanguage } from "@codemirror/lang-css";
import { htmlLanguage } from "@codemirror/lang-html";
import { markdownLanguage } from "@codemirror/lang-markdown";
import type { Language } from "@codemirror/language";

// Map hljs-compatible CSS classes to Lezer tags — keeps existing theme CSS working
const hljsHighlighter = tagHighlighter([
  { tag: tags.keyword, class: "hljs-keyword" },
  { tag: tags.controlKeyword, class: "hljs-keyword" },
  { tag: tags.operatorKeyword, class: "hljs-keyword" },
  { tag: tags.definitionKeyword, class: "hljs-keyword" },
  { tag: tags.moduleKeyword, class: "hljs-keyword" },
  { tag: tags.string, class: "hljs-string" },
  { tag: tags.special(tags.string), class: "hljs-string" },
  { tag: tags.regexp, class: "hljs-regexp" },
  { tag: tags.number, class: "hljs-number" },
  { tag: tags.integer, class: "hljs-number" },
  { tag: tags.float, class: "hljs-number" },
  { tag: tags.bool, class: "hljs-literal" },
  { tag: tags.null, class: "hljs-literal" },
  { tag: tags.comment, class: "hljs-comment" },
  { tag: tags.lineComment, class: "hljs-comment" },
  { tag: tags.blockComment, class: "hljs-comment" },
  { tag: tags.docComment, class: "hljs-comment" },
  { tag: tags.variableName, class: "hljs-variable" },
  { tag: tags.definition(tags.variableName), class: "hljs-variable" },
  { tag: tags.function(tags.variableName), class: "hljs-title" },
  { tag: tags.definition(tags.function(tags.variableName)), class: "hljs-title" },
  { tag: tags.propertyName, class: "hljs-property" },
  { tag: tags.function(tags.propertyName), class: "hljs-title" },
  { tag: tags.definition(tags.propertyName), class: "hljs-property" },
  { tag: tags.typeName, class: "hljs-type" },
  { tag: tags.className, class: "hljs-title" },
  { tag: tags.tagName, class: "hljs-tag" },
  { tag: tags.attributeName, class: "hljs-attr" },
  { tag: tags.attributeValue, class: "hljs-string" },
  { tag: tags.operator, class: "hljs-operator" },
  { tag: tags.punctuation, class: "hljs-punctuation" },
  { tag: tags.meta, class: "hljs-meta" },
  { tag: tags.processingInstruction, class: "hljs-meta" },
  { tag: tags.self, class: "hljs-built_in" },
  { tag: tags.atom, class: "hljs-literal" },
]);

// Language registry — maps language identifiers to Lezer parsers
const languageMap: Record<string, Language> = {
  typescript: typescriptLanguage,
  ts: typescriptLanguage,
  javascript: javascriptLanguage,
  js: javascriptLanguage,
  python: pythonLanguage,
  py: pythonLanguage,
  json: jsonLanguage,
  yaml: yamlLanguage,
  yml: yamlLanguage,
  css: cssLanguage,
  html: htmlLanguage,
  xml: htmlLanguage,
  svelte: htmlLanguage,
  markdown: markdownLanguage,
  md: markdownLanguage,
};

function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/**
 * Highlight code using CodeMirror's Lezer parsers.
 * Falls back to HTML-escaped plain text for unsupported languages.
 */
export function cmHighlightCode(code: string, language?: string): string {
  const lang = language ? languageMap[language.toLowerCase()] : undefined;
  if (!lang) return escapeHtml(code);

  const tree = lang.parser.parse(code);
  let result = "";
  let pos = 0;

  highlightCode(
    code,
    tree,
    hljsHighlighter,
    (text, classes) => {
      const escaped = escapeHtml(text);
      result += classes ? `<span class="${classes}">${escaped}</span>` : escaped;
      pos += text.length;
    },
    () => {
      result += "\n";
      pos++;
    },
  );

  // Append any remaining unhighlighted text
  if (pos < code.length) {
    result += escapeHtml(code.slice(pos));
  }

  return result;
}

// Re-export for backward compat — ToolPanel imports this name
export { cmHighlightCode as highlightCode };

const marked = new Marked({
  breaks: false,
  gfm: true,
  renderer: {
    code({ text, lang }: { text: string; lang?: string }) {
      const language = lang?.split(/\s/)[0] ?? "";
      const highlighted = cmHighlightCode(text, language || undefined);
      const label = language ? `<span class="code-lang">${language}</span>` : "";
      return `<pre class="code-block">${label}<code class="hljs">${highlighted}</code></pre>`;
    },
    table(_token: Tokens.Table) {
      return `<div class="table-wrapper"><table></table></div>`;
    },
  },
});

const tableRenderer = {
  renderer: {
    table({ header, rows }: Tokens.Table): string {
      const cell = (c: Tokens.TableCell): string => {
        const raw = c.tokens.map((t) => ("raw" in t ? (t as { raw: string }).raw : "")).join("");
        const inline = marked.parseInline(raw) as string;
        const content = typeof inline === "string" ? inline : String(inline ?? "");
        const tag = c.header ? "th" : "td";
        const align = c.align ? ` align="${c.align}"` : "";
        return `<${tag}${align}>${content}</${tag}>\n`;
      };
      const head = "<tr>\n" + header.map(cell).join("") + "</tr>\n";
      const body = rows.map((row: Tokens.TableCell[]) => "<tr>\n" + row.map(cell).join("") + "</tr>\n").join("");
      return `<table>\n<thead>\n${head}</thead>\n${body ? `<tbody>${body}</tbody>` : ""}</table>\n`;
    },
  },
};

marked.use(tableRenderer);

// Fast renderer for streaming — no highlighting, just HTML escaping
const markedFast = new Marked({
  breaks: false,
  gfm: true,
  renderer: {
    code({ text, lang }: { text: string; lang?: string }) {
      const language = lang?.split(/\s/)[0] ?? "";
      const label = language ? `<span class="code-lang">${language}</span>` : "";
      return `<pre class="code-block">${label}<code>${escapeHtml(text)}</code></pre>`;
    },
    table(_token: Tokens.Table) {
      return `<div class="table-wrapper"><table></table></div>`;
    },
  },
});

markedFast.use(tableRenderer);

export function renderMarkdownFast(text: string): string {
  const t0 = performance.now();
  if (typeof text !== "string") text = String(text ?? "");
  const raw = markedFast.parse(text, { async: false });
  const html = typeof raw === "string" ? raw : String(raw ?? "");
  const result = DOMPurify.sanitize(html, {
    ADD_ATTR: ["class", "type", "checked", "disabled"],
    ADD_TAGS: ["input"],
  });
  const ms = performance.now() - t0;
  if (ms > 10) console.warn(`[perf] renderMarkdownFast ${ms.toFixed(1)}ms (${text.length} chars)`);
  return result;
}

export function renderMarkdown(text: string): string {
  const t0 = performance.now();
  if (typeof text !== "string") text = String(text ?? "");
  const raw = marked.parse(text, { async: false });
  const html = typeof raw === "string" ? raw : String(raw ?? "");
  const result = DOMPurify.sanitize(html, {
    ADD_ATTR: ["class", "type", "checked", "disabled"],
    ADD_TAGS: ["input"],
  });
  const ms = performance.now() - t0;
  if (ms > 20) console.warn(`[perf] renderMarkdown ${ms.toFixed(1)}ms (${text.length} chars)`);
  return result;
}

/** Infer a language from a file path or tool name */
export function inferLanguage(toolName: string, input?: string): string | undefined {
  if (toolName === "exec") return "bash";
  if (toolName === "web_fetch") return "html";
  if (toolName === "web_search" || toolName === "ls") return undefined;

  const path = typeof input === "string" ? input : "";
  const ext = path.match(/\.(\w+)$/)?.[1]?.toLowerCase();
  if (!ext) return undefined;

  const extMap: Record<string, string> = {
    ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript",
    py: "python", sh: "bash", bash: "bash", fish: "bash",
    json: "json", yaml: "yaml", yml: "yaml", toml: "yaml",
    sql: "sql", md: "markdown", html: "html", xml: "xml",
    css: "css", svelte: "html",
  };
  return extMap[ext];
}

/** Infer language from a file path (for file preview) */
export function inferLanguageFromPath(filePath: string): string | undefined {
  const ext = filePath.match(/\.(\w+)$/)?.[1]?.toLowerCase();
  if (!ext) return undefined;

  const extMap: Record<string, string> = {
    ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript",
    py: "python", sh: "bash", bash: "bash", fish: "bash",
    json: "json", yaml: "yaml", yml: "yaml", toml: "yaml",
    sql: "sql", md: "markdown", html: "html", xml: "xml",
    css: "css", svelte: "html",
  };
  return extMap[ext];
}
