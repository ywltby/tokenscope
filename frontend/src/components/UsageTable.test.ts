// 计划 C5：cost_unknown_partial_complete——费用未知的追溯完整：
// 存在未计价内容时出现"未知†"列并按行显示未计价 token；全部已计价时不出现。
// （† 语义 = 估算费用仅含已计价部分：无价格模型 + 部分计价两种来源。）
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import UsageTable from "./UsageTable.vue";
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

  it("数字列右对齐且使用 tabular 数字类", () => {
    const w = mount(UsageTable, { props: { report: report([group("m")], false) } });
    const cols = exposed(w).columns;
    for (const key of ["requests", "tokens.input", "tokens.output", "total", "cost_usd"]) {
      const c = cols.find((x) => x.key === key);
      expect(c, `${key} 列存在`).toBeDefined();
      expect(c!.align).toBe("right");
      expect(c!.className).toContain("ts-num");
    }
  });

  it("费用列表头带估算语义；未知标记用警告色而非 opacity", () => {
    const w = mount(UsageTable, { props: { report: report([group("m")], true) } });
    const cols = exposed(w).columns;
    const cost = cols.find((x) => x.key === "cost_usd");
    expect(String(cost!.title)).toContain("估算");
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
});
