// Markdown to Signal text style ranges — single-pass to avoid offset drift
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

interface Segment {
  start: number;
  end: number;
  inner: string;
  styles: SignalTextStyle[];
}

export function formatForSignal(markdown: string): FormattedText {
  const segments = collectSegments(markdown);

  segments.sort((a, b) => a.start - b.start);

  // Remove overlapping segments (earlier/longer wins)
  const filtered: Segment[] = [];
  let lastEnd = 0;
  for (const seg of segments) {
    if (seg.start >= lastEnd) {
      filtered.push(seg);
      lastEnd = seg.end;
    }
  }

  // Build output in one pass — positions are always correct
  const styles: StyleRange[] = [];
  let output = "";
  let pos = 0;

  for (const seg of filtered) {
    output += markdown.slice(pos, seg.start);
    const styleStart = output.length;
    output += seg.inner;

    for (const style of seg.styles) {
      styles.push({ start: styleStart, length: seg.inner.length, style });
    }

    pos = seg.end;
  }

  output += markdown.slice(pos);

  return { text: output, styles };
}

function collectSegments(text: string): Segment[] {
  const segments: Segment[] = [];

  // Code blocks (highest priority — suppress inner markdown)
  for (const match of text.matchAll(/```(?:\w+)?\n([\s\S]*?)```/g)) {
    const inner = match[1]!;
    segments.push({
      start: match.index!,
      end: match.index! + match[0].length,
      inner: inner.replace(/\n$/, ""),
      styles: ["MONOSPACE"],
    });
  }

  // Inline code (next priority)
  for (const match of text.matchAll(/`([^`]+)`/g)) {
    const inner = match[1]!;
    const start = match.index!;
    const end = start + match[0].length;
    if (overlaps(start, end, segments)) continue;
    segments.push({ start, end, inner, styles: ["MONOSPACE"] });
  }

  // Inline formatting (only outside code regions)
  const patterns: Array<{ regex: RegExp; styles: SignalTextStyle[] }> = [
    { regex: /\*\*\*(.+?)\*\*\*/g, styles: ["BOLD", "ITALIC"] },
    { regex: /\*\*(.+?)\*\*/g, styles: ["BOLD"] },
    { regex: /\*(.+?)\*/g, styles: ["ITALIC"] },
    { regex: /_(.+?)_/g, styles: ["ITALIC"] },
    { regex: /~~(.+?)~~/g, styles: ["STRIKETHROUGH"] },
    { regex: /\|\|(.+?)\|\|/g, styles: ["SPOILER"] },
  ];

  for (const { regex, styles } of patterns) {
    for (const match of text.matchAll(regex)) {
      const inner = match[1]!;
      const start = match.index!;
      const end = start + match[0].length;
      if (overlaps(start, end, segments)) continue;
      segments.push({ start, end, inner, styles });
    }
  }

  // Links: [label](url) → "label (url)" or just "label"
  for (const match of text.matchAll(/\[([^\]]+)\]\(([^)]+)\)/g)) {
    const label = match[1]!;
    const url = match[2]!;
    const start = match.index!;
    const end = start + match[0].length;
    if (overlaps(start, end, segments)) continue;

    const inner =
      label === url || url.startsWith("mailto:") ? label : `${label} (${url})`;
    segments.push({ start, end, inner, styles: [] });
  }

  return segments;
}

function overlaps(start: number, end: number, segments: Segment[]): boolean {
  return segments.some((s) => start < s.end && end > s.start);
}

export function stylesToSignalParam(styles: StyleRange[]): string[] {
  return styles.map((s) => `${s.start}:${s.length}:${s.style}`);
}
