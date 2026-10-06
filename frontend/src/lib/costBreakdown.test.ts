// 设计系统 Task 6：费用明细行模型——事实→公式→结果→来源、未知与缺价语义。
import { describe, expect, it } from "vitest";
import { formatCostBreakdownRows, formatCostBreakdownText } from "./costBreakdown";
import type { EventCostBreakdown } from "../types";

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
    expect(t).toContain("输入 272,001 × $8.00/M = $2.18");
    expect(t).toContain("输出 1,000 × $30.00/M = $0.03");
    expect(t).toContain("缓存写 0 × $10.00/M = $0.00");
    expect(t).toContain("缓存读 5,000 token，缺少单价");
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
    expect(t).toContain("输入 272,001 + 缓存写 0 + 缓存读 5,000 = 277,001");
    expect(t).toContain("命中档位 >272K");
    expect(t).toContain("时间档 peak（时区 Asia/Shanghai）");
  });

  it("来源区区分保守估算候选与服务器实际路由", () => {
    const t = text(bd());
    expect(t).toContain("计价来源 外置价格表");
    expect(t).toContain("完整匹配");
    expect(t).toContain("候选 2 条，按本请求条件取最高费用（保守估算，非服务器实际路由）");
  });

  it("未知标记只出现在缺价相关行（unknown 行可被高亮渲染）", () => {
    const r = rows(bd()).filter((x) => x.unknown);
    expect(r.map((x) => x.label)).toEqual(["缓存读", "未计价"]);
  });
});
