import { Marked } from "marked";
import DOMPurify from "dompurify";

const marked = new Marked({
  breaks: true,
  gfm: true,
});

// Lazy-loaded highlight.js instance
let hljs: typeof import("highlight.js").default | null = null;
let hljsLoading: Promise<void> | null = null;

async function ensureHljs(): Promise<typeof import("highlight.js").default> {
  if (hljs) return hljs;
  if (!hljsLoading) {
    hljsLoading = import("highlight.js").then((mod) => {
      hljs = mod.default;
    });
  }
  await hljsLoading;
  return hljs!;
}

export function renderMarkdown(text: string): string {
  const raw = marked.parse(text, { async: false }) as string;
  return DOMPurify.sanitize(raw, {
    ADD_ATTR: ["class"],
  });
}

export async function highlightCode(code: string, language?: string): Promise<string> {
  const hl = await ensureHljs();
  if (language && hl.getLanguage(language)) {
    return hl.highlight(code, { language }).value;
  }
  return hl.highlightAuto(code).value;
}

export function highlightCodeSync(code: string, language?: string): string {
  if (!hljs) return escapeHtml(code);
  if (language && hljs.getLanguage(language)) {
    return hljs.highlight(code, { language }).value;
  }
  return hljs.highlightAuto(code).value;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

// Pre-warm highlight.js on first import
ensureHljs();
