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
// UX08：样式契约断言需要 SFC 源文本（项目无 @types/node，用 Vite ?raw）
import dashboardSource from "./Dashboard.vue?raw";
import TrendChart from "../components/TrendChart.vue";
import type { Dim, EventList, Group, SummaryReport } from "../types";
import { SNAPSHOT_VERSION } from "../lib/viewSnapshot";

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
  query_id: "q-test-a",
  pricing_revision: "rev-test",
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

const events: EventList = {
  query_id: "q-test-a",
  pricing_revision: "rev-test",
  rows: [],
  total: 0,
  warnings: [],
};

/// SF04：query_begin 句柄（与 fixtures 的会话身份一致）。
const queryInfo = {
  queryId: "q-test-a",
  generation: 1,
  pricingRevision: "rev-test",
  timezone: "Asia/Shanghai",
  asOf: "2026-10-05T00:00:00+08:00",
};

/// 默认全部命令成功；个别测试用自定义实现覆盖特定命令。
function mockOk(): void {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "query_begin") return Promise.resolve(queryInfo);
    if (cmd === "view_cache_load") return Promise.resolve(null);
    if (cmd === "query_summary") return Promise.resolve(summaryA);
    if (cmd === "query_events") return Promise.resolve(events);
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
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return new Promise((r) => (resolveSummary = r));
      if (cmd === "query_events") return new Promise((r) => (resolveEvents = r));
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
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return new Promise((r) => resolvers.push(r));
      if (cmd === "query_events") return Promise.resolve(events);
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
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") {
        return fail ? Promise.reject(new Error("boom")) : Promise.resolve(summaryA);
      }
      if (cmd === "query_events") return Promise.resolve(events);
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

  it("model_group_label_and_drill_key_are_distinct：下钻键是身份键，展示标签是友好名", async () => {
    const modelReport: SummaryReport = {
      ...summaryA,
      by: "model",
      groups: [{ ...group("claudeopus55", 10), label: "Claude Opus 5.5" }, group("合计", 10)],
      totals: group("合计", 10),
    };
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "query_summary") return Promise.resolve(modelReport);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    state(w)["by"] = "model";
    await nextTick();
    await flushPromises();
    const onClick = state(w)["onSummaryRowClick"] as (key: string) => void;
    onClick("claudeopus55");
    await nextTick();
    const drill = state(w)["drill"] as { key: string; label?: string } | null;
    // 传后端查询的是身份键；展示用 label（绝不是把名称反过来当 key）。
    expect(drill?.key).toBe("claudeopus55");
    expect(drill?.label).toBe("Claude Opus 5.5");
    expect(w.text()).toContain("Claude Opus 5.5");
    expect(w.text()).not.toContain("claudeopus55");
  });
});

