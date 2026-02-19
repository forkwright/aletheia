import { Marked } from "marked";
import DOMPurify from "dompurify";

// Selective highlight.js — only the languages we actually need (~90% smaller)
import hljs from "highlight.js/lib/core";
import typescript from "highlight.js/lib/languages/typescript";
import javascript from "highlight.js/lib/languages/javascript";
import python from "highlight.js/lib/languages/python";
import bash from "highlight.js/lib/languages/bash";
import json from "highlight.js/lib/languages/json";
import yaml from "highlight.js/lib/languages/yaml";
import sql from "highlight.js/lib/languages/sql";
import markdownLang from "highlight.js/lib/languages/markdown";
import xml from "highlight.js/lib/languages/xml";
import css from "highlight.js/lib/languages/css";

hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("python", python);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("shell", bash);
hljs.registerLanguage("sh", bash);
hljs.registerLanguage("json", json);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("yml", yaml);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("markdown", markdownLang);
hljs.registerLanguage("md", markdownLang);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("html", xml);
hljs.registerLanguage("css", css);
hljs.registerLanguage("ts", typescript);
hljs.registerLanguage("js", javascript);

export function highlightCode(code: string, language?: string): string {
  if (language && hljs.getLanguage(language)) {
    return hljs.highlight(code, { language }).value;
  }
  return hljs.highlightAuto(code).value;
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
  },
});

export function renderMarkdown(text: string): string {
  const raw = marked.parse(text, { async: false }) as string;
  return DOMPurify.sanitize(raw, {
    ADD_ATTR: ["class"],
  });
}

/** Infer a highlight.js language from a file path or tool name */
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
