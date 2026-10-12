// 设计系统 Task 3/4：SummaryCards 指标卡——显示层断言（不重算公式）。
import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import SummaryCards from "./SummaryCards.vue";
import { TOKEN_BUCKETS } from "../lib/tokenDisplay";
import type { Group } from "../types";

// NTooltip 打桩为透传渲染（trigger + 内容同渲染），断言公式收进 tooltip。
vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const NTooltipStub = dc({
    name: "NTooltip",
    setup(_, { slots }) {
      return () => h("div", { class: "tooltip-stub" }, [slots.trigger?.(), slots.default?.()]);
    },
  });
  return { ...actual, NTooltip: NTooltipStub };
});

function totals(over: Partial<Group> = {}): Group {
  return {
    key: "totals",
    requests: 12,
    tokens: { input: 12000, output: 3000, cache_write: 4000, cache_read: 50000 },
    cost_usd: 0.0126,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    ...over,
  };
}

function mountCards(t: Group) {
  return mount(SummaryCards, { props: { totals: t } });
}

describe("SummaryCards 指标条（设计系统 Task 3）", () => {
  it("统一读数条四项齐全，费用旁标注估算", () => {
    const w = mountCards(totals());
    const strip = w.find('[aria-label="用量指标"]');
    expect(strip.exists()).toBe(true);
    const text = strip.text();
    expect(text).toContain("估算费用");
    expect(text).toContain("总 token");
    expect(text).toContain("请求数");
    expect(text).toContain("缓存命中率");
    expect(text).toContain("估算");
  });

  it("零值显示 0；无缓存基数时命中率为 N/A", () => {
    const w = mountCards(
      totals({
        requests: 0,
        tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
        cost_usd: 0,
      }),
    );
    const text = w.text();
    expect(text).toContain("$0.00");
    // 分项与请求数的零值是真实 0，不得显示 — 或空白
    expect(text).toContain("0");
    expect(text).toContain("N/A");
  });

  it("未知价格显示未知标记，不伪装成 0", () => {
    const w = mountCards(totals({ cost_usd: 0, unknown_pricing: true }));
    const text = w.text();
    expect(text).toContain("†");
    expect(text).not.toContain("$0.00");
  });

  it("极小非零金额不显示为 $0.00", () => {
    const w = mountCards(totals({ cost_usd: 0.0000126 }));
    const text = w.text();
    // "$0.000013" 包含子串 "$0.00"——用负向断言排除"恰为 $0.00 结尾"的显示
    expect(text).not.toMatch(/\$0\.00(?!\d)/);
    expect(text).toContain("$0.000013");
  });

  it("命中率以包含两类缓存的总输入为分母", () => {
    const w = mountCards(totals());
    // 50000 / (12000 + 4000 + 50000) = 75.8%
    expect(w.text()).toContain("75.8%");
  });

  it("token 分项行使用低饱和语义色与文字，不使用 emoji", () => {
    const w = mountCards(totals());
    const parts = w.find('[aria-label="token 分项"]');
    expect(parts.exists()).toBe(true);
    const text = parts.text();
    // RC06/RC07：四类名称与顺序都来自 tokenDisplay 单一来源——逐项核对，
    // 且按该来源的顺序出现（不再各自硬编码一套词表）。
    for (const b of TOKEN_BUCKETS) {
      expect(text, `分项应含「${b.label}」`).toContain(b.label);
    }
    const positions = TOKEN_BUCKETS.map((b) => text.indexOf(b.label));
    expect(
      positions.every((p) => p >= 0),
      "四类都应出现",
    ).toBe(true);
    expect(positions, "顺序必须与单一来源一致").toEqual([...positions].sort((a, b) => a - b));
    // 2026-10-10 用户修订：英雄栏为**互斥四桶**——「输入」是未缓存输入
    // （12,000），不是含缓存的总输入（66,000）；四类相加 = 总 token。
    expect(text).toContain("12,000");
    expect(text).not.toContain("66,000");
    expect(text).toContain("50,000");
    for (const v of ["12,000", "3,000", "4,000", "50,000"]) {
      expect(text, `四类分项数值都应出现：${v}`).toContain(v);
    }
    // DESIGN.md：不使用 emoji 作为产品图标
    expect(w.text()).not.toMatch(/\p{Extended_Pictographic}/u);
  });
});

