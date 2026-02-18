// Link pre-processing — fetch URLs in messages and append previews
import { createLogger } from "../koina/logger.js";
import { validateUrl } from "../organon/built-in/ssrf-guard.js";

const log = createLogger("semeion:preprocess");

const URL_REGEX = /https?:\/\/[^\s<>[\]()'"]+/g;
const MAX_URLS = 3;
const FETCH_TIMEOUT_MS = 8000;
const MAX_PREVIEW_CHARS = 1500;

export async function preprocessLinks(
  text: string,
  maxUrls = MAX_URLS,
): Promise<string> {
  const matches = [...text.matchAll(URL_REGEX)];
  if (matches.length === 0) return text;

  const urls = [...new Set(matches.map((m) => m[0]))].slice(0, maxUrls);
  const previews: string[] = [];

  const results = await Promise.allSettled(
    urls.map((url) => fetchPreview(url)),
  );

  for (const result of results) {
    if (result.status === "fulfilled" && result.value) {
      previews.push(result.value);
    }
  }

  if (previews.length === 0) return text;
  return text + "\n\n" + previews.join("\n\n");
}

async function fetchPreview(url: string): Promise<string | null> {
  try {
    await validateUrl(url);
  } catch {
    log.debug(`Skipping private/blocked URL: ${url}`);
    return null;
  }

  try {
    const res = await fetch(url, {
      headers: {
        "User-Agent": "Aletheia/1.0",
        Accept: "text/html,application/json,text/plain,*/*",
      },
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
      redirect: "follow",
    });

    if (!res.ok) return null;

    const contentType = res.headers.get("content-type") ?? "";

    // Skip non-text responses (images, video, etc.)
    if (!contentType.includes("text/") && !contentType.includes("json")) {
      return null;
    }

    const html = await res.text();

    // Extract title
    const titleMatch = html.match(/<title[^>]*>([^<]+)<\/title>/i);
    const title = titleMatch?.[1]?.trim() ?? null;

    // Extract description meta tag
    const descMatch = html.match(
      /<meta[^>]*name=["']description["'][^>]*content=["']([^"']+)["']/i,
    );
    const description = descMatch?.[1]?.trim() ?? null;

    // Strip HTML for content
    const content = stripHtml(html).slice(0, MAX_PREVIEW_CHARS);

    const lines = [`[Link: ${url}]`];
    if (title) lines.push(`Title: ${title}`);
    if (description) lines.push(`Description: ${description}`);
    if (content && content.length > 50) {
      lines.push(content);
    }
    lines.push("[/Link]");

    return lines.join("\n");
  } catch (err) {
    log.debug(`Failed to preview ${url}: ${err instanceof Error ? err.message : err}`);
    return null;
  }
}

function stripHtml(html: string): string {
  let text = html;
  // Remove block elements with content (handles spaces before closing >)
  text = text.replace(/<script[\s\S]*?<\/script\s*>/gi, "");
  text = text.replace(/<style[\s\S]*?<\/style\s*>/gi, "");
  text = text.replace(/<nav[\s\S]*?<\/nav\s*>/gi, "");
  text = text.replace(/<footer[\s\S]*?<\/footer\s*>/gi, "");
  // Semantic whitespace
  text = text.replace(/<br\s*\/?>/gi, "\n");
  text = text.replace(/<\/p>/gi, "\n\n");
  text = text.replace(/<\/div>/gi, "\n");
  text = text.replace(/<\/h[1-6]>/gi, "\n\n");
  text = text.replace(/<li[^>]*>/gi, "- ");
  text = text.replace(/<\/li>/gi, "\n");
  // Strip all remaining tags — loop to handle malformed/nested tags
  let prev = "";
  while (prev !== text) {
    prev = text;
    text = text.replace(/<[^>]*>/g, "");
  }
  // Decode entities, then loop-strip script/style blocks and all tags until stable
  text = decodeHtmlEntities(text);
  prev = "";
  while (prev !== text) {
    prev = text;
    text = text.replace(/<script[\s\S]*?<\/script\s*>/gi, "");
    text = text.replace(/<style[\s\S]*?<\/style\s*>/gi, "");
    text = text.replace(/<[^>]*>/g, "");
  }
  text = text.replace(/\n{3,}/g, "\n\n");
  text = text.replace(/[ \t]+/g, " ");
  return text.trim();
}

function decodeHtmlEntities(text: string): string {
  const entities: Record<string, string> = {
    "&amp;": "&", "&lt;": "<", "&gt;": ">",
    "&quot;": '"', "&#39;": "'", "&nbsp;": " ",
    "&#x27;": "'", "&#x2F;": "/", "&#x60;": "`",
  };
  return text.replace(/&(?:#x?[0-9a-f]+|[a-z]+);/gi, (match) => entities[match] ?? match);
}
