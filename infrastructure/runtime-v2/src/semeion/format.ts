// Markdown to Signal text style ranges
export type SignalTextStyle =
  | "BOLD"
  | "ITALIC"
  | "STRIKETHROUGH"
  | "MONOSPACE"
  | "SPOILER";

export interface StyleRange {
  start: number;
  length: number;
  style: SignalTextStyle;
}

export interface FormattedText {
  text: string;
  styles: StyleRange[];
}

export function formatForSignal(markdown: string): FormattedText {
  const styles: StyleRange[] = [];
  let text = markdown;

  text = applyInlineStyle(text, styles, /\*\*\*(.+?)\*\*\*/g, ["BOLD", "ITALIC"]);
  text = applyInlineStyle(text, styles, /\*\*(.+?)\*\*/g, ["BOLD"]);
  text = applyInlineStyle(text, styles, /\*(.+?)\*/g, ["ITALIC"]);
  text = applyInlineStyle(text, styles, /_(.+?)_/g, ["ITALIC"]);
  text = applyInlineStyle(text, styles, /~~(.+?)~~/g, ["STRIKETHROUGH"]);
  text = applyInlineStyle(text, styles, /\|\|(.+?)\|\|/g, ["SPOILER"]);

  text = applyCodeBlocks(text, styles);
  text = applyInlineCode(text, styles);

  text = applyLinks(text);

  return { text, styles };
}

function applyInlineStyle(
  text: string,
  styles: StyleRange[],
  pattern: RegExp,
  styleNames: SignalTextStyle[],
): string {
  let result = text;
  let offset = 0;

  for (const match of text.matchAll(pattern)) {
    const fullMatch = match[0];
    const inner = match[1];
    const originalIdx = match.index!;
    const adjustedIdx = originalIdx - offset;

    result =
      result.slice(0, adjustedIdx) +
      inner +
      result.slice(adjustedIdx + fullMatch.length);

    for (const style of styleNames) {
      styles.push({ start: adjustedIdx, length: inner.length, style });
    }

    offset += fullMatch.length - inner.length;
  }

  return result;
}

function applyCodeBlocks(text: string, styles: StyleRange[]): string {
  let result = text;
  const blockPattern = /```(?:\w+)?\n([\s\S]*?)```/g;
  let offset = 0;

  for (const match of text.matchAll(blockPattern)) {
    const fullMatch = match[0];
    const inner = match[1].replace(/\n$/, "");
    const originalIdx = match.index!;
    const adjustedIdx = originalIdx - offset;

    result =
      result.slice(0, adjustedIdx) +
      inner +
      result.slice(adjustedIdx + fullMatch.length);

    styles.push({ start: adjustedIdx, length: inner.length, style: "MONOSPACE" });
    offset += fullMatch.length - inner.length;
  }

  return result;
}

function applyInlineCode(text: string, styles: StyleRange[]): string {
  let result = text;
  const codePattern = /`([^`]+)`/g;
  let offset = 0;

  for (const match of text.matchAll(codePattern)) {
    const fullMatch = match[0];
    const inner = match[1];
    const originalIdx = match.index!;
    const adjustedIdx = originalIdx - offset;

    result =
      result.slice(0, adjustedIdx) +
      inner +
      result.slice(adjustedIdx + fullMatch.length);

    styles.push({ start: adjustedIdx, length: inner.length, style: "MONOSPACE" });
    offset += fullMatch.length - inner.length;
  }

  return result;
}

function applyLinks(text: string): string {
  return text.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, label, url) => {
    if (label === url || url.startsWith("mailto:")) return label;
    return `${label} (${url})`;
  });
}

export function stylesToSignalParam(styles: StyleRange[]): string[] {
  return styles.map((s) => `${s.start}:${s.length}:${s.style}`);
}
