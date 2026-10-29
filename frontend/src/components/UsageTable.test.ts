// 计划 C5：cost_unknown_partial_complete——费用未知的追溯完整：
// 存在未计价内容时出现"未知†"列并按行显示未计价 token；全部已计价时不出现。
// （† 语义 = 估算费用仅含已计价部分：无价格模型 + 部分计价两种来源。）
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import UsageTable from "./UsageTable.vue";
// Vite ?raw：直接取 SFC 源文本做样式契约断言（项目无 @types/node）
import usageTableSource from "./UsageTable.vue?raw";
import type { Group, SummaryReport } from "../types";

function group(key: string, unknownTokens?: Partial<Group["unknown_tokens"]>): Group {
  return {
    key,
    requests: 1,
    tokens: { input: 10, output: 5, cache_write: 0, cache_read: 0 },
    cost_usd: 0.01,
    unknown_pricing: unknownTokens !== undefined,
    unknown_tokens: {
      input: unknownTokens?.input ?? 0,
      output: unknownTokens?.output ?? 0,
      cache_write: unknownTokens?.cache_write ?? 0,
      cache_read: unknownTokens?.cache_read ?? 0,
    },
  };
}

function report(groups: Group[], totalsUnknown: boolean): SummaryReport {
  return {
    query_id: "q",
    pricing_revision: "rev",
    timezone: "Asia/Shanghai",
    generated_at: "t",
    sources: [],
    by: "model",
    groups,
    totals: {
      ...group("合计"),
      unknown_pricing: totalsUnknown,
    },
    warnings: [],
  };
}

describe("UsageTable 费用可追溯（C5）", () => {
  it("cost_unknown_partial_complete：有未计价内容时出现未知†列并显示数量", () => {
    const w = mount(UsageTable, {
      props: {
        report: report([group("m-free", { input: 100, output: 50 }), group("m-priced")], true),
      },
    });
    expect(w.text()).toContain("未知†");
    expect(w.text()).toContain("150");
    // 脚注覆盖两种来源（无价格模型 + 部分计价缺分项）
    expect(w.text()).toContain("估算");
    expect(w.text()).toContain("缺分项价");
  });

  it("全部已计价时不出现未知†列", () => {
    const w = mount(UsageTable, { props: { report: report([group("m-priced")], false) } });
    expect(w.text()).not.toContain("未知†");
  });

  it("unknown_only_row_shows_unknown_not_zero：完全未计价行显示未知†而非 $0.00†（G-P3-1）", () => {
    // 与 SummaryCards 英雄栏同口径：缺价 ≠ 免费。
    const g = group("m-free", { input: 10 });
    g.cost_usd = 0;
    const w = mount(UsageTable, { props: { report: report([g], true) } });
    expect(w.text()).toContain("未知†");
    expect(w.text()).not.toContain("$0.00†");
  });
});

