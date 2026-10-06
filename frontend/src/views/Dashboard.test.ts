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

// v3 快照样例（模块级：多个 describe 共用）
function snapshotPayload() {
  return {
    v: 3,
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

  it("view_cache_query_mismatch：v2 旧快照（ms range）不得当新数据展示", async () => {
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
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "summarize") return Promise.resolve(summaryA);
      if (cmd === "list_events") {
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
    const call = invokeMock.mock.calls.filter((c) => c[0] === "list_events").at(-1)![1] as Record<
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

describe("Dashboard 状态（设计系统 Task 7）", () => {
  it("空数据时有明确状态文案与下一步指引", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "summarize")
        return Promise.resolve({
          timezone: "UTC",
          generated_at: "t",
          sources: [],
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
          warnings: [],
        });
      if (cmd === "list_events") return Promise.resolve({ rows: [], total: 0, warnings: [] });
      if (cmd === "source_status") return Promise.resolve([]);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      return Promise.resolve(null);
    });
    const w = mount(Dashboard, { props: { refreshKey: 0 } });
    await flushPromises();
    expect(w.text()).toContain("暂无数据");
    expect(w.text()).toContain("调整时间范围");
  });
});

describe("Dashboard 来源筛选（设计系统 Task 2，图标 + 文字分段）", () => {
  it("来源是 radiogroup 分段控件：每项图标 + 文字，点击切换过滤", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    const group = w.find('[role="radiogroup"][aria-label="数据来源"]');
    expect(group.exists()).toBe(true);
    const items = group.findAll('[role="radio"]');
    expect(items.map((t) => t.text())).toEqual(["全部", "Claude Code", "Codex"]);
    expect(items[0].attributes("aria-checked")).toBe("true");
    // 图标 + 文字（AgentIcon 渲染在 .seg-icon 内），不再是纯图标方块
    expect(items[1].find(".seg-icon").exists()).toBe(true);
    await items[2].trigger("click");
    await flushPromises();
    expect(state(w)["agent"]).toBe("codex");
  });
});

describe("Dashboard 页头与通知（设计系统 Task 3）", () => {
  it("大标题 + 时区/日期摘要在左，状态胶囊与刷新在标题行右侧", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    expect(w.find(".page-title").text()).toBe("用量汇总");
    const head = w.find(".page-head");
    expect(head.find(".page-sub").text()).toContain("统计时区");
    const pill = head.find(".ts-status-pill");
    expect(pill.exists()).toBe(true);
    expect(pill.text()).toContain("已更新");
    expect(head.findAll("button").some((b) => b.text() === "刷新")).toBe(true);
  });

  it("stale 快照 → 标题行警告胶囊；后台刷新落地 → 成功胶囊", async () => {
    let resolveSummary!: (v: SummaryReport) => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "view_cache_load") return Promise.resolve(snapshotPayload());
      if (cmd === "summarize") return new Promise((r) => (resolveSummary = r));
      if (cmd === "list_events") return Promise.resolve(events);
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

  it("筛选顺序固定：来源 → 聚合维度 → 日期 → 时区", async () => {
    mockOk();
    const w = mountDashboard();
    await flushPromises();
    const src = w.find('[aria-label="数据来源"]').element;
    const dim = w.find('[aria-label="聚合维度"]').element;
    const range = w.findComponent({ name: "DateRangeSelect" }).element;
    const tz = w.find(".tz-select").element;
    const FOLLOWING = Node.DOCUMENT_POSITION_FOLLOWING;
    expect(src.compareDocumentPosition(dim) & FOLLOWING).toBeTruthy();
    expect(dim.compareDocumentPosition(range) & FOLLOWING).toBeTruthy();
    expect(range.compareDocumentPosition(tz) & FOLLOWING).toBeTruthy();
  });

  it("来源异常渲染为内联通知，去设置动作发出 go-settings", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "summarize") return Promise.resolve(summaryA);
      if (cmd === "list_events") return Promise.resolve(events);
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
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "summarize") return Promise.resolve(summaryA);
      if (cmd === "list_events") return Promise.resolve(events);
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
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "summarize") return Promise.reject(new Error("boom"));
      if (cmd === "list_events") return Promise.resolve(events);
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
