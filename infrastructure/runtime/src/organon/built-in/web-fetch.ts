// Web fetch tool — retrieve URL content
import type { ToolHandler } from "../registry.js";
import { validateUrl } from "./ssrf-guard.js";

export const webFetchTool: ToolHandler = {
  definition: {
    name: "web_fetch",
    description:
      "Fetch content from a URL. Returns the page text content, HTML stripped.",
    input_schema: {
      type: "object",
      properties: {
        url: {
          type: "string",
          description: "URL to fetch",
        },
        maxLength: {
          type: "number",
          description: "Maximum response length in characters (default: 50000)",
        },
      },
      required: ["url"],
    },
  },
  async execute(input: Record<string, unknown>): Promise<string> {
    const url = input["url"] as string;
    const maxLength = (input["maxLength"] as number) ?? 50000;

    try {
      await validateUrl(url);

      const res = await fetch(url, {
        headers: {
          "User-Agent": "Aletheia/1.0",
          Accept: "text/html,application/json,text/plain,*/*",
        },
        signal: AbortSignal.timeout(15000),
        redirect: "follow",
      });

      if (!res.ok) {
        return `Error: HTTP ${res.status} ${res.statusText}`;
      }

      const contentLength = res.headers.get("content-length");
      if (contentLength && parseInt(contentLength, 10) > maxLength * 2) {
        return `Error: Response too large (${contentLength} bytes, limit ${maxLength * 2})`;
      }

      const contentType = res.headers.get("content-type") ?? "";
      const text = await res.text();

      let content: string;
      if (contentType.includes("text/html")) {
        content = stripHtml(text);
      } else {
        content = text;
      }

      if (content.length > maxLength) {
        return content.slice(0, maxLength) + "\n\n... [truncated]";
      }

      return content;
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      return `Error: ${msg}`;
    }
  },
};

function stripHtml(html: string): string {
  let text = html;

  text = text.replace(/<script[^>]*>[\s\S]*?<\/script>/gi, "");
  text = text.replace(/<style[^>]*>[\s\S]*?<\/style>/gi, "");
  text = text.replace(/<nav[^>]*>[\s\S]*?<\/nav>/gi, "");
  text = text.replace(/<footer[^>]*>[\s\S]*?<\/footer>/gi, "");

  text = text.replace(/<br\s*\/?>/gi, "\n");
  text = text.replace(/<\/p>/gi, "\n\n");
  text = text.replace(/<\/div>/gi, "\n");
  text = text.replace(/<\/h[1-6]>/gi, "\n\n");
  text = text.replace(/<li[^>]*>/gi, "- ");
  text = text.replace(/<\/li>/gi, "\n");

  text = text.replace(/<a[^>]*href="([^"]*)"[^>]*>([^<]*)<\/a>/gi, "$2 ($1)");

  text = text.replace(/<[^>]+>/g, "");

  text = text.replace(/&amp;/g, "&");
  text = text.replace(/&lt;/g, "<");
  text = text.replace(/&gt;/g, ">");
  text = text.replace(/&quot;/g, '"');
  text = text.replace(/&#39;/g, "'");
  text = text.replace(/&nbsp;/g, " ");

  text = text.replace(/\n{3,}/g, "\n\n");
  text = text.replace(/[ \t]+/g, " ");

  return text.trim();
}
