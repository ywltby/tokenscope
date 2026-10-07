// 设计系统 Task 6：费用明细行模型——事实→公式→结果→来源、未知与缺价语义。
import { describe, expect, it } from "vitest";
import { formatCostBreakdownRows, formatCostBreakdownText } from "./costBreakdown";
import { fmtPriceOrUnknown, formatTieredPricing } from "./tieredPrice";
import type { EventCostBreakdown, PricingEntry } from "../types";

function bd(over: Partial<EventCostBreakdown> = {}): EventCostBreakdown {
  return {
    matched: {
      raw_key: "nano-gpt/qwen/qwen3.8-27b-obliterated:thinking",
      channel: "nano-gpt",
      source: "external",
      matched_key: "qwen3-8-27b-obliterated:thinking",
      match_mode: "full",
      candidate_count: 2,
      reason: "highest_complete_cost",
      schedule_label: "peak",
      schedule_timezone: "Asia/Shanghai",
      request_at: "2026-01-05T04:00:00Z",
    },
    basis: "prompt_tokens",
    basis_value: 277_001,
    segment_label: ">272K",
    lines: [
      { kind: "input", tokens: 272_001, unit_price: 8, subtotal: 2.176008, priced: true },
      { kind: "output", tokens: 1_000, unit_price: 30, subtotal: 0.03, priced: true },
      { kind: "cache_write", tokens: 0, unit_price: 10, subtotal: 0, priced: true },
      { kind: "cache_read", tokens: 5_000, unit_price: null, subtotal: 0, priced: false },
    ],
    cost_usd: 2.206008,
    unknown: { input: 0, output: 0, cache_write: 0, cache_read: 5_000 },
    complete: false,
    ...over,
  };
}

/** same_as_input fixture：cache_read 显式沿用输入价（单价已由后端解析）。 */
function sameAsInputBd(): EventCostBreakdown {
  const base = bd();
  return {
    ...base,
    lines: base.lines.map((l) =>
      l.kind === "cache_read"
        ? { ...l, unit_price: 8, subtotal: 0.04, priced: true, rate_kind: "same_as_input" as const }
        : { ...l, rate_kind: "fixed" as const },
    ),
    cost_usd: 2.246008,
    unknown: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    complete: true,
  };
}

/** 排除诊断 fixture：完整候选胜出但有 2 个不完整候选被排除。 */
function excludedBd(): EventCostBreakdown {
  const base = bd();
  return {
    ...base,
    matched: {
      ...base.matched,
      reason: "highest_complete_cost",
      complete_candidate_count: 1,
      incomplete_candidate_count: 2,
      incomplete_candidates_excluded: 2,
    },
    excluded_candidate_warning:
      "有 2 个候选因缺少价格未参与主估算（zenmux/gpt-5.4、peak/gpt-5.4）；若其缺失分项有价，实际最高费用可能更高",
  };
}

const text = (b: EventCostBreakdown) => formatCostBreakdownText(b);
const rows = (b: EventCostBreakdown) => formatCostBreakdownRows(b);