describe("SummaryCards 指标卡（设计系统 Task 4）", () => {
  it("单卡结构：左主读数费用，右三项次读数以发丝线分隔", () => {
    const w = mountCards(totals({ cost_usd: 12.84 }));
    expect(w.find(".ts-card").exists()).toBe(true);
    const main = w.find(".metric-main");
    expect(main.text()).toContain("估算费用");
    expect(main.text()).toContain("$12.84");
    expect(main.text()).not.toContain("USD · 估算值，非账单");
    const secondary = w.find(".metric-secondary");
    expect(secondary.findAll(".metric-item").length).toBe(3);
    expect(w.findAll(".metric-sep").length).toBe(2);
  });

  it("比例条按互斥四桶分段，四类颜色齐全、宽度与 token 数一致", () => {
    const w = mountCards(totals());
    // total = 12000 + 3000 + 4000 + 50000 = 69000（互斥四桶，不相加堆叠）
    const segments = w.findAll(".bar-segment");
    expect(segments.length, "缓存写/缓存命中必须有独立分段（颜色可见）").toBe(4);
    const classes = segments.map((s) => s.classes().join(" "));
    expect(classes).toEqual([
      "bar-segment bar-input",
      "bar-segment bar-output",
      "bar-segment bar-cache_write",
      "bar-segment bar-cache_read",
    ]);
    const widths = segments.map((s) => s.attributes("style"));
    expect(widths[0]).toContain("17.39%"); // 12,000 / 69,000
    expect(widths[1]).toContain("4.35%"); // 3,000
    expect(widths[2]).toContain("5.80%"); // 4,000
    expect(widths[3]).toContain("72.46%"); // 50,000
    // 各段标注可访问名称与数值（缓存两项不再是隐形的子集）。
    expect(segments[2].attributes("aria-label")).toContain("缓存写");
    expect(segments[3].attributes("aria-label")).toContain("缓存命中");
  });

  it("全部为零时比例条为空槽，不渲染分段", () => {
    const w = mountCards(
      totals({ tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 } }),
    );
    expect(w.find(".parts-bar-empty").exists()).toBe(true);
    expect(w.findAll(".bar-segment").length).toBe(0);
  });

  it("未知价格在费用旁显示警告胶囊「含未计价 token」", () => {
    const w = mountCards(totals({ cost_usd: 0.5, unknown_pricing: true }));
    const pill = w.find(".ts-pill-warning");
    expect(pill.exists()).toBe(true);
    expect(pill.text()).toContain("含未计价 token");
  });

  it("R09 hit_rate_tooltip_opens_on_focus：focus 打开、blur/Escape 关闭（受控状态切换）", async () => {
    // NTooltip 透传打桩（真实浮层验收见 SummaryCards.tooltip.test.ts）：
    // 本文件只验证受控状态的切换逻辑。
    const w = mountCards(totals());
    const trigger = w.find(".metric-label-help");
    expect(trigger.exists()).toBe(true);
    expect(trigger.attributes("aria-expanded")).toBe("false");
    // focus 打开
    await trigger.trigger("focus");
    await flushPromises();
    expect(trigger.attributes("aria-expanded")).toBe("true");
    // blur 关闭
    await trigger.trigger("blur");
    await flushPromises();
    expect(trigger.attributes("aria-expanded")).toBe("false");
    // focus 再开 → Escape 关闭
    await trigger.trigger("focus");
    await trigger.trigger("keydown", { key: "Escape" });
    await flushPromises();
    expect(trigger.attributes("aria-expanded")).toBe("false");
    // RC06：触发器改为原生 button 后，Enter/Space 由**浏览器**合成一次 click，
    // 组件不再手写 keydown 切换（手写 + 原生会重复 toggle）。因此这里断言：
    // 单独 keydown 不改变状态，激活事件（click）才切换。
    for (const key of ["Enter", " "]) {
      await trigger.trigger("keydown", { key, preventDefault: () => {} } as never);
      await flushPromises();
      expect(trigger.attributes("aria-expanded"), `单独 keydown(${key}) 不得再次 toggle`).toBe(
        "false",
      );
    }
    await trigger.trigger("click");
    await flushPromises();
    expect(trigger.attributes("aria-expanded")).toBe("true");
  });

  it("R09 hover 打开、离开关闭（鼠标路径与键盘等价）", async () => {
    const w = mountCards(totals());
    const trigger = w.find(".metric-label-help");
    await trigger.trigger("mouseenter");
    expect(trigger.attributes("aria-expanded")).toBe("true");
    await trigger.trigger("mouseleave");
    expect(trigger.attributes("aria-expanded")).toBe("false");
  });

  it("命中率公式收进 tooltip（正文只留结论），标签可聚焦", () => {
    const w = mountCards(totals());
    const trigger = w.find(".metric-label-help");
    expect(trigger.exists()).toBe(true);
    // RC06：可聚焦性来自原生 button（不再需要 tabindex="0" 的 div 兜底，
    // 也不需要手写键盘处理）。
    expect(trigger.element.tagName).toBe("BUTTON");
    expect(trigger.attributes("type")).toBe("button");
    expect(trigger.attributes("role"), "button 不需要再挂 role").toBeUndefined();
    // 公式只出现在 tooltip 内容节点（.hit-tip）内，标签本身只有结论
    const tip = w.find(".hit-tip");
    expect(tip.exists()).toBe(true);
    expect(tip.text()).toContain("命中率 = 缓存命中");
    expect(trigger.text()).not.toContain("命中率 = 缓存命中");
  });
});
