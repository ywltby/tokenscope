// 设计系统 Task 3/4：SummaryCards 指标卡——显示层断言（不重算公式）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import SummaryCards from "./SummaryCards.vue";
import { DEFAULT_LOCALE, setLocale } from "../lib/i18n";
import { TOKEN_BUCKETS } from "../lib/tokenDisplay";
import type { Group } from "../types";

afterEach(() => {
  setLocale(DEFAULT_LOCALE);
});

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
    // v0.9.2：费用旁不再有常驻来源/估算标注（构成进悬浮详情）。
    expect(text).not.toContain("来源记录");
  });

  it("零值显示 0；无缓存基数时命中率为 N/A", () => {
    const w = mountCards(
      totals({
        requests: 0,
        tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
        cost_usd: 0,
        total_cost_usd: 0,
      }),
    );
    const text = w.text();
    expect(text).toContain("$0.00");
    // 分项与请求数的零值是真实 0，不得显示 — 或空白
    expect(text).toContain("0");
    expect(text).toContain("N/A");
  });

  it("未知价格显示「未知」，不伪装成 0、不带脚注符号", () => {
    const w = mountCards(totals({ cost_usd: 0, total_cost_usd: null, unknown_pricing: true }));
    const text = w.text();
    expect(text).toContain("未知");
    expect(text).not.toContain("†");
    expect(text).not.toContain("$0.00");
  });

  it("极小非零金额不显示为 $0.00", () => {
    const w = mountCards(totals({ cost_usd: 0.0000126, total_cost_usd: 0.0000126 }));
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
    const w = mountCards(
      totals({ cost_usd: 12.84, total_cost_usd: 12.84, has_estimated_cost: true }),
    );
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

  it("部分未定价 → 橙色金额、无常驻脚注（v0.9.3）", () => {
    const w = mountCards(
      totals({
        cost_usd: 0.5,
        total_cost_usd: 0.5,
        unknown_pricing: true,
        cost_partially_unknown: true,
        unknown_tokens: { input: 100, output: 0, cache_write: 0, cache_read: 0 },
      }),
    );
    const main = w.find(".metric-value-main .gcb-amount-main");
    expect(main.exists()).toBe(true);
    // 主读数（触发器内）只有金额；构成/缺失说明只在悬浮内容里。
    expect(main.text()).toContain("$0.50");
    expect(main.text()).not.toContain("†");
    expect(main.classes()).toContain("gcb-warning");
    expect(main.text()).not.toContain("未定价 token 合计");
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

// v0.9.2（2026-11-15 用户产品决策）：N18/T3/来源费用三类披露**不再常驻**
// ——总览卡费用区只显示单一金额（未定价用警告色表达，v0.9.3 起无脚注符号）；构成（分档估算、
// 按 0 估算、来源费用、覆盖完整性）全部进悬浮详情（GroupCostBreakdownTooltip）。
describe("v0.9.2 费用披露进悬浮（总览卡不再常驻）", () => {
  it("N18/T3/来源胶囊均不常驻显示", () => {
    const w = mountCards(
      totals({
        cache_write_split_estimated: true,
        cache_price_zero_estimated: true,
        unknown_pricing: false,
        source_cost_usd: 9,
        total_cost_usd: 9,
        cost_detail: "rollup_only",
      }),
    );
    // 定位常驻区（主读数触发器 + 指标卡标签），悬浮内容挂 body 不算常驻。
    const main = w.find(".metric-value-main .gcb-amount-main");
    expect(main.exists()).toBe(true);
    const mainText = main.text();
    expect(mainText).toContain("$9.00");
    expect(mainText).not.toContain("来源记录");
    expect(mainText).not.toContain("含写入分档估算");
    const cardLabel = w.find(".metric-label");
    expect(cardLabel.text()).not.toContain("来源记录");
    expect(cardLabel.text()).not.toContain("含未计价 token");
  });

  it("部分未定价 → 橙色金额、无常驻说明", () => {
    const w = mountCards(
      totals({
        cost_usd: 0.5,
        total_cost_usd: 0.5,
        cost_partially_unknown: true,
        unknown_pricing: true,
        unknown_tokens: { input: 300, output: 0, cache_write: 0, cache_read: 0 },
      }),
    );
    const text = w.text();
    expect(text).toContain("$0.50");
    expect(text).not.toContain("†");
    expect(w.find(".gcb-amount-main").classes()).toContain("gcb-warning");
    expect(text).not.toContain("含未计价 token");
  });

  it("混合构成 → 单一合并金额（未舍入相加后统一格式化）", () => {
    const w = mountCards(
      totals({
        cost_usd: 1.0,
        source_cost_usd: 7.25,
        total_cost_usd: 8.25,
        has_estimated_cost: true,
        cost_detail: "mixed",
      }),
    );
    const main = w.find(".metric-value-main .gcb-amount-main");
    expect(main.exists()).toBe(true);
    const mainText = main.text();
    expect(mainText).toContain("$8.25");
    expect(mainText).not.toContain("$7.25");
    expect(mainText).not.toContain("$1.00");
    // 悬浮详情里才有构成（$1.00 估算 / $7.25 来源）——挂在 body，不属常驻区。
    expect(w.text()).toContain("$7.25");
  });

  it("完全没有费用依据 → 「未知」警告色，不伪装成零", () => {
    const w = mountCards(
      totals({
        cost_usd: 0,
        total_cost_usd: null,
        unknown_pricing: true,
        unknown_tokens: { input: 100, output: 0, cache_write: 0, cache_read: 0 },
      }),
    );
    expect(w.text()).toContain("未知");
    expect(w.text()).not.toContain("未知†");
    expect(w.find(".gcb-amount-main").classes()).toContain("gcb-warning");
  });

  it("费用悬浮触发器可键盘聚焦（aria-label 含金额）", () => {
    const w = mountCards(totals({ total_cost_usd: 8.25, cost_usd: 8.25 }));
    const trigger = w.find(".gcb-trigger");
    expect(trigger.exists()).toBe(true);
    expect((trigger.element as HTMLElement).tagName).toBe("BUTTON");
    expect(trigger.attributes("aria-label")).toContain("$8.25");
  });

  it("partial 覆盖缺失 → 常驻区只有金额（部分说明进悬浮）", () => {
    const w = mountCards(
      totals({
        cost_usd: 0,
        source_cost_usd: 7.25,
        total_cost_usd: 7.25,
        cost_detail: "rollup_only",
        source_cost_state: "partial",
        cost_partially_unknown: true,
        unknown_pricing: true,
        unknown_tokens: { input: 500, output: 0, cache_write: 0, cache_read: 0 },
      }),
    );
    const text = w.text();
    expect(text).toContain("$7.25");
    expect(text).not.toContain("†");
    expect(text).not.toContain("部分来源记录");
    expect(text).not.toContain("其余汇总未提供费用");
  });
});

describe("总 token 换算单位按量级进位", () => {
  function tokenCard(w: ReturnType<typeof mountCards>) {
    const item = w.findAll(".metric-secondary .metric-item")[0];
    return {
      exact: item.find(".metric-value").text(),
      unit: item.find(".metric-unit").text(),
    };
  }

  it("中文：小于 1 万显示 tokens，1 万起用万，达到 1 亿自动进位为亿并保留上方精确总数", () => {
    const small = mountCards(
      totals({ tokens: { input: 600, output: 250, cache_write: 0, cache_read: 0 } }),
    );
    expect(tokenCard(small)).toEqual({ exact: "850", unit: "tokens" });

    const wan = mountCards(totals());
    expect(tokenCard(wan)).toEqual({ exact: "69,000", unit: "≈ 6.90 万 tokens" });

    const yi = mountCards(
      totals({
        tokens: {
          input: 1_000_000_000,
          output: 862_000_000,
          cache_write: 2_000_000_000,
          cache_read: 10_000_000_000,
        },
      }),
    );
    expect(tokenCard(yi)).toEqual({
      exact: "13,862,000,000",
      unit: "≈ 138.62 亿 tokens",
    });
  });

  it("英文：按 K / M / B 进位，且运行期切换语言会立即同步换算单位", async () => {
    setLocale("en-US");

    const small = mountCards(
      totals({ tokens: { input: 600, output: 250, cache_write: 0, cache_read: 0 } }),
    );
    expect(tokenCard(small)).toEqual({ exact: "850", unit: "tokens" });

    const k = mountCards(totals());
    expect(tokenCard(k)).toEqual({ exact: "69,000", unit: "≈ 69.00K tokens" });
    expect(tokenCard(k).unit).not.toContain("10k");

    const m = mountCards(
      totals({
        tokens: { input: 2_340_000, output: 1_000_000, cache_write: 0, cache_read: 9_000_000 },
      }),
    );
    expect(tokenCard(m)).toEqual({ exact: "12,340,000", unit: "≈ 12.34M tokens" });

    const b = mountCards(
      totals({
        tokens: {
          input: 1_000_000_000,
          output: 862_000_000,
          cache_write: 2_000_000_000,
          cache_read: 10_000_000_000,
        },
      }),
    );
    expect(tokenCard(b)).toEqual({ exact: "13,862,000,000", unit: "≈ 13.86B tokens" });

    // 运行期切回中文，同一挂载实例立即切回「亿」且精确总数不变
    setLocale("zh-CN");
    await flushPromises();
    expect(tokenCard(b)).toEqual({ exact: "13,862,000,000", unit: "≈ 138.62 亿 tokens" });
  });

  it("万亿 / T 及超大任意精度聚合总数：精确千分位、科学计数法、命中率与比例条全部无损", async () => {
    // 1) 万亿 / T
    const trillion = mountCards(
      totals({
        tokens: {
          input: 1_000_000_000_000,
          output: 862_000_000_000,
          cache_write: 2_000_000_000_000,
          cache_read: 10_000_000_000_000,
        },
      }),
    );
    expect(tokenCard(trillion)).toEqual({
      exact: "13,862,000,000,000",
      unit: "≈ 13.86 万亿 tokens",
    });
    setLocale("en-US");
    await flushPromises();
    expect(tokenCard(trillion)).toEqual({
      exact: "13,862,000,000,000",
      unit: "≈ 13.86T tokens",
    });

    // 2) 10¹⁵ 英文转上标科学计数法，中文仍显示 1000.00 万亿
    const quadrillion = mountCards(
      totals({
        tokens: {
          input: "1000000000000000",
          output: 0,
          cache_write: 0,
          cache_read: 0,
        },
      }),
    );
    expect(tokenCard(quadrillion)).toEqual({
      exact: "1,000,000,000,000,000",
      unit: "≈ 1.00 × 10¹⁵ tokens",
    });
    setLocale("zh-CN");
    await flushPromises();
    expect(tokenCard(quadrillion)).toEqual({
      exact: "1,000,000,000,000,000",
      unit: "≈ 1000.00 万亿 tokens",
    });

    // 3) 超出 u64::MAX 的任意长十进制字符串（来自后端 BigUint 序列化）
    const huge = mountCards(
      totals({
        tokens: {
          input: "100000000000000000000",
          output: "23456789012345678901",
          cache_write: "0",
          cache_read: "100000000000000000000",
        },
      }),
    );
    expect(tokenCard(huge)).toEqual({
      exact: "223,456,789,012,345,678,901",
      unit: "≈ 2.23 × 10²⁰ tokens",
    });
    // 命中率：10^20 / (10^20 + 10^20) = 50.0%
    expect(huge.text()).toContain("50.0%");
    const segs = huge.findAll(".bar-segment");
    expect(segs.length).toBe(4);
    expect(segs[0].attributes("style")).toContain("44.75%");
    expect(segs[3].attributes("style")).toContain("44.75%");
  });

  it("20 位与 30 位大数自动降档字号、按逗号断行保留无空白完整文本，并支持一键复制精确整数", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });

    // 10 位常规数：保持 26px 默认档
    const normal = mountCards(
      totals({ tokens: { input: 13_861_826_426, output: 0, cache_write: 0, cache_read: 0 } }),
    );
    expect(normal.find(".metric-value-tokens").classes()).not.toContain("metric-value-long");
    expect(normal.find(".metric-value-tokens").classes()).not.toContain("metric-value-xlong");

    // 20 位大数（26 字符）：切换 metric-value-long 档，保留完整 title
    const d20 = mountCards(
      totals({
        tokens: { input: "18446744073709551625", output: 0, cache_write: 0, cache_read: 0 },
      }),
    );
    const val20 = d20.find(".metric-value-tokens");
    expect(val20.classes()).toContain("metric-value-long");
    expect(val20.text()).toBe("18,446,744,073,709,551,625");
    expect(val20.attributes("title")).toBe("18,446,744,073,709,551,625");

    // 30 位大数（39 字符）：切换 metric-value-xlong 档，逗号后含 wbr 断行点且文本无多余空白
    const raw30 = "123456789012345678901234567890";
    const d30 = mountCards(
      totals({ tokens: { input: raw30, output: 0, cache_write: 0, cache_read: 0 } }),
    );
    const val30 = d30.find(".metric-value-tokens");
    expect(val30.classes()).toContain("metric-value-xlong");
    expect(val30.text()).toBe("123,456,789,012,345,678,901,234,567,890");
    expect(val30.findAll("wbr").length).toBe(9);
    expect(val30.findAll(".num-chunk").length).toBe(10);

    // 点击复制按钮：写入无逗号完整精确整数并展示「已复制」反馈
    const copyBtn = d30.find(".copy-total-btn");
    expect(copyBtn.exists()).toBe(true);
    expect(copyBtn.attributes("aria-label")).toBe("复制精确数值");
    await copyBtn.trigger("click");
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith(raw30);
    expect(d30.find(".copy-feedback").text()).toBe("已复制");

    // 切到英文：无障碍标签与反馈同步切英文
    setLocale("en-US");
    await flushPromises();
    expect(copyBtn.attributes("aria-label")).toBe("Copied");
    expect(d30.find(".copy-feedback").text()).toBe("Copied");
    d30.unmount();
  });
});
