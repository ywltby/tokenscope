// SF01（安全与数据一致性审查 Task 1）：图表 tooltip 安全输出契约。
// 红灯先行：formatter 必须返回由 document.createElement + textContent
// 构建的 HTMLElement——原始标签（模型/项目全名、系列名，含 <>&"'、中文、
// 长字符串）只作为文本出现，永不参与 innerHTML、属性名、URL 或 CSS 拼接；
// 不用自制黑名单正则"消毒"。ECharts 的 HTML tooltip 支持 DOM 分支
//（HTMLElement 会被 appendChild 挂载），换行/布局用固定节点。
import { describe, expect, it } from "vitest";
import { buildTooltipNode, type TooltipEntry } from "./chartTooltip";

function entries(names: string[], value = 5): TooltipEntry[] {
  return names.map((seriesName) => ({ seriesName, value }));
}

describe("chart_tooltip_treats_labels_as_text", () => {
  it("模型/项目全名含 HTML 标签时按完整文本输出，不产生注入元素", () => {
    const payload = "<img src=x onerror=window.__ATTACKED=1>";
    const node = buildTooltipNode({
      title: payload,
      entries: entries(["输入"]),
      formatValue: (v) => String(v),
    });
    // HTMLElement（DOM 分支），非字符串
    expect(node instanceof HTMLElement).toBe(true);
    // 原始标签完整可读（作为文本）
    expect(node.textContent).toContain(payload);
    // 没有注入的 img/脚本元素；全部子节点是文本或固定 <br>
    expect(node.querySelector("img, script, svg, iframe")).toBeNull();
    for (const child of Array.from(node.childNodes)) {
      const ok =
        child.nodeType === Node.TEXT_NODE ||
        (child.nodeType === Node.ELEMENT_NODE && (child as Element).tagName === "BR");
      expect(ok, `非白名单节点类型 ${child.nodeType}`).toBe(true);
    }
  });

  it("fallback 名、系列名、<>&\"'、中文与长字符串同样按文本处理", () => {
    const dangerous = "&<>\"' 中文模型名 <svg onload=alert(1)>";
    const long = `${"长".repeat(600)}<a href="javascript:alert(1)">x</a>`;
    const node = buildTooltipNode({
      title: null, // fallback：用 series 侧 name
      fallbackTitle: dangerous,
      entries: entries([dangerous, long]),
      formatValue: (v) => String(v),
    });
    expect(node.textContent).toContain(dangerous);
    expect(node.textContent).toContain(long);
    expect(node.querySelector("a, svg, script")).toBeNull();
    // 每个系列行都是文本节点 + <br>，没有属性/URL/CSS 拼接入口
    const elementChildren = Array.from(node.children).map((c) => c.tagName);
    expect(elementChildren.every((t) => t === "BR")).toBe(true);
  });

  it("数值行使用注入的 formatter 文本；空值行仍为文本", () => {
    const node = buildTooltipNode({
      title: "claude-opus-5-5",
      entries: [
        { seriesName: "输入", value: 1234 },
        { seriesName: "输出", value: null },
      ],
      formatValue: (v) => `#${v}`,
    });
    expect(node.textContent).toContain("输入 #1234");
    expect(node.textContent).toContain("输出");
  });
});
