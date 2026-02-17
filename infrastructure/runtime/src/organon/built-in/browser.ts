// Browser tool — navigate, screenshot, extract via headless Chromium
import type { ToolHandler } from "../registry.js";
import { validateUrl } from "./ssrf-guard.js";

let browserPromise: Promise<Browser> | null = null;
let browserInstance: Browser | null = null;
let pageCount = 0;
const MAX_PAGES = 3;
const PAGE_TIMEOUT = 30000;

type Browser = import("playwright-core").Browser;
type Page = import("playwright-core").Page;

async function getBrowser(): Promise<Browser> {
  if (!browserPromise) {
    browserPromise = (async () => {
      const { chromium } = await import("playwright-core");
      const browser = await chromium.launch({
        executablePath:
          process.env["CHROMIUM_PATH"] ?? "/usr/bin/chromium-browser",
        args: [
          "--no-sandbox",
          "--disable-setuid-sandbox",
          "--disable-dev-shm-usage",
          "--disable-gpu",
        ],
      });
      browserInstance = browser;
      return browser;
    })();
  }
  return browserPromise;
}

async function withPage<T>(fn: (page: Page) => Promise<T>): Promise<T> {
  if (pageCount >= MAX_PAGES) {
    throw new Error(`Max concurrent pages (${MAX_PAGES}) reached`);
  }

  const browser = await getBrowser();
  const page = await browser.newPage();
  pageCount++;

  let cleaned = false;
  const cleanup = () => {
    if (!cleaned) {
      cleaned = true;
      pageCount--;
      page.close().catch(() => {});
    }
  };

  const timer = setTimeout(cleanup, PAGE_TIMEOUT);

  try {
    return await fn(page);
  } finally {
    clearTimeout(timer);
    cleanup();
  }
}

export const browserTool: ToolHandler = {
  definition: {
    name: "browser",
    description:
      "Browse a URL with headless Chromium — renders JavaScript, takes screenshots, extracts via CSS selectors.\n\n" +
      "USE WHEN:\n" +
      "- Pages that require JavaScript to render content (SPAs, dynamic sites)\n" +
      "- Taking screenshots for visual verification\n" +
      "- Extracting structured data via CSS selectors\n\n" +
      "DO NOT USE WHEN:\n" +
      "- Static pages or APIs — use web_fetch instead (faster, lighter)\n" +
      "- Simple web searches — use web_search instead\n\n" +
      "TIPS:\n" +
      "- Actions: navigate (get text), screenshot (base64 PNG), extract (CSS selector)\n" +
      "- Use waitFor to wait for dynamic content before extracting\n" +
      "- Max 3 concurrent pages\n" +
      "- Requires Chromium — set CHROMIUM_PATH if not at default location",
    input_schema: {
      type: "object",
      properties: {
        url: {
          type: "string",
          description: "URL to navigate to",
        },
        action: {
          type: "string",
          enum: ["navigate", "screenshot", "extract"],
          description:
            "Action: navigate (get page text), screenshot (base64 PNG), extract (CSS selector)",
        },
        selector: {
          type: "string",
          description: "CSS selector for extract action",
        },
        waitFor: {
          type: "string",
          description: "CSS selector to wait for before extracting",
        },
        timeout: {
          type: "number",
          description: "Navigation timeout in ms (default: 15000)",
        },
      },
      required: ["url"],
    },
  },
  async execute(input: Record<string, unknown>): Promise<string> {
    const url = String(input["url"] ?? "");
    const action = String(input["action"] ?? "navigate");
    const selector = input["selector"] as string | undefined;
    const waitFor = input["waitFor"] as string | undefined;
    const timeout = (input["timeout"] as number) ?? 15000;

    try {
      await validateUrl(url);

      return await withPage(async (page) => {
        await page.goto(url, { waitUntil: "domcontentloaded", timeout });

        if (waitFor) {
          await page.waitForSelector(waitFor, { timeout }).catch(() => {});
        }

        switch (action) {
          case "screenshot": {
            const buffer = await page.screenshot({ type: "png", fullPage: false });
            return `data:image/png;base64,${buffer.toString("base64")}`;
          }

          case "extract": {
            if (!selector) return "Error: selector required for extract action";
            const elements = await page.$$eval(selector, (els) =>
              els.map((el) => el.textContent?.trim() ?? ""),
            );
            return elements.filter(Boolean).join("\n");
          }

          case "navigate":
          default: {
            const text = (await page.evaluate(`
              (() => {
                const body = document.body;
                if (!body) return "";
                for (const tag of ["script", "style", "nav", "footer", "header"]) {
                  for (const el of body.querySelectorAll(tag)) el.remove();
                }
                return body.innerText || "";
              })()
            `)) as string;
            const truncated =
              text.length > 50000
                ? text.slice(0, 50000) + "\n\n... [truncated]"
                : text;
            return truncated;
          }
        }
      });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      // If chromium not found, give actionable error
      if (msg.includes("ENOENT") || msg.includes("executable")) {
        return `Error: Chromium not found. Set CHROMIUM_PATH env or install chromium-headless.`;
      }
      return `Error: ${msg}`;
    }
  },
};

export async function closeBrowser(): Promise<void> {
  if (browserPromise) {
    const browser = await browserPromise;
    await browser.close();
    browserPromise = null;
    browserInstance = null;
  }
}

// Safety net: kill Chromium if still alive on unexpected exit.
// process.on("exit") is synchronous — can't await, so we use the cached
// browserInstance ref and send SIGKILL directly to the child process.
// Primary cleanup happens in daemon shutdown (closeBrowser()).
process.on("exit", () => {
  if (browserInstance) {
    try {
      (browserInstance as unknown as { process(): { kill(sig: string): void } | null }).process()?.kill("SIGKILL");
    } catch {
      // Best-effort — process may already be gone
    }
  }
});
