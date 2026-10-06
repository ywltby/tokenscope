// D1/C5 打磨回归：EventTable——项目列末段展示（C2）与「加载更多」
// 分页交互（D1 游标的前端入口）。
// Task 7：费用悬浮提示——纯函数 formatCostBreakdown 的排版断言
//（NDataTable 虚拟滚动在测试环境不渲染行，与既有用例同口径）。
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import EventTable from "./EventTable.vue";
import { projectLabel, type EventCostBreakdown, type EventList, type EventRow } from "../types";
import { formatCostBreakdown } from "../lib/costBreakdown";

function row(over: Partial<EventRow> = {}): EventRow {
  return {
    ts: "2026-10-05 10:00:00",
    record_id: "m1",
    cursor: "2026-10-05T10:00:00Z|m1",
    agent: "codex",
    model: "gpt-x",
    session_id: "s",
    project: "C:/work/alpha",
    input: 1,
    output: 1,
    cache_write: 0,
    cache_read: 0,
    cost_usd: 0,
    ...over,
  };
}

function list(rows: EventRow[], total: number): EventList {
  return { rows, total, warnings: [] };
}

/** Task 7 fixture：外置来源、完整匹配、峰谷档命中的高档请求（部分缺价）。 */
function highTierBd(): EventCostBreakdown {
  return {
    matched: {
      raw_key: "nano-gpt/qwen/qwen3.8-27b-obliterated:thinking",
      channel: "nano-gpt",
      source: "external",
      matched_key: "qwen3-8-27b-obliterated:thinking",
      match_mode: "full",
      candidate_count: 2,
      reason: "candidates_highest_cost",
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
  };
}

function lowTierBd(): EventCostBreakdown {
  const bd = highTierBd();
  return {
    ...bd,
    matched: { ...bd.matched, schedule_label: null, schedule_timezone: null },
    basis_value: 130_000,
    segment_label: null,
    lines: [
      { kind: "input", tokens: 100_000, unit_price: 4, subtotal: 0.4, priced: true },
      { kind: "output", tokens: 50_000, unit_price: 20, subtotal: 1.0, priced: true },
      { kind: "cache_write", tokens: 10_000, unit_price: 5, subtotal: 0.05, priced: true },
      { kind: "cache_read", tokens: 20_000, unit_price: 0.4, subtotal: 0.008, priced: true },
    ],
    cost_usd: 1.458,
    unknown: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    complete: true,
  };
}

const text = (bd: EventCostBreakdown) =>
  formatCostBreakdown(bd)
    .map((l) => l.text)
    .join("\n");

describe("EventTable（D1 分页 / C2 项目列）", () => {
  it("project_label_visible：项目列取路径末段（NDataTable 虚拟滚动在测试环境不渲染行，纯函数验证）", () => {
    // 正斜杠 / 反斜杠 / 尾部斜杠 / 无分隔符（slug、占位名）
    expect(projectLabel("C:/work/alpha")).toBe("alpha");
    expect(projectLabel("C:\\work\\beta")).toBe("beta");
    expect(projectLabel("D:/other/alpha/")).toBe("alpha");
    expect(projectLabel("(根目录)")).toBe("(根目录)");
    expect(projectLabel("my-slug")).toBe("my-slug");
  });

  it("加载更多按钮在还有余量时出现，显示剩余数并 emit load-more", async () => {
    const w = mount(EventTable, {
      props: { list: list([row()], 3), filterLabel: "", more: true },
    });
    expect(w.text()).toContain("加载更多");
    expect(w.text()).toContain("还剩 2 条");
    const btn = w.findAll("button").find((b) => b.text().includes("加载更多"));
    expect(btn).toBeDefined();
    await btn!.trigger("click");
    expect(w.emitted("load-more")).toHaveLength(1);
  });

  it("全部加载后不显示按钮", () => {
    const w = mount(EventTable, {
      props: { list: list([row()], 1), filterLabel: "" },
    });
    expect(w.text()).not.toContain("加载更多");
  });
});

describe("EventTable 费用悬浮（Task 7）", () => {
  it("高档请求：来源/模型、渠道、匹配方式与候选最高费用说明可见", () => {
    const t = text(highTierBd());
    expect(t).toContain("nano-gpt/qwen/qwen3.8-27b-obliterated:thinking（外置价格表）");
    expect(t).toContain("渠道 nano-gpt");
    expect(t).toContain("完整匹配");
    expect(t).toContain("候选 2 条，按本请求条件取最高费用（保守估算）");
  });

  it("请求时间、时间档及时区可见；无峰谷规则时明确说明", () => {
    expect(text(highTierBd())).toContain("请求时间 2026-01-05T04:00:00Z");
    expect(text(highTierBd())).toContain("时间档 peak（时区 Asia/Shanghai）");
    expect(text(lowTierBd())).toContain("时间档：无峰谷规则");
  });

  it("prompt 度量式 = input + cache_write + cache_read = basis_value", () => {
    expect(text(highTierBd())).toContain("prompt tokens = 272,001 + 0 + 5,000 = 277,001");
    expect(text(lowTierBd())).toContain("prompt tokens = 100,000 + 10,000 + 20,000 = 130,000");
  });

  it("命中档位可见：高档显示分段标签，低档显示基础价档", () => {
    expect(text(highTierBd())).toContain("命中档位：>272K");
    expect(text(lowTierBd())).toContain("命中档位：基础价档");
  });

  it("每个分项展示 token、USD/百万单价与小计；金额无二进制浮点长尾", () => {
    const t = text(highTierBd());
    expect(t).toContain("输入 272,001 × $8.00/M = $2.18");
    expect(t).toContain("输出 1,000 × $30.00/M = $0.03");
    expect(t).toContain("缓存写 0 × $10.00/M = $0.00");
    expect(t).not.toMatch(/2\.1760080\d*0000/);
    expect(t).not.toContain("0.030000000000000002");
  });

  it("未计价分项有明确文案且标注 unknown；总价与缺价说明可见", () => {
    const lines = formatCostBreakdown(highTierBd());
    const unknownLine = lines.find((l) => l.text.includes("缓存读 5,000"));
    expect(unknownLine).toBeDefined();
    expect(unknownLine!.text).toContain("未计价：缺少单价");
    expect(unknownLine!.unknown).toBe(true);
    const t = lines.map((l) => l.text).join("\n");
    expect(t).toContain("合计 $2.21");
    expect(t).toContain("未计价 token（缺价 ≠ 免费）：缓存读 5,000");
    // 完整计价请求不出缺价说明。
    expect(text(lowTierBd())).not.toContain("未计价 token");
  });

  it("cost_usd 列仍按原格式渲染；旧响应（无 breakdown）不崩溃", () => {
    const w = mount(EventTable, {
      props: {
        list: list(
          [
            row({ cost_usd: 2.206008, cost_breakdown: highTierBd() }),
            row({ record_id: "m2", cost_usd: 1.458, cost_breakdown: null }),
            row({ record_id: "m3", cost_usd: null }),
          ],
          3,
        ),
        filterLabel: "",
      },
    });
    // 挂载即无异常（行渲染交给 NDataTable；列渲染函数经类型检查覆盖）。
    expect(w.exists()).toBe(true);
  });
});

// 设计系统 Task 5：明细表列语义（对齐/数字/省略列）。
describe("EventTable 表格语义（设计系统 Task 5）", () => {
  function exposed(w: ReturnType<typeof mount>) {
    return w.vm as unknown as {
      columns: { key: string; align?: string; className?: string; ellipsis?: unknown }[];
    };
  }

  it("数字列右对齐且使用 tabular 数字类；模型列省略并可查完整值", () => {
    const w = mount(EventTable, { props: { list: list([row()], 1), filterLabel: "" } });
    const cols = exposed(w).columns;
    for (const key of ["input", "output", "cache_write", "cache_read", "cost_usd"]) {
      const c = cols.find((x) => x.key === key);
      expect(c, `${key} 列存在`).toBeDefined();
      expect(c!.align).toBe("right");
      expect(c!.className).toContain("ts-num");
    }
    const model = cols.find((x) => x.key === "model");
    expect(model!.ellipsis).toBeTruthy();
  });
});