// 当前版本快照样例（模块级：多个 describe 共用）
function snapshotPayload() {
  return {
    v: SNAPSHOT_VERSION,
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

describe("Dashboard 视图快照与刷新（C4/F08）", () => {
  it("view_cache_query_mismatch：v6 快照连同筛选一起恢复，口径一致", async () => {
    let resolveSummary!: (v: SummaryReport) => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(snapshotPayload());
      // 挂起后台刷新：先验证"缓存数据"展示态，再放行
      if (cmd === "query_summary") return new Promise((r) => (resolveSummary = r));
      if (cmd === "query_events") return Promise.resolve(events);
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

  it("v4 旧快照（可能含混代拼接/晚到接管残留）被忽略，走正常加载", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") {
        const p = snapshotPayload();
        return Promise.resolve({ ...p, v: 4 });
      }
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // v4 被忽略：筛选不得被恢复
    expect(state(w)["agent"]).toBe("all");
    expect(state(w)["stale"]).toBe(false);
  });

  it("old_project_identity_snapshot_is_ignored：旧项目身份快照的旧分组与旧下钻 key 都不恢复", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") {
        const p = snapshotPayload();
        return Promise.resolve({
          ...p,
          // v7 及更早的快照：项目 key 是"会话初始路径/slug"口径，分组 key 与
          // 下钻 key（老 slug）在项目根归并后都已失效——读取时必须整体忽略，
          // 而不是恢复旧分组。
          v: 6,
          filters: { ...p.filters, drill: { type: "project", key: "alpha" } },
        });
      }
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect(state(w)["agent"]).toBe("all");
    expect(state(w)["drill"]).toBeNull();
    expect(state(w)["stale"]).toBe(false);
    // 走正常加载：展示的是后端新分组，而不是旧快照里的旧分组。
    expect((state(w)["report"] as SummaryReport).groups[0].key).toBe("2026-10-01");
  });

  it("old_matching_rules_snapshot_is_ignored：旧匹配规则算出的金额不恢复，新响应后正常保存", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") {
        const p = snapshotPayload();
        // v8 快照：金额由旧的模型匹配规则（`-`/`.` 不等价）算出。
        return Promise.resolve({
          ...p,
          v: 8,
          report: {
            ...summaryA,
            groups: [group("claude-opus-5.5", 42)],
            totals: group("合计", 42),
          },
        });
      }
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 旧快照整体忽略：金额与分组来自新响应（而非旧规则的 42）。
    expect(state(w)["stale"]).toBe(false);
    const report = state(w)["report"] as SummaryReport;
    expect(report.groups[0].key).toBe("2026-10-01");
    // 新响应落地后按当前版本正常保存（含新的 v）。
    const w2 = mountDashboard();
    await flushPromises();
    await nextTick();
    const saves = invokeMock.mock.calls.filter((c) => c[0] === "view_cache_save");
    expect((saves.at(-1)?.[1] as { value: { v: number } }).value.v).toBe(SNAPSHOT_VERSION);
    w2.unmount();
  });

  it("late_snapshot_cannot_replace_fresh_report：晚到缓存不得覆盖新结果", async () => {
    let resolveCache!: (v: unknown) => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load")
        return new Promise((r) => {
          resolveCache = r;
        });
      if (cmd === "query_summary") return Promise.resolve(summaryB); // 新汇总 99 先落地
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect((state(w)["report"] as SummaryReport).groups[0].key).toBe("2026-10-02");
    // 旧缓存（10）此时才到：必须被丢弃
    resolveCache(snapshotPayload());
    await flushPromises();
    expect((state(w)["report"] as SummaryReport).groups[0].key).toBe("2026-10-02");
    expect(state(w)["stale"]).toBe(false); // 新结果落地后缓存不再接管
  });

  it("view_cache_query_mismatch：v2 旧快照（ms range）不得当新数据展示", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load")
        return Promise.resolve({ report: summaryA, events, saved_at: "x" });
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 旧快照整体忽略：数据来自新查询（stale 不被置位）
    expect(state(w)["stale"]).toBe(false);
  });

  it("events_load_more_pagination：游标追加载取，不重不漏且游标正确", async () => {
    const row = (ts: string, rid: string) => ({
      ts,
      record_id: rid,
      cursor: `${ts}T00:00:00.000Z|${rid}`,
      agent: "codex",
      model: "m",
      session_id: "s",
      project: "p",
      input: 1,
      output: 1,
      cache_write: 0,
      cache_read: 0,
      cost_usd: 0,
    });
    invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") {
        const before = (args?.before as string | null) ?? null;
        if (!before) {
          return Promise.resolve({
            rows: [row("2026-10-02 10:00:00", "b"), row("2026-10-01 09:00:00", "a")],
            total: 3,
            warnings: [],
          });
        }
        return Promise.resolve({
          rows: [row("2026-09-30 08:00:00", "z")],
          total: 3,
          warnings: [],
        });
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect((state(w)["events"] as EventList).rows.length).toBe(2);
    expect(state(w)["hasMore"]).toBe(true);
    (state(w)["loadMoreEvents"] as () => void)();
    await flushPromises();
    const ev = state(w)["events"] as EventList;
    expect(ev.rows.length).toBe(3);
    expect(ev.rows.map((r) => r.record_id)).toEqual(["b", "a", "z"]);
    const call = invokeMock.mock.calls.filter((c) => c[0] === "query_events").at(-1)![1] as Record<
      string,
      unknown
    >;
    expect(call.before).toBe("2026-10-01 09:00:00T00:00:00.000Z|a");
    expect(state(w)["hasMore"]).toBe(false);
  });

  it("refresh_preserves_filters：手动刷新以当前筛选重跑且筛选不动", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    state(w)["agent"] = "claude";
    await nextTick();
    await flushPromises();
    // SF04：query_begin 携带主筛选参数；query_summary 只带 query_id。
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "query_begin");
    const before = calls.length;
    const refresh = w.findAll("button").find((b) => b.text() === "刷新");
    expect(refresh).toBeDefined();
    await refresh!.trigger("click");
    await flushPromises();
    const after = invokeMock.mock.calls.filter((c) => c[0] === "query_begin");
    expect(after.length).toBeGreaterThan(before);
    expect(after.at(-1)![1]).toMatchObject({ agent: "claude" });
    expect(state(w)["agent"]).toBe("claude");
  });
});

