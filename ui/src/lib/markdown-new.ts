import { Marked, type Tokens } from "marked";
import DOMPurify from "dompurify";

// Simple highlighting using CodeMirror's existing CSS classes
// This provides a lighter alternative to highlight.js while maintaining compatibility

export function highlightCode(code: string, language?: string): string {
  // For this Quick Win, we'll use a simple approach that removes highlight.js
  // while maintaining the existing hljs CSS classes for compatibility
  
  if (!language) {
    return code
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  // Simple keyword-based highlighting for the most common languages
  // This provides basic syntax highlighting while being much lighter than highlight.js
  let highlighted = code
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  switch (language.toLowerCase()) {
    case "javascript":
    case "js":
    case "typescript":
    case "ts":
      highlighted = highlighted
        .replace(/\b(const|let|var|function|class|import|export|from|if|else|for|while|return|true|false|null|undefined)\b/g, '<span class="hljs-keyword">$1</span>')
        .replace(/\b(\d+\.?\d*)\b/g, '<span class="hljs-number">$1</span>')
        .replace(/(["'`])([^"'`]*?)\1/g, '<span class="hljs-string">$1$2$1</span>')
        .replace(/\/\/.*$/gm, '<span class="hljs-comment">$&</span>')
        .replace(/\/\*[\s\S]*?\*\//g, '<span class="hljs-comment">$&</span>');
      break;

    case "python":
    case "py":
      highlighted = highlighted
        .replace(/\b(def|class|import|from|if|elif|else|for|while|return|True|False|None|and|or|not|in|is|with|as|try|except|finally)\b/g, '<span class="hljs-keyword">$1</span>')
        .replace(/\b(\d+\.?\d*)\b/g, '<span class="hljs-number">$1</span>')
        .replace(/(["'])([^"']*?)\1/g, '<span class="hljs-string">$1$2$1</span>')
        .replace(/#.*$/gm, '<span class="hljs-comment">$&</span>');
      break;

    case "json":
      highlighted = highlighted
        .replace(/(["'])([^"']*?)\1/g, '<span class="hljs-string">$1$2$1</span>')
        .replace(/\b(true|false|null)\b/g, '<span class="hljs-literal">$1</span>')
        .replace(/\b(\d+\.?\d*)\b/g, '<span class="hljs-number">$1</span>');
      break;

    case "bash":
    case "shell":
    case "sh":
      highlighted = highlighted
        .replace(/\b(if|then|else|elif|fi|for|while|do|done|function|case|esac|echo|cd|ls|grep|awk|sed)\b/g, '<span class="hljs-keyword">$1</span>')
        .replace(/(["'])([^"']*?)\1/g, '<span class="hljs-string">$1$2$1</span>')
        .replace(/#.*$/gm, '<span class="hljs-comment">$&</span>')
        .replace(/\$\w+/g, '<span class="hljs-variable">$&</span>');
      break;

    case "css":
      highlighted = highlighted
        .replace(/([.#]\w+)/g, '<span class="hljs-selector-class">$1</span>')
        .replace(/(\w+)\s*:/g, '<span class="hljs-attribute">$1</span>:')
        .replace(/\/\*[\s\S]*?\*\//g, '<span class="hljs-comment">$&</span>');
      break;

    case "html":
    case "xml":
      highlighted = highlighted
        .replace(/&lt;(\/?)([\w-]+)/g, '&lt;<span class="hljs-tag">$1$2</span>')
        .replace(/(\w+)=/g, '<span class="hljs-attr">$1</span>=')
        .replace(/(["'])([^"']*?)\1/g, '<span class="hljs-string">$1$2$1</span>')
        .replace(/&lt;!--[\s\S]*?--&gt;/g, '<span class="hljs-comment">$&</span>');
      break;

    case "yaml":
    case "yml":
      highlighted = highlighted
        .replace(/^(\s*)([^:\s]+):/gm, '$1<span class="hljs-attr">$2</span>:')
        .replace(/(["'])([^"']*?)\1/g, '<span class="hljs-string">$1$2$1</span>')
        .replace(/\b(true|false|null)\b/g, '<span class="hljs-literal">$1</span>')
        .replace(/\b(\d+\.?\d*)\b/g, '<span class="hljs-number">$1</span>')
        .replace(/#.*$/gm, '<span class="hljs-comment">$&</span>');
      break;

    default:
      // No highlighting for unknown languages
      break;
  }

  return highlighted;
}

const marked = new Marked({
  breaks: false,
  gfm: true,
  renderer: {
    code({ text, lang }: { text: string; lang?: string }) {
      const language = lang?.split(/\s/)[0] ?? "";
      const highlighted = highlightCode(text, language || undefined);
      const label = language ? `<span class="code-lang">${language}</span>` : "";
      return `<pre class="code-block">${label}<code class="hljs">${highlighted}</code></pre>`;
    },
    table(_token: Tokens.Table) {
      // Overridden by tableRenderer below — this branch never executes
      return `<div class="table-wrapper"><table></table></div>`;
    },
  },
});

// Explicit table renderer — bypasses potential internal state issues with
// marked's default table rendering in browser bundles
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

// Fast renderer for streaming — skips highlighting to avoid blocking the main thread.
// Code blocks are HTML-escaped only; syntax highlighting fires once after streaming ends.
const markedFast = new Marked({
  breaks: false,
  gfm: true,
  renderer: {
    code({ text, lang }: { text: string; lang?: string }) {
      const language = lang?.split(/\s/)[0] ?? "";
      const label = language ? `<span class="code-lang">${language}</span>` : "";
      const escaped = text
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;");
      return `<pre class="code-block">${label}<code>${escaped}</code></pre>`;
    },
    table(_token: Tokens.Table) {
      // Overridden by tableRenderer below — this branch never executes
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

/** Infer a CodeMirror language from a file path or tool name */
export function inferLanguage(toolName: string, input?: string): string | undefined {
  if (toolName === "exec") return "bash";
  if (toolName === "web_fetch") return "html";
  if (toolName === "web_search" || toolName === "ls") return undefined;

  // For file tools, try to infer from the file path in the input
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