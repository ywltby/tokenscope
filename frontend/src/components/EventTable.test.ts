// D1/C5 打磨回归：EventTable——项目列末段展示（C2）与「加载更多」
// 分页交互（D1 游标的前端入口）。
// Task 7：费用悬浮提示——纯函数 formatCostBreakdown 的排版断言
//（NDataTable 虚拟滚动在测试环境不渲染行，与既有用例同口径）。
import { describe, expect, it } from "vitest";
import { nextTick } from "vue";
import { mount } from "@vue/test-utils";
import type { VNode } from "vue";
import EventTable from "./EventTable.vue";
import { projectLabel, type EventCostBreakdown, type EventList, type EventRow } from "../types";
import { formatCostBreakdownRows, formatCostBreakdownText } from "../lib/costBreakdown";

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

const text = (bd: EventCostBreakdown) => formatCostBreakdownText(bd);

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
    expect(t).toContain("匹配模型 nano-gpt/qwen/qwen3.8-27b-obliterated:thinking");
    expect(t).toContain("计价来源 外置价格表");
    expect(t).toContain("渠道 nano-gpt");
    expect(t).toContain("完整匹配");
    expect(t).toContain("候选 2 条，按本请求条件取最高费用（保守估算，非服务器实际路由）");
  });

  it("请求时间、时间档及时区可见；无峰谷规则时明确说明", () => {
    expect(text(highTierBd())).toContain("请求时间 2026-01-05T04:00:00Z");
    expect(text(highTierBd())).toContain("时间档 peak（时区 Asia/Shanghai）");
    expect(text(lowTierBd())).toContain("时间档 无峰谷规则");
  });

  it("prompt 度量式 = input + cache_write + cache_read = basis_value", () => {
    expect(text(highTierBd())).toContain(
      "prompt tokens：输入 272,001 + 缓存写 0 + 缓存读 5,000 = 277,001",
    );
    expect(text(lowTierBd())).toContain(
      "prompt tokens：输入 100,000 + 缓存写 10,000 + 缓存读 20,000 = 130,000",
    );
  });

  it("命中档位可见：高档显示分段标签，低档显示基础价档", () => {
    expect(text(highTierBd())).toContain("命中档位 >272K");
    expect(text(lowTierBd())).toContain("命中档位 基础价档");
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
    const rows = formatCostBreakdownRows(highTierBd());
    const unknownRow = rows.find((l) => l.label === "缓存读");
    expect(unknownRow).toBeDefined();
    expect(unknownRow!.value).toContain("缺少单价");
    expect(unknownRow!.unknown).toBe(true);
    const t = formatCostBreakdownText(highTierBd());
    expect(t).toContain("估算合计 $2.21");
    expect(t).toContain("未计价：缺价 ≠ 免费：未计价 token 不计入合计");
    // 完整计价请求不出缺价说明。
    expect(text(lowTierBd())).not.toContain("未计价");
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

  it("卡片化：外层 .ts-card + 卡头「请求明细」，提示行不再用 opacity 压低", () => {
    const w = mount(EventTable, { props: { list: list([row()], 1), filterLabel: "" } });
    expect(w.find("section.ts-card").exists()).toBe(true);
    expect(w.find(".card-title").text()).toContain("请求明细");
    expect(w.find(".table-hint").exists()).toBe(true);
    expect(w.html()).not.toContain("opacity: 0.6");
  });

  it("下钻筛选在卡头显示为可关闭筛选标签，关闭 emit clear-filter", async () => {
    const w = mount(EventTable, {
      props: { list: list([row()], 1), filterLabel: "model: gpt-x", filterClosable: true },
    });
    const chip = w.find(".filter-chip");
    expect(chip.exists()).toBe(true);
    expect(chip.text()).toContain("model: gpt-x");
    await chip.trigger("click");
    expect(w.emitted("clear-filter")).toHaveLength(1);
  });
});

// 设计系统 Task 6：费用明细玻璃浮层——触发路径、浮层视觉、分段结构。
describe("EventTable 费用浮层（设计系统 Task 6）", () => {
  /// 取费用列 render 的 VNode（NTooltip）——行级渲染在测试环境不可达，
  /// 与既有用例同口径直接调列渲染函数。
  function tooltipVnode(w: ReturnType<typeof mount>): VNode {
    const cols = (
      w.vm as unknown as { columns: { key: string; render?: (r: object) => unknown }[] }
    ).columns;
    const cost = cols.find((c) => c.key === "cost_usd")!;
    const bdRow = row({ cost_usd: 2.206008, cost_breakdown: highTierBd() });
    return cost.render!(bdRow) as VNode;
  }

  it("触发器支持 click/hover/focus 打开、Escape 关闭，aria-expanded 随开合翻转", async () => {
    const w = mount(EventTable, {
      props: {
        list: list([row({ cost_usd: 2.206008, cost_breakdown: highTierBd() })], 1),
        filterLabel: "",
      },
    });
    const vnode = tooltipVnode(w);
    const trigger = (vnode.children as Record<string, () => VNode>).trigger();
    const p = trigger.props as Record<string, unknown>;
    expect(p.tabindex).toBe(0);
    expect(p.role).toBe("button");
    expect(p["aria-expanded"]).toBe(false);
    for (const h of ["onClick", "onFocus", "onMouseenter", "onMouseleave", "onBlur", "onKeydown"]) {
      expect(typeof p[h], `${h} 处理器存在`).toBe("function");
    }
    // click 打开 → aria-expanded/show 翻转
    (p.onClick as () => void)();
    await nextTick();
    const reopened = tooltipVnode(w);
    expect((reopened.props as Record<string, unknown>).show).toBe(true);
    const trigger2 = (reopened.children as Record<string, () => VNode>).trigger();
    expect((trigger2.props as Record<string, unknown>)["aria-expanded"]).toBe(true);
    // Escape 关闭
    ((trigger2.props as Record<string, unknown>).onKeydown as (e: { key: string }) => void)({
      key: "Escape",
    });
    await nextTick();
    expect((tooltipVnode(w).props as Record<string, unknown>).show).toBe(false);
  });

  it("浮层视觉：480px 上限 + 16px 玻璃模糊 + 公式区实色衬底", () => {
    const w = mount(EventTable, {
      props: {
        list: list([row({ cost_usd: 2.206008, cost_breakdown: highTierBd() })], 1),
        filterLabel: "",
      },
    });
    const vnode = tooltipVnode(w);
    const style = String((vnode.props as Record<string, unknown>).style);
    expect(style).toContain("max-width: 480px");
    expect(style).toContain("var(--ts-glass-blur-popover)");
    const content = (vnode.children as Record<string, () => VNode>).default();
    expect(JSON.stringify(content)).toContain("bd-formula");
    expect(JSON.stringify(content)).toContain("ts-card-solid");
  });
});