describe("Dashboard 状态（设计系统 Task 7）", () => {
  function emptyReport(warnings: string[] = [], sources: object[] = []) {
    return {
      timezone: "UTC",
      generated_at: "t",
      sources,
      by: "day",
      groups: [],
      totals: {
        key: "totals",
        requests: 0,
        tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
        cost_usd: 0,
        unknown_pricing: false,
        unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
      },
      warnings,
    };
  }

  function mountEmpty(warnings: string[] = [], sources: object[] = []): VueWrapper {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "query_summary") return Promise.resolve(emptyReport(warnings, sources));
      if (cmd === "query_events") return Promise.resolve({ rows: [], total: 0, warnings: [] });
      if (cmd === "source_status") return Promise.resolve([]);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      return Promise.resolve(null);
    });
    return mount(Dashboard, { props: { refreshKey: 0 } });
  }

  it("空数据时有明确状态文案与下一步指引", async () => {
    const w = mountEmpty();
    await flushPromises();
    expect(w.text()).toContain("暂无数据");
    expect(w.text()).toContain("调整时间范围");
  });

  it("R08 empty_report_keeps_collection_errors_visible：空结果 + 采集异常必须可见", async () => {
    const w = mountEmpty(
      ["Codex 快照解析失败，该层已忽略"],
      [
        {
          agent: "codex",
          stats: { io_errors: 2, bad_lines: 5, lines_seen: 10, events: 0 },
        },
      ],
    );
    await flushPromises();
    const notice = w.findAll(".ts-notice").find((n) => n.text().includes("本轮采集存在部分问题"));
    // 采集诊断通知必须可见
    expect(notice).toBeDefined();
    expect(notice!.text()).toContain("2 个文件读取失败");
    expect(notice!.text()).toContain("5 行解析失败");
    expect(notice!.text()).toContain("1 条采集警告");
    // 详情可展开
    const toggle = notice!.findAll("button").find((b) => b.text().includes("详情"));
    await toggle!.trigger("click");
    expect(w.text()).toContain("Codex 快照解析失败");
    // 空状态仍显示
    expect(w.text()).toContain("暂无数据");
  });

  it("R08 partial_report_has_visible_notice：部分数据 + 警告可见且表格仍渲染", async () => {
    const r = { ...summaryA, warnings: ["来源目录重叠：重复文件只统计一次"] };
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(r);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    const notice = w.findAll(".ts-notice").find((n) => n.text().includes("本轮采集存在部分问题"));
    expect(notice, "部分数据的警告必须可见").toBeDefined();
    expect(notice!.text()).toContain("1 条采集警告");
    // 表格仍渲染（部分数据展示；mountDashboard 中表格已打桩）
    expect(w.html()).toContain("usage-table-stub");
  });

  it("R08 clean_empty_report_is_not_error：干净空区间不误报", async () => {
    const w = mountEmpty();
    await flushPromises();
    const notices = w.findAll(".ts-notice").filter((n) => n.text().includes("本轮采集"));
    expect(notices.length).toBe(0); // 干净空区间不得显示采集诊断
  });
});

describe("视图快照查询身份（R04）", () => {
  function deferred<T>(): [Promise<T>, (v: T) => void] {
    let resolve!: (v: T) => void;
    const promise = new Promise<T>((r) => {
      resolve = r;
    });
    return [promise, resolve];
  }

  it("snapshot_waits_for_matching_summary_and_events：筛选不一致不落盘", async () => {
    const [sumAll, resolveSumAll] = deferred<SummaryReport>();
    const [evAll, resolveEvAll] = deferred<EventList>();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") {
        // 第一轮（all）挂起；切 claude 后的第二轮正常返回
        return invokeMock.mock.calls.filter((c) => c[0] === "query_summary").length === 1
          ? sumAll
          : Promise.resolve(summaryB);
      }
      if (cmd === "query_events") {
        return invokeMock.mock.calls.filter((c) => c[0] === "query_events").length === 1
          ? evAll
          : Promise.resolve(events);
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 切 claude：触发第二轮查询（第一轮 all 汇总仍挂起）
    state(w)["agent"] = "claude";
    await flushPromises();
    // claude 明细先回（此时汇总键仍是挂起的 all）→ 不得落盘
    resolveEvAll(events);
    await flushPromises();
    // all 汇总随后才回（其捕获键 agent=all，与 claude 明细键不一致）→ 不得落盘
    resolveSumAll(summaryA);
    await flushPromises();
    const saves = invokeMock.mock.calls.filter((c) => c[0] === "view_cache_save");
    for (const call of saves) {
      const p = call[1] as { value: { filters: { agent: string } } };
      expect(p.value.filters.agent).toBe("claude"); // 只允许同口径保存
    }
    // 之后 claude 汇总落地（summaryB 已在第二轮返回）→ 允许同口径保存
    expect(saves.length).toBeGreaterThanOrEqual(1);
  });

  it("failed_refresh_does_not_save_mixed_snapshot：汇总失败不落盘", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.reject(new Error("boom"));
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    const saves = invokeMock.mock.calls.filter((c) => c[0] === "view_cache_save");
    expect(saves.length).toBe(0); // 汇总失败时明细不得单独落盘
    expect(state(w)["summaryError"]).toBe("boom");
  });

  it("stale_failure_shows_retry_state：旧数据 + 刷新失败 = 失败态（非后台刷新中）", async () => {
    let fail = true;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(snapshotPayload());
      if (cmd === "query_summary")
        return fail ? Promise.reject(new Error("net down")) : Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 恢复后后台刷新失败：stale + error
    const pill = w.find(".page-head .ts-status-pill");
    expect(pill.classes()).toContain("ts-pill-warning");
    expect(pill.text()).toContain("刷新失败");
    expect(w.text()).toContain("net down");
    // 重试成功 → 已更新
    fail = false;
    const retry = w.findAll("button").find((b) => b.text() === "重试");
    await retry!.trigger("click");
    await flushPromises();
    expect(w.find(".page-head .ts-status-pill").text()).toContain("已更新");
  });

  it("unmounted_dashboard_cannot_overwrite_new_snapshot：卸载实例的晚到响应不落盘", async () => {
    const [sumAll, resolveSumAll] = deferred<SummaryReport>();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return sumAll;
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    w.unmount();
    // 卸载后旧实例的汇总才返回：不得更新状态，也不得触发保存
    resolveSumAll(summaryA);
    await flushPromises();
    const saves = invokeMock.mock.calls.filter((c) => c[0] === "view_cache_save");
    expect(saves.length).toBe(0);
  });
});

