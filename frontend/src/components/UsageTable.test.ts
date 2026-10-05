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
