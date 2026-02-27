// Tests for Slack mrkdwn ↔ Markdown format conversion (Spec 34, Phase 3)

import { describe, expect, it } from "vitest";
import { markdownToMrkdwn, chunkMrkdwn, mrkdwnToMarkdown, stripBotMention } from "./format.js";

// ---------------------------------------------------------------------------
// markdownToMrkdwn — outbound conversion
// ---------------------------------------------------------------------------

describe("markdownToMrkdwn", () => {
  it("returns empty string for empty input", () => {
    expect(markdownToMrkdwn("")).toBe("");
    expect(markdownToMrkdwn(undefined as unknown as string)).toBe("");
  });

  it("passes plain text through", () => {
    expect(markdownToMrkdwn("hello world")).toBe("hello world");
  });

  it("preserves bold, italic, strikethrough (shared syntax)", () => {
    expect(markdownToMrkdwn("**bold** _italic_ ~strike~")).toBe("**bold** _italic_ ~strike~");
  });

  it("escapes &, <, > in plain text", () => {
    expect(markdownToMrkdwn("a & b < c > d")).toBe("a &amp; b &lt; c &gt; d");
  });

  it("converts markdown links to Slack format", () => {
    expect(markdownToMrkdwn("[Click here](https://example.com)")).toBe(
      "<https://example.com|Click here>",
    );
  });

  it("converts headers to bold", () => {
    expect(markdownToMrkdwn("# Title")).toBe("*Title*");
    expect(markdownToMrkdwn("### Sub-heading")).toBe("*Sub-heading*");
  });

  it("preserves code blocks without mangling content", () => {
    const input = "Before\n```js\nconst x = a < b && c > d;\n```\nAfter";
    const result = markdownToMrkdwn(input);
    expect(result).toContain("```");
    // Code block should have escaped <> inside
    expect(result).toContain("&lt;");
    expect(result).toContain("&gt;");
    // Text outside code should also be escaped
    expect(result).toContain("Before");
    expect(result).toContain("After");
  });

  it("preserves inline code", () => {
    const input = "Use `foo < bar` here";
    const result = markdownToMrkdwn(input);
    expect(result).toContain("`foo &lt; bar`");
  });

  it("converts blockquotes", () => {
    // After escaping, > becomes &gt; then we convert back to >
    const result = markdownToMrkdwn("> quoted text");
    expect(result).toBe("> quoted text");
  });

  it("converts horizontal rules", () => {
    expect(markdownToMrkdwn("---")).toBe("───");
    expect(markdownToMrkdwn("***")).toBe("───");
  });
});

// ---------------------------------------------------------------------------
// chunkMrkdwn — message chunking
// ---------------------------------------------------------------------------

describe("chunkMrkdwn", () => {
  it("returns single chunk for short text", () => {
    expect(chunkMrkdwn("hello")).toEqual(["hello"]);
  });

  it("returns single chunk for text at limit", () => {
    const text = "a".repeat(4000);
    expect(chunkMrkdwn(text)).toEqual([text]);
  });

  it("splits at paragraph boundary when possible", () => {
    const para1 = "a".repeat(3000);
    const para2 = "b".repeat(3000);
    const text = `${para1}\n\n${para2}`;
    const chunks = chunkMrkdwn(text);
    expect(chunks.length).toBe(2);
    expect(chunks[0]).toBe(para1);
    expect(chunks[1]).toBe(para2);
  });

  it("splits at newline when no paragraph boundary", () => {
    const line1 = "a".repeat(3500);
    const line2 = "b".repeat(3500);
    const text = `${line1}\n${line2}`;
    const chunks = chunkMrkdwn(text);
    expect(chunks.length).toBe(2);
  });

  it("hard-cuts when no good break point", () => {
    const text = "a".repeat(8000);
    const chunks = chunkMrkdwn(text);
    expect(chunks.length).toBe(2);
    expect(chunks[0]!.length).toBeLessThanOrEqual(4000);
    expect(chunks[1]!.length).toBeLessThanOrEqual(4000);
  });
});

// ---------------------------------------------------------------------------
// mrkdwnToMarkdown — inbound conversion
// ---------------------------------------------------------------------------

describe("mrkdwnToMarkdown", () => {
  it("returns empty string for empty input", () => {
    expect(mrkdwnToMarkdown("")).toBe("");
  });

  it("converts Slack links to markdown", () => {
    expect(mrkdwnToMarkdown("<https://example.com|Click here>")).toBe(
      "[Click here](https://example.com)",
    );
  });

  it("converts bare Slack URLs", () => {
    expect(mrkdwnToMarkdown("<https://example.com>")).toBe("https://example.com");
  });

  it("converts user mentions", () => {
    expect(mrkdwnToMarkdown("<@U12345678>")).toBe("@U12345678");
  });

  it("converts channel mentions with name", () => {
    expect(mrkdwnToMarkdown("<#C12345678|general>")).toBe("#general");
  });

  it("converts channel mentions without name", () => {
    expect(mrkdwnToMarkdown("<#C12345678>")).toBe("#C12345678");
  });

  it("converts special commands", () => {
    expect(mrkdwnToMarkdown("<!here>")).toBe("@here");
    expect(mrkdwnToMarkdown("<!channel>")).toBe("@channel");
  });

  it("unescapes HTML entities", () => {
    expect(mrkdwnToMarkdown("a &amp; b &lt; c &gt; d")).toBe("a & b < c > d");
  });
});

// ---------------------------------------------------------------------------
// stripBotMention
// ---------------------------------------------------------------------------

describe("stripBotMention", () => {
  it("strips mention at start of message", () => {
    expect(stripBotMention("<@U99999> hello", "U99999")).toBe("hello");
  });

  it("strips mention with colon separator", () => {
    expect(stripBotMention("<@U99999>: hello", "U99999")).toBe("hello");
  });

  it("strips mention with extra whitespace", () => {
    expect(stripBotMention("<@U99999>  hello", "U99999")).toBe("hello");
  });

  it("does not strip mention in the middle", () => {
    expect(stripBotMention("hey <@U99999> hello", "U99999")).toBe("hey <@U99999> hello");
  });

  it("does not strip different bot mention", () => {
    expect(stripBotMention("<@U11111> hello", "U99999")).toBe("<@U11111> hello");
  });
});