describe("视图快照批次与恢复所有权（F03）", () => {
  function deferred<T>(): [Promise<T>, (v: T) => void] {
    let resolve!: (v: T) => void;
    const promise = new Promise<T>((r) => {
      resolve = r;
    });
    return [promise, resolve];
  }

  type SavePayload = {
    value: {
      filters: { by: string; agent: string };
      report: SummaryReport;
      events: EventList;
    };
  };

  function saveCalls(): SavePayload[] {
    return invokeMock.mock.calls
      .filter((c) => c[0] === "view_cache_save")
      .map((c) => c[1] as SavePayload);
  }

  it("same_filter_refresh_failure_does_not_save_mixed_snapshot：同筛选刷新新汇总成功+明细失败不得落盘拼接", async () => {
    // 旧 10/10 已保存；同筛选手动刷新：汇总 99 成功、明细失败——
    // 不得出现 filters 与旧明细（同筛选但旧批次）拼成的 99/10 快照。
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") {
        return invokeMock.mock.calls.filter((c) => c[0] === "query_summary").length === 1
          ? Promise.resolve(summaryA)
          : Promise.resolve(summaryB);
      }
      if (cmd === "query_events") {
        return invokeMock.mock.calls.filter((c) => c[0] === "query_events").length === 1
          ? Promise.resolve(events)
          : Promise.reject(new Error("ev boom"));
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect(saveCalls().length).toBeGreaterThanOrEqual(1); // 首批 10/10 正常落盘
    const refreshBtn = w.findAll("button").find((b) => b.text() === "刷新");
    await refreshBtn!.trigger("click");
    await flushPromises();
    // 汇总已是 99（2026-10-02），但明细失败：任何保存都不得携带新汇总
    for (const call of saveCalls()) {
      expect(call.value.report.groups[0].key).toBe("2026-10-01");
    }
  });

  it("dimension_switch_waits_for_matching_report：切维度后明细先到不得保存 model/day 拼接", async () => {
    // day 批次完成后切 model：明细（model）先回、汇总（model）在途——
    // 不得保存 filters.by=model + report.by=day 的混代快照。
    const [sumModel, resolveSumModel] = deferred<SummaryReport>();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") {
        return invokeMock.mock.calls.filter((c) => c[0] === "query_summary").length === 1
          ? Promise.resolve(summaryA)
          : sumModel;
      }
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    state(w)["by"] = "model";
    await flushPromises();
    // 明细（model）已落地、汇总（model）仍挂起：任何保存的 filters.by
    // 必须与 report.by 一致（修复前保存 model/day 拼接）
    for (const call of saveCalls()) {
      expect(call.value.filters.by).toBe(call.value.report.by);
    }
    // 汇总随后落地 → 同批次保存放行（report.by 与查询维度一致）
    resolveSumModel({ ...summaryB, by: "model" });
    await flushPromises();
    const last = saveCalls().at(-1)!.value;
    expect(last.filters.by).toBe("model");
    expect(last.report.by).toBe("model");
  });

  it("user_filter_change_prevents_late_cache_restore：用户已选筛选，晚到缓存不得接管", async () => {
    // 首载未返回时用户选 claude；all 筛选的启动缓存晚到——不得重置筛选。
    const [cache, resolveCache] = deferred<unknown>();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return cache;
      if (cmd === "query_summary") return new Promise(() => {}); // 首载挂起
      if (cmd === "query_events") return new Promise(() => {});
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    state(w)["agent"] = "claude";
    await flushPromises();
    resolveCache(snapshotPayload()); // filters.agent = "all"
    await flushPromises();
    expect(state(w)["agent"]).toBe("claude");
    expect(state(w)["report"]).toBeNull();
    expect(state(w)["stale"]).toBe(false);
  });

  it("append_cannot_invalidate_new_filter_first_page：新筛选首批在途时追加被拒绝", async () => {
    // 切筛选后首批（model）在途：此时触发的"加载更多"必须被拒绝——
    // 旧筛选的游标不得用于新筛选，model 首批也不得被追加响应作废。
    const row = (ts: string, rid: string) => ({
      ts,
      record_id: rid,
      cursor: `${ts}T00:00:00.000Z|${rid}`,
      agent: "codex",
      model: "m",
      session_id: "s",
      project: "p",
      input: 1,
      output: 1,
      cache_write: 0,
      cache_read: 0,
      cost_usd: 0,
    });
    const [firstModelPage, resolveFirstModelPage] = deferred<EventList>();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") {
        const n = invokeMock.mock.calls.filter((c) => c[0] === "query_events").length;
        if (n === 1)
          return Promise.resolve({
            rows: [row("2026-10-02 10:00:00", "a")],
            total: 3,
            warnings: [],
          });
        if (n === 2) return firstModelPage; // model 首批挂起
        return Promise.resolve({
          rows: [row("2026-10-01 09:00:00", "z")],
          total: 3,
          warnings: [],
        });
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["a"]);
    state(w)["by"] = "model";
    await flushPromises(); // model 首批在途（eventsLoading = true）
    (state(w)["loadMoreEvents"] as () => void)(); // 必须被拒绝：不发起第三次请求
    await flushPromises();
    resolveFirstModelPage({
      query_id: "q-test-a",
      pricing_revision: "rev-test",
      rows: [row("2026-10-02 11:00:00", "b")],
      total: 3,
      warnings: [],
    });
    await flushPromises();
    const evCalls = invokeMock.mock.calls.filter((c) => c[0] === "query_events");
    expect(evCalls.length).toBe(2);
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["b"]);
  });
});

