// SF01（安全与数据一致性审查 Task 1）：图表 tooltip 的安全输出边界。
// formatter 返回 HTMLElement——由 document.createElement + textContent
// 构建，ECharts 的 HTML tooltip 走 DOM 分支（appendChild 挂载），换行用
// 固定 <br> 节点。原始标签（模型/项目全名、系列名）永不参与 innerHTML、
// 属性名、URL 或 CSS 拼接；不做自制黑名单正则"消毒"——安全来自输出
// 边界（只有 textContent 能写入不可信文本），而非字符串过滤。

export interface TooltipEntry {
  /** 系列名（如"输入/输出/缓存写/缓存命中"）——不可信文本 */
  seriesName: string;
  value: number | null | undefined;
}

export interface TooltipInput {
  /** 完整原始键（full label）；缺省时用 fallbackTitle */
  title?: string | null;
  /** title 缺失时的回退名（原始参数 name）——同样不可信 */
  fallbackTitle?: string | null;
  entries: TooltipEntry[];
  /** 数值 → 显示文本（后端金额口径的展示格式化） */
  formatValue: (v: number) => string;
}

/** 附加一行文本节点（无格式化拼接）。 */
function appendLine(root: HTMLElement, text: string, useBreak: boolean): void {
  if (useBreak) root.appendChild(document.createElement("br"));
  root.appendChild(document.createTextNode(text));
}

/**
 * 构建 tooltip 根节点：标题行 + 每系列一行 "系列名 数值"。
 * 全部内容经 textNode 写入；document 不可用时抛错（不存在字符串回退路径，
 * 不给 innerHTML 拼接留出口）。
 */
export function buildTooltipNode(input: TooltipInput): HTMLElement {
  const root = document.createElement("div");
  root.className = "chart-tooltip";
  const title = input.title ?? input.fallbackTitle ?? "";
  appendLine(root, title, false);
  input.entries.forEach((e) => {
    const valueText = e.value == null ? "" : ` ${input.formatValue(Number(e.value))}`;
    appendLine(root, `${e.seriesName}${valueText}`, true);
  });
  return root;
}
