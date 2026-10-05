// 计划 A1/A2（docs/plans/active/2026-10-05-product-review-and-roadmap.md）：
// Dashboard 行为回归——mock IPC，不依赖 Tauri 运行时。
// F01：汇总与明细曾共用 runSeq，两个 immediate watcher 依次触发导致
// 首个汇总响应被作废（无快照 → 永久转圈；有快照 → 永久停留旧数据）。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import Dashboard from "./Dashboard.vue";
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

const summaryA: SummaryReport = {
  timezone: "Asia/Shanghai",
  generated_at: "2026-10-05T00:00:00+08:00",
  sources: [],
  by: "day",
  groups: [group("2026-10-01", 10)],
  totals: group("合计", 10),
  warnings: [],
};

const summaryB: SummaryReport = {
  ...summaryA,
  groups: [group("2026-10-02", 99)],
  totals: group("合计", 99),
};

const events: EventList = { rows: [], total: 0, warnings: [] };

/// 默认全部命令成功；个别测试用自定义实现覆盖特定命令。
function mockOk(): void {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "view_cache_load") return Promise.resolve(null);
    if (cmd === "summarize") return Promise.resolve(summaryA);
    if (cmd === "list_events") return Promise.resolve(events);
    if (cmd === "source_status") return Promise.resolve([]);
    return Promise.resolve(null);
  });
}