describe("Dashboard 来源筛选（纯图标分段）", () => {
  it("来源只显示图标且保留名称，点击切换过滤", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    const group = w.find('[role="radiogroup"][aria-label="数据来源"]');
    expect(group.exists()).toBe(true);
    const items = group.findAll('[role="radio"]');
    expect(items.map((t) => t.text())).toEqual(["", "", ""]);
    expect(items.map((t) => t.attributes("aria-label"))).toEqual(["全部", "Claude Code", "Codex"]);
    expect(items.map((t) => t.attributes("title"))).toEqual(["全部", "Claude Code", "Codex"]);
    expect(items[0].attributes("aria-checked")).toBe("true");
    // AgentIcon 保留品牌身份，文字由可访问名称与悬停提示承载。
    expect(items[1].find(".seg-icon").exists()).toBe(true);
    await items[2].trigger("click");
    await flushPromises();
    expect(state(w)["agent"]).toBe("codex");
  });
});

describe("Dashboard 页头与通知（设计系统 Task 3）", () => {
  it("大标题在左，状态胶囊与刷新在右，不显示时区日期说明", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    expect(w.find(".page-title").text()).toBe("用量汇总");
    const head = w.find(".page-head");
    expect(head.find(".page-sub").exists()).toBe(false);
    const pill = head.find(".ts-status-pill");
    expect(pill.exists()).toBe(true);
    expect(pill.text()).toContain("已更新");
    expect(head.findAll("button").some((b) => b.text() === "刷新")).toBe(true);
  });

  it("stale 快照 → 标题行警告胶囊；后台刷新落地 → 成功胶囊", async () => {
    let resolveSummary!: (v: SummaryReport) => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(snapshotPayload());
      if (cmd === "query_summary") return new Promise((r) => (resolveSummary = r));
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    const pill = w.find(".page-head .ts-status-pill");
    expect(pill.classes()).toContain("ts-pill-warning");
    expect(pill.text()).toContain("缓存数据");
    resolveSummary(summaryA);
    await flushPromises();
    expect(w.find(".page-head .ts-status-pill").classes()).toContain("ts-pill-success");
  });

  it("筛选顺序固定：来源首行，日期与聚合维度次行，时区仅在设置页", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    const src = w.find('[aria-label="数据来源"]').element;
    const dim = w.find('[aria-label="聚合维度"]').element;
    const range = w.findComponent({ name: "DateRangeSelect" }).element;
    expect(w.find(".tz-select").exists()).toBe(false);
    const FOLLOWING = Node.DOCUMENT_POSITION_FOLLOWING;
    expect(src.compareDocumentPosition(range) & FOLLOWING).toBeTruthy();
    expect(range.compareDocumentPosition(dim) & FOLLOWING).toBeTruthy();
    expect(range.parentElement).toBe(dim.parentElement);
    expect(src.parentElement).not.toBe(dim.parentElement);
  });

  it("来源异常渲染为内联通知，去设置动作发出 go-settings", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status")
        return Promise.resolve([
          {
            agent: "codex",
            dir: "C:/codex",
            enabled: true,
            exists: false,
            files: 0,
            state: "missing",
          },
        ]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    const notice = w.find(".ts-notice");
    expect(notice.exists()).toBe(true);
    expect(notice.text()).toContain("Codex 数据目录不存在");
    expect(notice.find(".ts-notice-icon").exists()).toBe(true);
    const action = notice.findAll("button, a").find((n) => n.text().includes("去设置"));
    expect(action).toBeDefined();
    await action!.trigger("click");
    expect(w.emitted("go-settings")).toBeTruthy();
  });

  it("多个来源异常合并为一条可展开通知（任务 7：多条合并）", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status")
        return Promise.resolve([
          {
            agent: "codex",
            dir: "C:/codex",
            enabled: true,
            exists: false,
            files: 0,
            state: "missing",
          },
          {
            agent: "claude-code",
            dir: "C:/claude",
            enabled: true,
            exists: true,
            files: 0,
            state: "empty",
          },
        ]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 收起时：一条合并通知，不铺开逐条明细
    expect(w.text()).toContain("2 个来源异常");
    expect(w.text()).not.toContain("数据目录不存在");
    const notice = w.findAll(".ts-notice").find((n) => n.text().includes("个来源异常"))!;
    const toggle = notice.findAll("button").find((b) => b.text().includes("详情"));
    expect(toggle).toBeDefined();
    await toggle!.trigger("click");
    // 展开后逐条明细可见
    expect(w.text()).toContain("数据目录不存在");
    expect(w.text()).toContain("没有发现会话日志");
  });

  it("汇总加载失败渲染为可重试内联通知（非 NAlert）", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.reject(new Error("boom"));
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    const notices = w.findAll(".ts-notice");
    expect(notices.length).toBeGreaterThan(0);
    expect(notices.some((n) => n.text().includes("汇总加载失败"))).toBe(true);
    expect(w.find(".n-alert").exists()).toBe(false);
  });
});