// 设计系统 Task 5：表格语义——列对齐/tabular 数字/估算表头/键盘下钻。
describe("UsageTable 表格语义（设计系统 Task 5）", () => {
  function exposed(w: ReturnType<typeof mount>) {
    return w.vm as unknown as {
      columns: {
        key: string;
        align?: string;
        className?: string;
        title?: string;
        render?: (row: object) => unknown;
      }[];
      rowProps: (g: Group) => Record<string, unknown>;
    };
  }

  it("数字列居中且使用 tabular 数字类", () => {
    const w = mount(UsageTable, { props: { report: report([group("m")], false) } });
    const cols = exposed(w).columns;
    for (const key of ["requests", "tokens.input", "tokens.output", "total", "cost_usd"]) {
      const c = cols.find((x) => x.key === key);
      expect(c, `${key} 列存在`).toBeDefined();
      expect(c!.align).toBe("center");
      expect(c!.className).toContain("ts-num");
    }
  });

  it("费用列表头简化为费用；未知标记用警告色而非 opacity", () => {
    const w = mount(UsageTable, { props: { report: report([group("m")], true) } });
    const cols = exposed(w).columns;
    const cost = cols.find((x) => x.key === "cost_usd");
    expect(String(cost!.title)).toBe("费用");
    const unknownCol = cols.find((x) => x.key === "unknown_tokens");
    const vnode = unknownCol!.render!(group("x", { input: 100 })) as { props?: { style?: string } };
    const style = JSON.stringify(vnode?.props?.style ?? "");
    expect(style).not.toContain("opacity");
    expect(style).toContain("--ts-warning");
  });

  it("聚合行可通过 Enter/Space 键盘下钻（不只靠点击）", () => {
    const w = mount(UsageTable, { props: { report: report([group("m1")], false) } });
    const rp = exposed(w).rowProps(group("m1"));
    expect(rp.tabindex).toBe(0);
    expect(typeof rp.onkeydown).toBe("function");
    (rp.onkeydown as (e: { key: string; preventDefault: () => void }) => void)({
      key: "Enter",
      preventDefault: () => {},
    });
    expect(w.emitted("row-click")).toBeTruthy();
  });

  it("卡片化：外层 .ts-card，行有可见焦点路径（ts-focusable）", () => {
    const w = mount(UsageTable, { props: { report: report([group("m1")], false) } });
    expect(w.find("section.ts-card").exists()).toBe(true);
    const rp = exposed(w).rowProps(group("m1"));
    expect(String(rp.class)).toContain("ts-focusable");
  });

  it("可点击行右侧带 › 指示，合计行不带", () => {
    const w = mount(UsageTable, { props: { report: report([group("m1")], false) } });
    const cols = exposed(w).columns;
    const first = cols[0];
    const normal = first.render!(group("m1")) as { children?: unknown[] };
    const total = first.render!(group("合计")) as { children?: unknown[] };
    const hasArrow = (v: { children?: unknown[] }): boolean =>
      JSON.stringify(v.children ?? "").includes("›");
    expect(hasArrow(normal)).toBe(true);
    expect(hasArrow(total)).toBe(false);
  });

  it("row_accessibility_uses_label_not_key：可访问名称用展示名，点击仍传身份键", () => {
    const row: Group = { ...group("claudeopus55"), label: "Claude Opus 5.5" };
    const w = mount(UsageTable, { props: { report: report([row], false) } });
    const rp = exposed(w).rowProps(row);
    const aria = String(rp["aria-label"]);
    expect(aria).toContain("Claude Opus 5.5");
    expect(aria, "屏幕阅读器不应读出压缩身份键").not.toContain("claudeopus55");
    // 点击参数仍是后端筛选键（展示名不得当 key 用）。
    (rp.onclick as () => void)();
    expect(w.emitted("row-click")?.[0]).toEqual(["claudeopus55"]);
  });

  it("row_accessibility_falls_back_to_key：没有展示名时退回身份键", () => {
    const row = group("alpha");
    const w = mount(UsageTable, { props: { report: report([row], false) } });
    const rp = exposed(w).rowProps(row);
    expect(String(rp["aria-label"])).toContain("alpha");
  });

  it("total_row_emits_total_row_event：合计行走独立事件，不发 row-click（U04）", () => {
    // U04：合计行不是维度值——由 UsageTable 发 `total-row`，父级不再靠字面量
    // 「合计」判断；键盘 Enter/Space 与点击等价。
    const w = mount(UsageTable, {
      props: { report: report([group("m1"), group("合计")], false) },
    });
    const total = exposed(w).rowProps(group("合计"));
    (total.onclick as () => void)();
    expect(w.emitted("total-row"), "合计行点击发 total-row").toBeTruthy();
    expect(w.emitted("row-click"), "合计行不得发 row-click").toBeUndefined();
    (total.onkeydown as (e: { key: string; preventDefault: () => void }) => void)({
      key: " ",
      preventDefault: () => {},
    });
    expect(w.emitted("total-row")?.length).toBe(2);

    const normal = exposed(w).rowProps(group("m1"));
    (normal.onclick as () => void)();
    expect(w.emitted("row-click")?.[0]).toEqual(["m1"]);
    expect(w.emitted("total-row")?.length, "维度行不得发 total-row").toBe(2);
  });
});

// UX01：合计行字重 600 必须命中真实单元格（修复前是死选择器 .total-row strong，
// 实际合计单元格由 render 输出 span，字重仍为 400）。
describe("UsageTable 合计行字重（UX01）", () => {
  it("total_row_is_semibold：合计行真实 td 字重 600，上分隔线保留", () => {
    const src = usageTableSource;
    // 真实 td 规则（:deep(.total-row td)）合并后同时含上分隔线与 600 字重
    const bodies = [...src.matchAll(/:deep\(\.total-row td\)\s*\{([^}]*)\}/g)].map((m) => m[1]);
    expect(bodies.length, "必须存在命中真实 td 的合计行规则").toBeGreaterThan(0);
    const merged = bodies.join("\n");
    expect(merged).toContain("border-top: 1px solid var(--ts-separator-strong)");
    expect(merged).toContain("font-weight: 600");
    // 不再有死选择器 .total-row strong
    expect(src).not.toMatch(/\.total-row strong\s*\{/);
  });
});