describe("costBreakdown 行模型（设计系统 Task 6）", () => {
  it("按 事实→公式→结果→来源 分组，组间 divider", () => {
    const r = rows(bd());
    const firstLabels = ["匹配模型", "渠道", "请求时间", "prompt tokens", "命中档位", "时间档"];
    expect(r.slice(0, 6).map((x) => x.label)).toEqual(firstLabels);
    expect(r[6].divider).toBe(true);
    const dividerIdx = r.map((x, i) => (x.divider ? i : -1)).filter((i) => i >= 0);
    // 四组之间恰有三条分隔
    expect(dividerIdx).toEqual([6, 11, 14]);
    expect(r.slice(15).map((x) => x.label)).toEqual(["计价来源", "匹配方式", "候选选择"]);
  });

  it("公式行展示 token × 单价/M = 小计；未计价行显式说明", () => {
    const t = text(bd());
    expect(t).toContain("输入 272,001 × $8/M = $2.18");
    expect(t).toContain("输出 1,000 × $30/M = $0.0300");
    expect(t).toContain("缓存写 0 × $10/M = $0.00");
    expect(t).toContain("缓存命中 5,000 token，缺少单价");
  });

  it("结果行不吞掉极小非零金额", () => {
    const b = bd({ cost_usd: 0.0000126 });
    b.lines = b.lines.map((l) => ({ ...l, subtotal: l.priced ? 0.0000126 : 0 }));
    const t = text(b);
    expect(t).toContain("$0.000013");
    expect(t).not.toMatch(/\$0\.00(?!\d)/);
  });

  it("完全未知显示无法估算，不显示 $0.00", () => {
    const b = bd({
      cost_usd: 0,
      complete: false,
      lines: [
        { kind: "input", tokens: 100, unit_price: null, subtotal: 0, priced: false },
        { kind: "output", tokens: 0, unit_price: null, subtotal: 0, priced: false },
        { kind: "cache_write", tokens: 0, unit_price: null, subtotal: 0, priced: false },
        { kind: "cache_read", tokens: 0, unit_price: null, subtotal: 0, priced: false },
      ],
    });
    const r = rows(b).find((x) => x.total);
    expect(r!.value).toContain("无法估算");
    expect(text(b)).not.toContain("$0.00");
  });

  it("缺失字段显示暂无数据（不猜测渠道/时间）", () => {
    const b = bd();
    b.matched = {
      ...b.matched,
      channel: null,
      request_at: null,
      schedule_label: null,
      schedule_timezone: null,
    };
    const t = text(b);
    expect(t).toContain("渠道 暂无数据");
    expect(t).toContain("请求时间 暂无数据");
    expect(t).toContain("时间档 无峰谷规则");
  });

  it("事实区含 prompt 度量式、档位与峰谷条件", () => {
    const t = text(bd());
    expect(t).toContain("输入 272,001 + 缓存写 0 + 缓存命中 5,000 = 277,001");
    expect(t).toContain("命中档位 >272K");
    expect(t).toContain("时间档 peak（时区 Asia/Shanghai）");
  });

  it("来源区区分保守估算候选与服务器实际路由", () => {
    const t = text(bd());
    expect(t).toContain("计价来源 外置价格表");
    expect(t).toContain("完整匹配");
    expect(t).toContain("候选 2 条（完整 ?/不完整 ?），在完整候选中取最高费用");
  });

  it("未知标记只出现在缺价相关行（unknown 行可被高亮渲染）", () => {
    const r = rows(bd()).filter((x) => x.unknown);
    expect(r.map((x) => x.label)).toEqual(["缓存命中", "未计价"]);
  });
});

describe("costBreakdown 三态与排除诊断（缓存读取定价解析计划 Task 5）", () => {
  it("same_as_input 公式行标注「输入价」且单价为解析后的实际数值", () => {
    const t = text(sameAsInputBd());
    expect(t).toContain("缓存命中 5,000 × 输入价 $8/M = $0.0400");
    expect(t).not.toContain("缺少单价");
  });

  it("显式 0 仍是普通固定价 $0，不显示未知标记", () => {
    const b = bd();
    const rows = formatCostBreakdownRows({
      ...b,
      lines: b.lines.map((l) =>
        l.kind === "cache_read"
          ? { ...l, unit_price: 0, subtotal: 0, priced: true, rate_kind: "fixed" as const }
          : { ...l, rate_kind: "fixed" as const },
      ),
      unknown: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
      complete: true,
    });
    const cr = rows.find((r) => r.label === "缓存命中");
    expect(cr!.value).toContain("$0.00/M");
    expect(cr!.unknown).toBeUndefined();
  });

  it("完整候选胜出时来源区显示两阶段选择与排除提示", () => {
    const t = text(excludedBd());
    expect(t).toContain("完整 1/不完整 2");
    expect(t).toContain("估算范围：有 2 个候选因缺少价格未参与主估算");
    expect(t).toContain("zenmux/gpt-5.4");
  });

  it("部分候选回退时说明「均不完整」", () => {
    const b = bd();
    const t = text({
      ...b,
      matched: { ...b.matched, reason: "highest_partial_cost" },
    });
    expect(t).toContain("均不完整");
  });
});