// UX05：图表实例生命周期与类别展示——父级用 computed 缓存真实类别（排除
// 合计），以真实类别数判断是否绘制；无关父级更新不改变 groups 身份。
describe("Dashboard 图表类别（UX05）", () => {
  function reportWith(count: number, by: Dim): SummaryReport {
    const real = Array.from({ length: count }, (_, i) => group(`${by}-${i}`, 10));
    const total = group("合计", 10 * count);
    return {
      ...summaryA,
      by,
      // 后端契约：groups 含合计行（即使没有任何真实类别）
      groups: [...real, total],
      totals: total,
    };
  }

  function mountWith(report: SummaryReport): VueWrapper {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(report);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    return mountDashboard();
  }

  it("zero_one_two_categories_have_explicit_rendering：各维度 0/1/2 类别渲染明确", async () => {
    const dims: Dim[] = ["day", "model", "project", "agent"];
    for (const dim of dims) {
      // 0 个真实类别（仅有合计）→ 不绘制图表，给明确空状态
      const w0 = mountWith(reportWith(0, dim));
      await flushPromises();
      expect(w0.findComponent(TrendChart).exists(), `${dim} 0 类别不应绘制图表`).toBe(false);
      expect(w0.find(".chart-empty").exists(), `${dim} 0 类别应有明确空状态`).toBe(true);
      w0.unmount();

      for (const n of [1, 2]) {
        const w = mountWith(reportWith(n, dim));
        await flushPromises();
        const chart = w.findComponent(TrendChart);
        expect(chart.exists(), `${dim} ${n} 类别应绘制图表`).toBe(true);
        // 传给图表的是真实类别（不含合计）
        expect((chart.props("groups") as Group[]).length).toBe(n);
        expect((chart.props("groups") as Group[]).every((x) => x.key !== "合计")).toBe(true);
        w.unmount();
      }
    }
  });

  it("chart_groups_identity_is_stable_across_unrelated_updates：无关更新不改 groups 身份", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    const before = w.findComponent(TrendChart).props("groups");
    expect(Array.isArray(before)).toBe(true);
    // 无关状态变化触发父级重渲染：不得生成新数组（否则图表被无谓重建）
    state(w)["showSourceDetails"] = true;
    await nextTick();
    const after = w.findComponent(TrendChart).props("groups");
    expect(after).toBe(before);
    w.unmount();
  });
});

// UX08：空状态标题对齐既有字阶（卡片级结论 17px/600/1.3/-0.01em）。
describe("Dashboard 空状态字阶（UX08）", () => {
  it("empty_title_uses_card_level_scale", () => {
    const block = /\.empty-title\s*\{([^}]*)\}/.exec(dashboardSource);
    expect(block, "必须存在 .empty-title 规则").not.toBeNull();
    const body = block![1];
    expect(body).toContain("font-size: 17px");
    expect(body).toContain("font-weight: 600");
    // 非标字阶（15px / 650）不得残留
    expect(body).not.toContain("15px");
    expect(body).not.toContain("650");
  });
});

