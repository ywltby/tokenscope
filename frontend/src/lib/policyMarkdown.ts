/// 隐私政策的**离线确定性渲染**（P04）：只做结构化行解析，不引第三方
/// markdown 库、不使用 v-html、不产生任何外部请求（图片/字体/iframe/预取
/// 全不涉及）。
///
/// 覆盖 `docs/privacy.md` 实际使用的语法：`#/##/###` 标题、`- ` 列表、空行
/// 分段、行内 `**粗体**`（去标记保留文本）、`` `code` ``、`[文字](url)`
/// ——链接渲染为「文字（url）」**纯文本**：未同意时第三方 URL 只作文本展示，
/// 不产生可点击外链（避免从同意页发起外部访问）。

export type PolicyBlock =
  { type: "h1" | "h2" | "h3" | "p"; text: string } | { type: "ul"; items: string[] };

/** 行内标记归一：粗体/行内代码去标记，链接降级为纯文本。 */
export function inlineText(raw: string): string {
  return raw
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, "$1（$2）")
    .trim();
}

/** Markdown → 结构化块（纯函数；块顺序与源文档一致，逐行不丢内容）。 */
export function renderPolicyMarkdown(markdown: string): PolicyBlock[] {
  const blocks: PolicyBlock[] = [];
  let paragraph: string[] = [];
  let list: string[] = [];

  const flushParagraph = (): void => {
    if (paragraph.length) {
      blocks.push({ type: "p", text: inlineText(paragraph.join(" ")) });
      paragraph = [];
    }
  };
  const flushList = (): void => {
    if (list.length) {
      blocks.push({ type: "ul", items: list });
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
    const item = /^[-*]\s+(.*)$/.exec(line);
    if (item) {
      flushParagraph();
      list.push(inlineText(item[1]));
      continue;
    }
    flushList();
    paragraph.push(line.trim());
  }
  flushParagraph();
  flushList();
  return blocks;
}
