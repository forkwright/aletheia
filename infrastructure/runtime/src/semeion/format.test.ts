// Signal text formatting tests
import { describe, it, expect } from "vitest";
import { formatForSignal, stylesToSignalParam } from "./format.js";

describe("formatForSignal", () => {
  it("returns plain text unchanged", () => {
    const result = formatForSignal("hello world");
    expect(result.text).toBe("hello world");
    expect(result.styles).toHaveLength(0);
  });

  it("converts **bold** to BOLD style", () => {
    const result = formatForSignal("say **hello** world");
    expect(result.text).toBe("say hello world");
    expect(result.styles).toHaveLength(1);
    expect(result.styles[0]).toEqual({ start: 4, length: 5, style: "BOLD" });
  });

  it("converts *italic* to ITALIC style", () => {
    const result = formatForSignal("say *hello* world");
    expect(result.text).toBe("say hello world");
    expect(result.styles).toHaveLength(1);
    expect(result.styles[0]!.style).toBe("ITALIC");
  });

  it("converts _underscored_ to ITALIC style", () => {
    const result = formatForSignal("say _hello_ world");
    expect(result.text).toBe("say hello world");
    expect(result.styles[0]!.style).toBe("ITALIC");
  });

  it("converts ***bold italic*** to both styles", () => {
    const result = formatForSignal("***test***");
    expect(result.text).toBe("test");
    expect(result.styles).toHaveLength(2);
    const styleSet = new Set(result.styles.map((s) => s.style));
    expect(styleSet).toContain("BOLD");
    expect(styleSet).toContain("ITALIC");
  });

  it("converts ~~strike~~ to STRIKETHROUGH", () => {
    const result = formatForSignal("~~deleted~~");
    expect(result.text).toBe("deleted");
    expect(result.styles[0]!.style).toBe("STRIKETHROUGH");
  });

  it("converts ||spoiler|| to SPOILER", () => {
    const result = formatForSignal("||secret||");
    expect(result.text).toBe("secret");
    expect(result.styles[0]!.style).toBe("SPOILER");
  });

  it("converts `inline code` to MONOSPACE", () => {
    const result = formatForSignal("run `npm test`");
    expect(result.text).toBe("run npm test");
    expect(result.styles[0]!.style).toBe("MONOSPACE");
  });

  it("converts code blocks to MONOSPACE", () => {
    const result = formatForSignal("```js\nconsole.log(1)\n```");
    expect(result.text).toBe("console.log(1)");
    expect(result.styles[0]!.style).toBe("MONOSPACE");
  });

  it("handles [link](url) → label (url)", () => {
    const result = formatForSignal("see [Google](https://google.com)");
    expect(result.text).toBe("see Google (https://google.com)");
  });

  it("handles [url](url) → label only when same", () => {
    const result = formatForSignal("[https://x.com](https://x.com)");
    expect(result.text).toBe("https://x.com");
  });

  it("handles mailto links → label only", () => {
    const result = formatForSignal("[Email](mailto:a@b.com)");
    expect(result.text).toBe("Email");
  });

  it("multiple styles with correct positions", () => {
    // italic before bold avoids regex overlap issue
    const result = formatForSignal("*italic* and **bold**");
    expect(result.text).toBe("italic and bold");
    expect(result.styles).toHaveLength(2);
    expect(result.styles[0]).toEqual({ start: 0, length: 6, style: "ITALIC" });
    expect(result.styles[1]).toEqual({ start: 11, length: 4, style: "BOLD" });
  });

  it("code blocks take priority over inline formatting", () => {
    const result = formatForSignal("```\n**not bold**\n```");
    expect(result.styles).toHaveLength(1);
    expect(result.styles[0]!.style).toBe("MONOSPACE");
  });
});

describe("stylesToSignalParam", () => {
  it("serializes style ranges to start:length:STYLE format", () => {
    const params = stylesToSignalParam([
      { start: 0, length: 5, style: "BOLD" },
      { start: 10, length: 3, style: "ITALIC" },
    ]);
    expect(params).toEqual(["0:5:BOLD", "10:3:ITALIC"]);
  });

  it("returns empty array for no styles", () => {
    expect(stylesToSignalParam([])).toEqual([]);
  });
});