describe("costBreakdown 溢出分项（修复后复核 F04）", () => {
  it("溢出行显示金额超出可表示范围，不得显示为免费或固定单价", () => {
    const b = bd();
    const r = formatCostBreakdownRows({
      ...b,
      lines: b.lines.map((l) =>
        l.kind === "input"
          ? {
              ...l,
              unit_price: 1e308,
              subtotal: 0,
              priced: false,
              rate_kind: "fixed" as const,
              overflow: true,
            }
          : l,
      ),
      cost_usd: 0.03,
      complete: false,
      unknown: { input: 272_001, output: 0, cache_write: 0, cache_read: 0 },
    });
    const input = r.find((x) => x.label === "输入");
    expect(input!.value).toContain("金额超出可表示范围");
    expect(input!.value).not.toContain("$");
    expect(input!.unknown).toBe(true);
    const t = text({
      ...b,
      lines: b.lines.map((l) =>
        l.kind === "input"
          ? { ...l, unit_price: 1e308, subtotal: 0, priced: false, overflow: true }
          : l,
      ),
      cost_usd: 0.03,
      complete: false,
    });
    expect(t).toContain("输入 272,001 token，金额超出可表示范围");
    expect(t).not.toContain("× $1.00");
  });
});

// ── RC07：单价精度在真实设置调用点与费用公式中保留 ──
// 复核现象：`formatMoney(1.234567, "unit")` 返回 `$1.23`（scenario 被丢弃
// 且 ≥1 固定两位），设置页单价与公式费率都丢精度。
describe("单价精度贯通设置与公式（RC07）", () => {
  it("unit_precision_survives_settings_and_breakdown", () => {
    // (a) 设置页单价格式化入口（fmtPriceOrUnknown → formatMoney(unit)）
    expect(fmtPriceOrUnknown(1.234567)).toBe("$1.234567");
    expect(fmtPriceOrUnknown(12.3456789)).toBe("$12.3456789");
    expect(fmtPriceOrUnknown(0)).toBe("$0.00");
    expect(fmtPriceOrUnknown("same_as_input")).toBe("同输入价");
    expect(fmtPriceOrUnknown(null)).toBe("未知");
    // 分段/峰谷嵌套视图同样保留精度（单位是 USD / 1M token）
    const nested: PricingEntry = {
      prefix: "precise",
      input: 1.234567,
      output: 12.3456789,
      cache_write: null,
      cache_read: "same_as_input",
      source: "外置",
      has_tiered_pricing: true,
      basis: "prompt_tokens",
      segments: [
        {
          label: ">272K",
          min_tokens: 272001,
          max_tokens: null,
          prices: { input: 1.234567, output: 12.3456789, cache_write: null, cache_read: null },
        },
      ],
    };
    const lines = formatTieredPricing(nested);
    expect(lines.some((l) => l.includes("输入 $1.234567") && l.includes("输出 $12.3456789"))).toBe(
      true,
    );

    // (b) 费用公式：费率展示完整精度；金额结果仍来自 DTO（不用显示串复算）。
    const precise = bd({
      lines: [
        {
          kind: "input",
          tokens: 1_000,
          unit_price: 1.234567,
          subtotal: 0.001234567,
          priced: true,
          rate_kind: "fixed",
        },
        { kind: "output", tokens: 0, unit_price: null, subtotal: 0, priced: false },
        { kind: "cache_write", tokens: 0, unit_price: null, subtotal: 0, priced: false },
        { kind: "cache_read", tokens: 0, unit_price: null, subtotal: 0, priced: false },
      ],
      cost_usd: 0.001234567,
      unknown: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
      complete: true,
    });
    const t = formatCostBreakdownText(precise);
    // 费率完整（$1.234567）；金额仍走 request 策略（<1 四位小数），
    // 关键是**单价**不被舍成 $1.23。
    expect(t).toContain("输入 1,000 × $1.234567/M = $0.0012");
    expect(t).toContain("估算合计 $0.0012");
    expect(t).not.toContain("$1.23/M");
  });
});
