// 设计系统 Task 3/4：SummaryCards 指标卡——显示层断言（不重算公式）。
import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import SummaryCards from "./SummaryCards.vue";
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

  it("命中率沿用 cache_read / (input + cache_read) 公式", () => {
    const w = mountCards(totals());
    // 50000 / (12000 + 50000) = 80.6%
    expect(w.text()).toContain("80.6%");
  });

  it("token 分项行使用低饱和语义色与文字，不使用 emoji", () => {
    const w = mountCards(totals());
    const parts = w.find('[aria-label="token 分项"]');
    expect(parts.exists()).toBe(true);
    const text = parts.text();
    expect(text).toContain("输入");
    expect(text).toContain("输出");
    expect(text).toContain("缓存写");
    expect(text).toContain("缓存命中");
    expect(text).toContain("12,000");
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
    expect(main.text()).toContain("USD · 估算值，非账单");
    const secondary = w.find(".metric-secondary");
    expect(secondary.findAll(".metric-item").length).toBe(3);
    expect(w.findAll(".metric-sep").length).toBe(2);
  });

  it("比例条按占比分段，宽度与 token 数一致", () => {
    const w = mountCards(totals());
    // total = 12000 + 3000 + 4000 + 50000 = 69000
    const segments = w.findAll(".bar-segment");
    expect(segments.length).toBe(4);
    expect(segments[0].attributes("style")).toContain("17.39%");
    expect(segments[3].attributes("style")).toContain("72.46%");
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

  it("命中率公式收进 tooltip（正文只留结论），标签可聚焦", () => {
    const w = mountCards(totals());
    // tooltip 内容含公式且触发器可聚焦（hover/focus 路径）
    const stub = w.find(".tooltip-stub");
    expect(stub.exists()).toBe(true);
    expect(stub.text()).toContain("命中率 = 缓存读");
    expect(stub.find('[tabindex="0"]').exists()).toBe(true);
    // 公式只存在于 tooltip 内容中，不得铺在卡片正文
    const cardText = w.find('[aria-label="用量指标"]').text();
    const outside = cardText.replace(stub.text(), "");
    expect(outside).not.toContain("命中率 = 缓存读");
  });
});
