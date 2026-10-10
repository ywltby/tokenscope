// H08 + 审阅回归：日报粒度历史的展示与"仅明细查询"恢复路径。
// 断言：时区不匹配提示来自 `rollup_coverage.timezone_mismatch`（蛇形字段，
// 早期 camelCase 改名会让这段提示整段消失）；点击「只看请求明细」后新建查询
// 会话并带 `rollups: "detail_only"`。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import Dashboard from "./Dashboard.vue";
import type { Group, SummaryReport } from "../types";

function group(key: string, input: number): Group {
  return {
    key,
    requests: 1,
    tokens: { input, output: 1, cache_write: 0, cache_read: 0 },
    cost_usd: 0,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  };
}

const mismatchText = "来源时区 Asia/Shanghai 与展示时区不一致：日粒度历史不参与按日视图";

function reportWithMismatch(): SummaryReport {
  return {
    query_id: "q-test-a",
    pricing_revision: "rev-test",
    timezone: "UTC",
    generated_at: "2026-10-10T00:00:00Z",
    sources: [],
    by: "day",
    groups: [group("2026-10-01", 10)],
    totals: group("合计", 10),
    warnings: [],
    rollup_coverage: {
      detail_buckets: 1,
      rollup_buckets: 2,
      unresolved_buckets: 0,
      unresolved_covered_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
      timezone_mismatch: mismatchText,
    },
  };
}

const queryInfo = {
  queryId: "q-test-a",
  generation: 1,
  pricingRevision: "rev-test",
  timezone: "UTC",
  asOf: "2026-10-10T00:00:00Z",
};

function mountDashboard(): VueWrapper {
  return mount(Dashboard, {
    props: { refreshKey: 0 },
    global: {
      stubs: {
        SummaryCards: true,
        UsageTable: true,
        TrendChart: true,
        EventTable: true,
        AgentIcon: true,
        DateRangeSelect: true,
      },
    },
  });
}

beforeEach(() => {
  invokeMock.mockReset();
  localStorage.clear();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "query_begin") return Promise.resolve(queryInfo);
    if (cmd === "view_cache_load") return Promise.resolve(null);
    if (cmd === "query_summary") return Promise.resolve(reportWithMismatch());
    if (cmd === "query_events")
      return Promise.resolve({
        query_id: "q-test-a",
        pricing_revision: "rev-test",
        rows: [],
        total: 0,
        warnings: [],
      });
    if (cmd === "source_status") return Promise.resolve([]);
    return Promise.resolve(null);
  });
});

describe("Dashboard 日粒度历史（H08）", () => {
  it("时区不匹配提示可见，并可切换为只看请求明细", async () => {
    const w = mountDashboard();
    await flushPromises();
    expect(w.text()).toContain("日粒度历史不参与按日视图");

    const action = w.findAll("button").find((b) => b.text().includes("只看请求明细"));
    expect(action, "必须提供「只看请求明细」恢复入口").toBeDefined();
    const beginsBefore = invokeMock.mock.calls.filter((c) => c[0] === "query_begin").length;
    await action!.trigger("click");
    await flushPromises();

    const begins = invokeMock.mock.calls.filter((c) => c[0] === "query_begin");
    expect(begins.length).toBeGreaterThan(beginsBefore);
    const last = begins[begins.length - 1][1] as { rollups: string | null };
    expect(last.rollups).toBe("detail_only");
    expect(w.text()).toContain("当前只统计请求明细");
    w.unmount();
  });

  it("切换后恢复自动参与同样重建查询会话", async () => {
    const w = mountDashboard();
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text().includes("只看请求明细"))!
      .trigger("click");
    await flushPromises();
    const restore = w.findAll("button").find((b) => b.text().includes("恢复自动参与"));
    expect(restore, "必须提供恢复入口").toBeDefined();
    await restore!.trigger("click");
    await flushPromises();
    const begins = invokeMock.mock.calls.filter((c) => c[0] === "query_begin");
    const last = begins[begins.length - 1][1] as { rollups: string | null };
    expect(last.rollups).toBeNull();
    w.unmount();
  });
});
