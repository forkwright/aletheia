// Browser tool — navigate, screenshot, extract via headless Chromium
import type { ToolHandler } from "../registry.js";

let browserPromise: Promise<Browser> | null = null;
let pageCount = 0;
const MAX_PAGES = 3;
const PAGE_TIMEOUT = 30000;

type Browser = import("playwright-core").Browser;
type Page = import("playwright-core").Page;

async function getBrowser(): Promise<Browser> {
  if (!browserPromise) {
    browserPromise = (async () => {
      const { chromium } = await import("playwright-core");
      return chromium.launch({
        executablePath:
          process.env.CHROMIUM_PATH || "/usr/bin/chromium-browser",
        args: [
          "--no-sandbox",
          "--disable-setuid-sandbox",
          "--disable-dev-shm-usage",
          "--disable-gpu",
        ],
      });
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

  const timer = setTimeout(() => {
    page.close().catch(() => {});
    pageCount--;
  }, PAGE_TIMEOUT);

  try {
    const result = await fn(page);
    return result;
  } finally {
    clearTimeout(timer);
    await page.close().catch(() => {});
    pageCount--;
  }
}

export const browserTool: ToolHandler = {
  definition: {
    name: "browser",
    description:
      "Browse a URL with a headless browser. Renders JavaScript, can take screenshots, and extract content via CSS selectors. Use for pages that require JS rendering.",
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
    const url = String(input.url ?? "");
    const action = String(input.action ?? "navigate");
    const selector = input.selector as string | undefined;
    const waitFor = input.waitFor as string | undefined;
    const timeout = (input.timeout as number) ?? 15000;

    try {
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
  }
}
