/// 隐私政策的**离线确定性渲染**（P04）：只做结构化行解析，不引第三方
/// markdown 库、不使用 v-html、不产生任何外部请求（图片/字体/iframe/预取
/// 全不涉及）。
///
/// 覆盖 `docs/privacy.md` 实际使用的语法：`#/##/###` 标题、`- ` 列表、空行
/// 分段、行尾 `\` 硬换行、行内 `**粗体**`（去标记保留文本）、`` `code` ``、`[文字](url)`
/// ——链接渲染为「文字（url）」**纯文本**：未同意时第三方 URL 只作文本展示，
/// 不产生可点击外链（避免从同意页发起外部访问）。

export type PolicyBlock =
  { type: "h1" | "h2" | "h3" | "p"; text: string } | { type: "ul"; items: string[] };

/** 行内标记归一：粗体/行内代码去标记，链接降级为纯文本。 */
export function inlineText(raw: string): string {
  return raw
    .replace(/\*\*(.+?)\*\*/gs, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, "$1（$2）")
    .trim();
}

/** 行尾未转义的反斜杠才是硬换行标记。 */
function endsWithHardBreak(line: string): boolean {
  const slashes = /\\+$/.exec(line)?.[0].length ?? 0;
  return slashes % 2 === 1;
}

/** 普通换行合为空格，硬换行保留为换行符，行内代码保持原文。 */
function inlineLines(lines: string[]): string {
  const trimmed = lines.map((line) => line.trim());
  const joined = trimmed.join("\n");
  const codeSpans = [...joined.matchAll(/`[^`]+`/g)].map((match) => ({
    start: match.index,
    end: match.index + match[0].length,
  }));
  let offset = 0;
  let text = "";
  for (let index = 0; index < trimmed.length; index++) {
    const line = trimmed[index];
    const slashOffset = offset + line.length - 1;
    const insideCode = codeSpans.some(
      ({ start, end }) => start <= slashOffset && slashOffset < end,
    );
    const hardBreak = index < trimmed.length - 1 && endsWithHardBreak(lines[index]) && !insideCode;
    text += hardBreak ? `${line.slice(0, -1)}\n` : line;
    if (index < trimmed.length - 1 && !hardBreak) text += " ";
    offset += line.length + 1;
  }
  return inlineText(text);
}

/** Markdown → 结构化块（纯函数；块顺序与源文档一致，逐行不丢内容）。 */
export function renderPolicyMarkdown(markdown: string): PolicyBlock[] {
  const blocks: PolicyBlock[] = [];
  let paragraph: string[] = [];
  let list: string[][] = [];

  const flushParagraph = (): void => {
    if (paragraph.length) {
      blocks.push({ type: "p", text: inlineLines(paragraph) });
      paragraph = [];
    }
  };
  const flushList = (): void => {
    if (list.length) {
      blocks.push({ type: "ul", items: list.map(inlineLines) });
      list = [];
    }
  };

  for (const rawLine of markdown.split(/\r?\n/)) {
    const line = rawLine.trimEnd();
    if (line.trim() === "") {
      flushParagraph();
      flushList();
      continue;
    }
    const heading = /^(#{1,3})\s+(.*)$/.exec(line);
    if (heading) {
      flushParagraph();
      flushList();
      const level = heading[1].length;
      const type: "h1" | "h2" | "h3" = level === 1 ? "h1" : level === 2 ? "h2" : "h3";
      blocks.push({ type, text: inlineText(heading[2]) });
      continue;
    }
    const item = /^[-*]\s+(.*)$/.exec(rawLine);
    if (item) {
      flushParagraph();
      list.push([item[1]]);
      continue;
    }
    const lastItem = list.at(-1);
    if (lastItem && endsWithHardBreak(lastItem[lastItem.length - 1])) {
      lastItem.push(rawLine);
      continue;
    }
    flushList();
    paragraph.push(rawLine);
  }
  flushParagraph();
  flushList();
  return blocks;
}
