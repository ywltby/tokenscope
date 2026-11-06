// H08 + 审阅回归：日报粒度历史的展示与"仅明细查询"恢复路径。
// 断言：时区不匹配提示来自 `rollup_coverage.timezone_mismatch`（蛇形字段，
// 早期 camelCase 改名会让这段提示整段消失）；点击「只看请求明细」后新建查询
// 会话并带 `rollups: "detail_only"`；该模式随视图快照保存/恢复（v12）——
// 重新进入页面不得静默回退成 auto。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import Dashboard from "./Dashboard.vue";
import {
  SNAPSHOT_VERSION,
  resetSnapshotQueueForTests,
  snapshotQueueIdle,
} from "../lib/viewSnapshot";
import { VIEW_FILTERS_KEY } from "../lib/viewFilters";
import { resetTimezoneForTests } from "../composables/timezone";
import type { EventList, Group, SummaryReport } from "../types";

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
  resetSnapshotQueueForTests();
  // U04：时区是模块级单例，参与"快照是否可用于首帧"的比较——每个用例从默认
  // 时区起步。
  resetTimezoneForTests();
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

  it("仅明细模式随视图快照保存（v11，复审 #7）", async () => {
    const w = mountDashboard();
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text().includes("只看请求明细"))!
      .trigger("click");
    await flushPromises();
    await snapshotQueueIdle();
    const saves = invokeMock.mock.calls.filter((c) => c[0] === "view_cache_save");
    expect(saves.length, "切换后必须有快照落盘").toBeGreaterThan(0);
    const payload = saves[saves.length - 1][1].value as {
      v: number;
      rollups_mode: string;
    };
    expect(payload.v).toBe(SNAPSHOT_VERSION);
    expect(payload.rollups_mode, "模式必须写进快照").toBe("detail_only");
    w.unmount();
  });

  it("重新进入页面恢复偏好中的仅明细模式，不回退 auto（U04 / 复审 #7）", async () => {
    // U04：日粒度参与方式是**筛选偏好**（localStorage），不再依赖视图快照恢复
    // ——偏好决定"查什么"，视图快照只在偏好与时区逐项相等时用于首帧展示。
    localStorage.setItem(
      VIEW_FILTERS_KEY,
      JSON.stringify({
        by: "day",
        agent: "all",
        preset: null,
        rollupsMode: "detail_only",
        drill: null,
      }),
    );
    const events: EventList = {
      query_id: "q-test-a",
      pricing_revision: "rev-test",
      rows: [],
      total: 0,
      warnings: [],
    };
    const report = reportWithMismatch();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(report);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect(w.text(), "偏好里的模式必须生效").toContain("当前只统计请求明细");
    const begins = invokeMock.mock.calls.filter((c) => c[0] === "query_begin");
    expect(begins.length, "首批查询必须重建会话").toBeGreaterThan(0);
    const last = begins[begins.length - 1][1] as { rollups: string | null };
    expect(last.rollups).toBe("detail_only");
    w.unmount();
  });

  it("旧版本快照（v10）被忽略：模式回退 auto（复审 #7）", async () => {
    const events: EventList = {
      query_id: "q-test-a",
      pricing_revision: "rev-test",
      rows: [],
      total: 0,
      warnings: [],
    };
    const report = reportWithMismatch();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load")
        return Promise.resolve({
          v: SNAPSHOT_VERSION - 1,
          saved_at: "2026-10-10T00:00:00Z",
          filters: { by: "day", agent: "all", preset: null, drill: null },
          rollups_mode: "detail_only",
          report,
          events,
        });
      if (cmd === "query_summary") return Promise.resolve(report);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    const begins = invokeMock.mock.calls.filter((c) => c[0] === "query_begin");
    const last = begins[begins.length - 1][1] as { rollups: string | null };
    expect(last.rollups, "旧版快照不可信，按正常加载走 auto").toBeNull();
    w.unmount();
  });

  it("同桶多批次汇总的边界歧义如实披露（第三轮）", async () => {
    const note =
      "有 1 个日期/模型桶的日汇总来自不同时刻导入的批次：覆盖边界只能取最早的来源快照时刻，" +
      "落在批次之间的明细已按明细计入，可能与较晚批次的汇总重复；这些桶请对照请求明细核对。";
    const report = reportWithMismatch();
    report.rollup_coverage = {
      ...report.rollup_coverage!,
      timezone_mismatch: null,
      multi_batch_boundary: note,
    };
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(report);
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
    const w = mountDashboard();
    await flushPromises();
    expect(w.text(), "批次之间的歧义必须出现在界面").toContain("不同时刻导入的批次");
    w.unmount();
  });

  it("旧版导入的批次边界不可确认时如实披露（第四轮）", async () => {
    const note =
      "有 1 个日期/模型桶的日汇总来自旧版导入（批次时间在读取结束后才生成，不能代表来源快照建立时刻）：" +
      "这些桶按整桶信任口径统计，快照之后到达的请求可能被少算。";
    const report = reportWithMismatch();
    report.rollup_coverage = {
      ...report.rollup_coverage!,
      timezone_mismatch: null,
      legacy_boundary: note,
    };
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(report);
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
    const w = mountDashboard();
    await flushPromises();
    expect(w.text(), "旧版批次边界必须可见").toContain("旧版导入");
    w.unmount();
  });
});