function mountDashboard(): VueWrapper {
  return mount(Dashboard, {
    props: { refreshKey: 0 },
    global: {
      // 重图表/表格子组件打桩：本组测试只验证查询编排与状态，不渲染 ECharts。
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

/** setup 内部状态的类型友好读取（script setup 绑定在开发构建下可经 vm 代理访问）。 */
function state(w: VueWrapper): Record<string, unknown> {
  return w.vm as unknown as Record<string, unknown>;
}

beforeEach(() => {
  invokeMock.mockReset();
  localStorage.clear();
});

describe("Dashboard 查询编排", () => {
  it("dashboard_first_load：无快照首次加载，汇总与明细都落地且不永久 loading", async () => {
    mockOk();
    // 明细先于汇总返回，放大旧逻辑下 runSeq 互相作废的问题
    const w = mountDashboard();
    await flushPromises();
    expect(state(w)["report"]).not.toBeNull();
    expect(state(w)["events"]).not.toBeNull();
    expect(state(w)["loading"]).toBe(false);
    expect(state(w)["eventsLoading"]).toBe(false);
    expect(w.html()).toContain("summary-cards-stub");
    // 首次加载不应显示缓存标记
    expect(w.text()).not.toContain("缓存数据");
  });

  it("dashboard_parallel_requests_settle：汇总与明细两条并发请求互不作废", async () => {
    let resolveSummary!: (v: SummaryReport) => void;
    let resolveEvents!: (v: EventList) => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "summarize") return new Promise((r) => (resolveSummary = r));
      if (cmd === "list_events") return new Promise((r) => (resolveEvents = r));
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect(state(w)["loading"]).toBe(true);
    expect(state(w)["eventsLoading"]).toBe(true);
    resolveEvents(events);
    await flushPromises();
    // 明细已回，汇总仍等待：此时 loading 必须仍为 true（独立状态）
    expect(state(w)["events"]).not.toBeNull();
    expect(state(w)["report"]).toBeNull();
    expect(state(w)["eventsLoading"]).toBe(false);
    resolveSummary(summaryA);
    await flushPromises();
    // 两条请求最终都必须落地（修复前：汇总响应被明细的请求代次作废）
    expect(state(w)["report"]).not.toBeNull();
    expect(state(w)["loading"]).toBe(false);
  });

  it("dashboard_latest_query_wins：乱序返回时只保留最新查询的结果", async () => {
    const resolvers: ((v: SummaryReport) => void)[] = [];
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "summarize") return new Promise((r) => resolvers.push(r));
      if (cmd === "list_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 切换 agent 触发第二次汇总查询
    state(w)["agent"] = "claude";
    await nextTick();
    await flushPromises();
    expect(resolvers.length).toBe(2);
    resolvers[1](summaryB); // 新响应先回
    await flushPromises();
    resolvers[0](summaryA); // 旧响应后回，必须被丢弃
    await flushPromises();
    const report = state(w)["report"] as SummaryReport;
    expect(report.groups[0].key).toBe("2026-10-02");
    expect(state(w)["loading"]).toBe(false);
  });

  it("ipc_error_visible：汇总失败有可见错误与重试，重试成功后恢复", async () => {
    let fail = true;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "summarize") {
        return fail ? Promise.reject(new Error("boom")) : Promise.resolve(summaryA);
      }
      if (cmd === "list_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 修复前：异常是无 catch 的未处理 rejection，界面无任何提示
    expect(state(w)["summaryError"]).toBe("boom");
    expect(w.text()).toContain("汇总加载失败");
    const retry = w.findAll("button").find((b) => b.text().includes("重试"));
    expect(retry).toBeDefined();
    fail = false;
    await retry!.trigger("click");
    await flushPromises();
    expect(state(w)["report"]).not.toBeNull();
    expect(state(w)["summaryError"]).toBeNull();
  });

  it("total_row_clears_drill：点击合计行不生成字面“合计”过滤", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    state(w)["by"] = "model";
    await nextTick();
    await flushPromises();
    const onClick = state(w)["onSummaryRowClick"] as (key: string) => void;
    onClick("claude-sonnet-4-5");
    expect((state(w)["drill"] as { key: string } | null)?.key).toBe("claude-sonnet-4-5");
    onClick("合计");
    expect(state(w)["drill"]).toBeNull();
  });
});

describe("Dashboard 视图快照与刷新（C4/F08）", () => {
  function snapshotPayload() {
    return {
      v: 2,
      saved_at: "2026-10-05T00:00:00Z",
      filters: {
        by: "model",
        agent: "claude",
        range: null,
        drill: { type: "model", key: "claude-sonnet-4-5" },
        tz: "Asia/Shanghai",
      },
      report: summaryA,
      events,
    };
  }

  it("view_cache_query_mismatch：v2 快照连同筛选一起恢复，口径一致", async () => {
    let resolveSummary!: (v: SummaryReport) => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "view_cache_load") return Promise.resolve(snapshotPayload());
      // 挂起后台刷新：先验证"缓存数据"展示态，再放行
      if (cmd === "summarize") return new Promise((r) => (resolveSummary = r));
      if (cmd === "list_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 筛选被恢复（数据与口径必然一致）
    expect(state(w)["agent"]).toBe("claude");
    expect(state(w)["by"]).toBe("model");
    expect((state(w)["drill"] as { key: string } | null)?.key).toBe("claude-sonnet-4-5");
    // 数据以"缓存数据"过期标记展示，等待后台刷新
    expect(state(w)["report"]).not.toBeNull();
    expect(state(w)["stale"]).toBe(true);
    expect(w.text()).toContain("缓存数据");
    resolveSummary(summaryA);
    await flushPromises();
    // 后台刷新落地后过期标记清除
    expect(state(w)["stale"]).toBe(false);
  });

  it("view_cache_query_mismatch：旧格式快照（无 v）不得当新数据展示", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "view_cache_load")
        return Promise.resolve({ report: summaryA, events, saved_at: "x" });
      if (cmd === "summarize") return Promise.resolve(summaryA);
      if (cmd === "list_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 旧快照整体忽略：数据来自新查询（stale 不被置位）
    expect(state(w)["stale"]).toBe(false);
  });

  it("refresh_preserves_filters：手动刷新以当前筛选重跑且筛选不动", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    state(w)["agent"] = "claude";
    await nextTick();
    await flushPromises();
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "summarize");
    const before = calls.length;
    const refresh = w.findAll("button").find((b) => b.text() === "刷新");
    expect(refresh).toBeDefined();
    await refresh!.trigger("click");
    await flushPromises();
    const after = invokeMock.mock.calls.filter((c) => c[0] === "summarize");
    expect(after.length).toBeGreaterThan(before);
    expect(after.at(-1)![1]).toMatchObject({ agent: "claude" });
    expect(state(w)["agent"]).toBe("claude");
  });
});