// N18 复核项 2：分组费用列必须消费每组 cache_write_split_estimated，
// 让估算组、完整组、真正未计价组可区分（真实组件渲染 + 可访问文本）。
describe("N18 分组费用列的分档估算标记", () => {
  function exposed(w: ReturnType<typeof mount>) {
    return w.vm as unknown as {
      columns: {
        key: string;
        render?: (row: object) => unknown;
      }[];
    };
  }
  function costText(group: Group, totalsUnknown: boolean): string {
    const w = mount(UsageTable, { props: { report: report([group], totalsUnknown) } });
    const cols = exposed(w).columns;
    const cost = cols.find((x) => x.key === "cost_usd");
    const vnode = cost!.render!(group) as { children?: unknown };
    // render 返回 h() 的 vnode：取文本内容（字符串或 children 数组）
    const c = vnode as { children?: unknown; props?: { innerHTML?: string } };
    if (typeof c.children === "string") return c.children;
    if (Array.isArray(c.children)) {
      return (c.children as unknown[])
        .map((x) =>
          typeof x === "string" ? x : String((x as { children?: string })?.children ?? ""),
        )
        .join("");
    }
    return String(c.children ?? "");
  }

  it("估算组费用旁出现可访问的「分档估算」标记，完整组不出现", () => {
    const estimated = group("g-est");
    estimated.cost_usd = 7.5;
    estimated.cache_write_split_estimated = true;
    const complete = group("g-complete");
    complete.cost_usd = 7.5;
    complete.cache_write_split_estimated = false;
    const estText = costText(estimated, false);
    const completeText = costText(complete, false);
    expect(estText).toContain("$7.50");
    expect(estText).toContain("分档估算");
    expect(completeText).toContain("$7.50");
    expect(completeText).not.toContain("分档估算");
  });

  it("真正未计价组仍显示未知†，不冒充分档估算", () => {
    const unpriced = group("g-unpriced", { cache_write: 2_000_000 });
    unpriced.cost_usd = 0;
    const text = costText(unpriced, true);
    expect(text).toContain("未知†");
    expect(text).not.toContain("分档估算");
  });

  it("混合组（估算 + 未计价）两个标记同时出现", () => {
    const mixed = group("g-mixed", { cache_write: 1_000_000 });
    mixed.cost_usd = 3.75;
    mixed.unknown_pricing = true;
    mixed.cache_write_split_estimated = true;
    const text = costText(mixed, true);
    // 费用 > 0 时未知只以 † 后缀标注（未知数量在“未知†”列）；分档估算
    // 标记独立出现——两种状态在同组可并存且可区分。
    expect(text).toContain("†");
    expect(text).toContain("分档估算");
    expect(text).toContain("$3.75");
  });
});

// N18 复核项 2：零价估算与未计价混合组——「未知†」与「分档估算」须独立
// 同时出现（金额 0 时原实现隐藏了分档估算标记）。
describe("N18 分组费用列零价估算与未计价并存", () => {
  function exposed(w: ReturnType<typeof mount>) {
    return w.vm as unknown as {
      columns: { key: string; render?: (row: object) => unknown }[];
    };
  }
  function costText(group: Group, totalsUnknown: boolean): string {
    const w = mount(UsageTable, { props: { report: report([group], totalsUnknown) } });
    const cols = exposed(w).columns;
    const cost = cols.find((x) => x.key === "cost_usd");
    const vnode = cost!.render!(group) as { children?: unknown };
    if (typeof vnode.children === "string") return vnode.children;
    if (Array.isArray(vnode.children)) {
      return (vnode.children as unknown[])
        .map((x) =>
          typeof x === "string" ? x : String((x as { children?: string })?.children ?? ""),
        )
        .join("");
    }
    return String(vnode.children ?? "");
  }
  it("cost=0 且 estimated=true、unknown_pricing=true：同时显示未知†与分档估算", () => {
    const mixed = group("g-zero-mix", { cache_write: 2_000_000 });
    mixed.cost_usd = 0;
    mixed.unknown_pricing = true;
    mixed.cache_write_split_estimated = true;
    const text = costText(mixed, true);
    expect(text).toContain("未知†");
    expect(text).toContain("分档估算");
  });

  it("对照：纯未计价（无估算）只有未知†", () => {
    const unpriced = group("g-pure-unpriced", { cache_write: 2_000_000 });
    unpriced.cost_usd = 0;
    unpriced.unknown_pricing = true;
    unpriced.cache_write_split_estimated = false;
    const text = costText(unpriced, true);
    expect(text).toContain("未知†");
    expect(text).not.toContain("分档估算");
  });

  it("对照：纯估算（金额 0，显式 0 合并价）只有分档估算", () => {
    const est = group("g-pure-est");
    est.cost_usd = 0;
    est.unknown_pricing = false;
    est.cache_write_split_estimated = true;
    const text = costText(est, false);
    expect(text).toContain("分档估算");
    expect(text).not.toContain("未知†");
  });

  it("对照：完整组无任何标记", () => {
    const complete = group("g-complete");
    complete.cost_usd = 7.5;
    complete.unknown_pricing = false;
    complete.cache_write_split_estimated = false;
    const text = costText(complete, false);
    expect(text).toContain("$7.50");
    expect(text).not.toContain("未知†");
    expect(text).not.toContain("分档估算");
  });
});