// ── RC02：恢复游标隔离与过期重试（2026-10-08 复核） ──
// 复核缺陷：磁盘恢复的旧游标缺少"当前首页成功"门槛；过期错误条的"重试"
// 复用已完成的 beginPromise，继续请求同一过期 query（begin 次数不变）。
describe("Dashboard 恢复分页资格与过期恢复（RC02）", () => {
  function deferred<T>(): [Promise<T>, (v: T) => void] {
    let resolve!: (v: T) => void;
    const promise = new Promise<T>((r) => {
      resolve = r;
    });
    return [promise, resolve];
  }

  function row(ts: string, rid: string) {
    return {
      ts,
      record_id: rid,
      cursor: `${ts}T00:00:00.000Z|${rid}`,
      agent: "codex",
      model: "claude-sonnet-4-5",
      session_id: "s",
      project: "p",
      input: 1,
      output: 1,
      cache_write: 0,
      cache_read: 0,
      cost_usd: 0,
    };
  }

  /** 恢复用旧视图：与 snapshotPayload() 的筛选一致，1 行但 total 5（有余量）。 */
  function restoredPayload() {
    const p = snapshotPayload();
    return {
      ...p,
      events: {
        query_id: "q-old-session",
        pricing_revision: "rev-test",
        rows: [row("2026-10-04 08:00:00", "old")],
        total: 5,
        warnings: [],
      },
    };
  }

  function eventsCalls() {
    return invokeMock.mock.calls
      .filter((c) => c[0] === "query_events")
      .map((c) => c[1] as { queryId: string; before: string | null });
  }

  function beginCalls(): number {
    return invokeMock.mock.calls.filter((c) => c[0] === "query_begin").length;
  }

  function retryButtonFor(w: VueWrapper, noticeText: string) {
    const notice = w.findAll(".ts-notice").find((n) => n.text().includes(noticeText));
    return notice?.findAll("button").find((b) => b.text() === "重试");
  }

  it("restored_view_cannot_page_before_current_first_page_succeeds：恢复视图无 live 分页资格", async () => {
    const newHandle = { ...queryInfo, queryId: "q-new-session" };
    // 新批次首页在被显式放行前一律拒绝（恢复后的批次刷新也可能发起首页）。
    let allowFirstPage = false;
    invokeMock.mockImplementation((cmd: string, args?: { before?: string | null }) => {
      if (cmd === "query_begin") return Promise.resolve(newHandle);
      if (cmd === "view_cache_load") return Promise.resolve(restoredPayload());
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") {
        if (!args?.before && !allowFirstPage) return Promise.reject(new Error("first page down"));
        return Promise.resolve({
          query_id: "q-new-session",
          pricing_revision: "rev-test",
          rows: [row("2026-10-06 10:00:00", "a"), row("2026-10-06 09:00:00", "b")],
          total: 5,
          warnings: [],
        });
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    // 旧数据保留（stale 展示），失败状态可见
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["old"]);
    expect(state(w)["eventsError"]).toBe("first page down");
    expect(state(w)["liveFirstPage"]).toBeNull();
    // 恢复期间按钮禁用并说明原因（不能点击无反馈）
    expect(state(w)["moreBlockedHint"]).toBe("刷新完成后可继续加载");

    // 触发加载更多：不得发出任何带旧 before 的 query_events
    (state(w)["loadMoreEvents"] as () => void)();
    await flushPromises();
    for (const c of eventsCalls()) {
      expect(c.before, "恢复视图的旧游标不得用于续页").toBeNull();
    }
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["old"]);

    // 新首页成功后才可用它的末行游标分页
    allowFirstPage = true;
    (state(w)["retryAfterFailure"] as (s: string) => void)("events");
    await flushPromises();
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["a", "b"]);
    expect(state(w)["liveFirstPage"]).not.toBeNull();
    expect(state(w)["moreBlockedHint"]).toBeUndefined();

    (state(w)["loadMoreEvents"] as () => void)();
    await flushPromises();
    const last = eventsCalls().at(-1)!;
    expect(last.before, "续页必须用当前首页的末行游标").toBe("2026-10-06 09:00:00T00:00:00.000Z|b");
    w.unmount();
  });

  it("expired_retry_starts_one_query_for_summary_and_first_page：过期重试只建一个新批次", async () => {
    let beginSeq = 0;
    let eventsSeqN = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") {
        beginSeq += 1;
        return Promise.resolve({ ...queryInfo, queryId: `q-batch-${beginSeq}` });
      }
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") {
        eventsSeqN += 1;
        if (eventsSeqN === 2) {
          // 追加页过期（会话被淘汰）
          return Promise.reject(new Error("query_expired: 查询会话不存在或已失效，请刷新重试"));
        }
        return Promise.resolve({
          query_id: `q-batch-${beginSeq}`,
          pricing_revision: "rev-test",
          rows: [row("2026-10-06 10:00:00", "a")],
          total: 2,
          warnings: [],
        });
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect(beginCalls()).toBe(1);
    expect((state(w)["events"] as EventList).rows.length).toBe(1);

    // 加载更多 → 过期错误可见
    (state(w)["loadMoreEvents"] as () => void)();
    await flushPromises();
    expect(state(w)["eventsError"]).toContain("query_expired");
    expect(beginCalls()).toBe(1);

    // 点击明细错误条"重试"：严格只新增一次 begin，汇总与首页共享新会话
    const retry = retryButtonFor(w, "明细加载失败");
    expect(retry, "明细错误条必须有重试按钮").toBeDefined();
    await retry!.trigger("click");
    await flushPromises();
    expect(beginCalls(), "过期恢复必须只新增一次 query_begin").toBe(2);

    const after = eventsCalls().filter((c) => c.queryId === "q-batch-2");
    expect(after.length, "新会话必须重新取首页").toBeGreaterThanOrEqual(1);
    expect(after[0].before, "新批次首页不带旧 before").toBeNull();
    // 旧页不被追加到新批次
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["a"]);
    expect(state(w)["eventsError"]).toBeNull();
    // 汇总侧同样落在新会话上
    const summaryCalls = invokeMock.mock.calls.filter((c) => c[0] === "query_summary");
    expect((summaryCalls.at(-1)![1] as { queryId: string }).queryId).toBe("q-batch-2");
    w.unmount();
  });

  it("failed_begin_can_be_retried：begin 拒绝后重试不复用已失败 Promise", async () => {
    let beginSeq = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") {
        beginSeq += 1;
        if (beginSeq === 1) return Promise.reject(new Error("begin down"));
        return Promise.resolve({ ...queryInfo, queryId: "q-ok" });
      }
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") {
        return Promise.resolve({
          query_id: "q-ok",
          pricing_revision: "rev-test",
          rows: [row("2026-10-06 10:00:00", "a")],
          total: 1,
          warnings: [],
        });
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect(beginCalls()).toBe(1);
    expect(state(w)["summaryError"]).toBe("begin down");

    const retry = retryButtonFor(w, "汇总加载失败");
    expect(retry).toBeDefined();
    await retry!.trigger("click");
    await flushPromises();
    expect(beginCalls(), "重试必须真正重发 begin").toBe(2);
    expect(state(w)["report"]).not.toBeNull();
    expect(state(w)["summaryError"]).toBeNull();
    w.unmount();
  });

  it("late_expired_page_cannot_replace_refreshed_results：晚到的旧分页不覆盖新批次", async () => {
    const [latePage, resolveLatePage] = deferred<EventList>();
    let beginSeq = 0;
    let eventsSeqN = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") {
        beginSeq += 1;
        return Promise.resolve({ ...queryInfo, queryId: `q-batch-${beginSeq}` });
      }
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") {
        eventsSeqN += 1;
        if (eventsSeqN === 2) return latePage; // 旧批次的追加页挂起
        return Promise.resolve({
          query_id: `q-batch-${beginSeq}`,
          pricing_revision: "rev-test",
          rows: [row("2026-10-06 10:00:00", eventsSeqN === 1 ? "first" : "fresh")],
          total: 2,
          warnings: [],
        });
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["first"]);

    // 旧批次追加页在途
    (state(w)["loadMoreEvents"] as () => void)();
    await flushPromises();

    // 用户刷新建立新批次并成功落地
    (state(w)["manualRefresh"] as () => void)();
    await flushPromises();
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["fresh"]);

    // 旧批次的过期分页晚到：不得追加、不得覆盖新批次结果
    resolveLatePage({
      query_id: "q-batch-1",
      pricing_revision: "rev-test",
      rows: [row("2026-10-01 00:00:00", "stale")],
      total: 2,
      warnings: [],
    });
    await flushPromises();
    expect((state(w)["events"] as EventList).rows.map((r) => r.record_id)).toEqual(["fresh"]);
    w.unmount();
  });
});

// ── AP06：来源检测的失败可见性、可重试与刷新重查 ──
// 复核缺陷：`void loadSources()` 没有 catch——source_status 拒绝会成为未处理
// Promise，界面既没有错误也没有重试；且来源状态只在挂载时读一次，用户把
// 目录修好后点"刷新"，旧 missing/empty 通知仍然挂着。
describe("Dashboard 来源检测失败（AP06）", () => {
  function statusProbe(): { reasons: unknown[]; stop: () => void } {
    const reasons: unknown[] = [];
    const onNode = (r: unknown): void => {
      reasons.push(r);
    };
    const proc = (
      globalThis as unknown as {
        process?: {
          on: (e: string, cb: (r: unknown) => void) => void;
          off: (e: string, cb: (r: unknown) => void) => void;
        };
      }
    ).process;
    proc?.on("unhandledRejection", onNode);
    return {
      reasons,
      stop: () => proc?.off("unhandledRejection", onNode),
    };
  }

  async function settle(): Promise<void> {
    await flushPromises();
    await new Promise((r) => setTimeout(r, 0));
    await flushPromises();
  }

  function retryIn(w: VueWrapper, noticeText: string) {
    const notice = w.findAll(".ts-notice").find((n) => n.text().includes(noticeText));
    return notice?.findAll("button").find((b) => b.text().includes("重试"));
  }

  const readyClaude = {
    agent: "claude-code",
    dir: "C:/claude",
    enabled: true,
    exists: true,
    files: 3,
    state: "ready",
  };
  const missingCodex = {
    agent: "codex",
    dir: "C:/codex",
    enabled: true,
    exists: false,
    files: 0,
    state: "missing",
  };

  it("source_status_failure_is_visible_and_retryable：失败可见可重试且不产生未处理拒绝", async () => {
    const probe = statusProbe();
    let statusCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") {
        statusCalls += 1;
        return statusCalls === 1
          ? Promise.reject(new Error("设置解析失败: settings.toml"))
          : Promise.resolve([readyClaude]);
      }
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await settle();
    expect(probe.reasons, "source_status 拒绝不得成为未处理 rejection").toEqual([]);
    probe.stop();

    expect(w.text()).toContain("来源目录检测失败");
    expect(w.text()).toContain("设置解析失败: settings.toml");
    // 来源检测失败不得动已成功的汇总数据
    expect(state(w)["report"], "汇总结果必须保留").toBeTruthy();

    const retry = retryIn(w, "来源目录检测失败");
    expect(retry, "错误条必须有重试入口").toBeDefined();
    await retry!.trigger("click");
    await settle();
    expect(statusCalls).toBe(2);
    expect(state(w)["sourceError"], "重试成功后错误必须消失").toBeNull();
    expect((state(w)["sourceStatus"] as unknown[]).length).toBe(1);
    expect(w.text()).not.toContain("来源目录检测失败");
    w.unmount();
  });

  it("manual_refresh_rechecks_source_status：目录修复后刷新使旧通知消失", async () => {
    let statusCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") {
        statusCalls += 1;
        return Promise.resolve(statusCalls === 1 ? [missingCodex] : [readyClaude]);
      }
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect(w.text()).toContain("数据目录不存在");

    const refresh = w.findAll("button").find((b) => b.text().trim() === "刷新");
    expect(refresh).toBeDefined();
    await refresh!.trigger("click");
    await flushPromises();
    expect(statusCalls, "手动刷新必须重查来源状态").toBe(2);
    expect(w.text(), "目录恢复后旧通知必须消失").not.toContain("数据目录不存在");
    w.unmount();
  });

  it("failed_status_read_marks_previous_state_stale：读取失败不把旧状态标为最新", async () => {
    let statusCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(queryInfo);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(summaryA);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "source_status") {
        statusCalls += 1;
        return statusCalls === 1
          ? Promise.resolve([missingCodex])
          : Promise.reject(new Error("读取超时"));
      }
      return Promise.resolve(null);
    });
    const w = mountDashboard();
    await flushPromises();
    expect(w.text()).toContain("数据目录不存在");

    const refresh = w.findAll("button").find((b) => b.text().trim() === "刷新")!;
    await refresh.trigger("click");
    await flushPromises();
    expect(w.text()).toContain("来源目录检测失败");
    expect(w.text(), "必须标注旧状态可能过期，而不是当作最新").toContain("可能已过期");
    expect(w.text()).toContain("读取超时");
    w.unmount();
  });
});
