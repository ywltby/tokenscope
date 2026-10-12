// U04：汇总页查询状态归属（`useUsageQuery`）的行为回归。
//
// 这一层是"页面上同时显示的汇总与明细永远来自同一批次、同一 query_id"的
// 唯一实现点，因此下列断言全部针对**跨代拼接**与**可见性合并**：
// 1. 同一 query_id 内连续切换下钻，旧明细晚到被丢弃；
// 2. 分页响应晚于筛选变化到达时不拼接；
// 3. 局部重试只重发失败的一侧；
// 4. 快照不得与新数据混代保存；
// 5. 隐藏期间多次触发只合并成一批；
// 6. 无待刷新标记时不发查询、筛选与下钻保留；
// 7. 快照只在"偏好 + 时区"逐项相等时用于首帧展示。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { defineComponent, h, nextTick } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { useUsageQuery, type UsageQueryApi } from "./useUsageQuery";
import { resetTimezoneForTests, useTimezone } from "./timezone";
import {
  resetSnapshotQueueForTests,
  snapshotQueueIdle,
  SNAPSHOT_VERSION,
} from "../lib/viewSnapshot";
import { VIEW_FILTERS_KEY } from "../lib/viewFilters";
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

const reportA: SummaryReport = {
  query_id: "q-1",
  pricing_revision: "rev-1",
  timezone: "Asia/Shanghai",
  generated_at: "2026-10-05T00:00:00+08:00",
  sources: [],
  by: "day",
  groups: [group("2026-10-01", 10)],
  totals: group("合计", 10),
  warnings: [],
};

const reportB: SummaryReport = {
  ...reportA,
  groups: [group("2026-10-02", 99)],
  totals: group("合计", 99),
};

function eventRow(rid: string, ts = "2026-10-02 10:00:00") {
  return {
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
  };
}

function listOf(rows: string[], total = rows.length): EventList {
  return {
    query_id: "q-1",
    pricing_revision: "rev-1",
    rows: rows.map((rid) => eventRow(rid)),
    total,
    warnings: [],
  };
}

const emptyList: EventList = listOf([], 0);

const handle = {
  queryId: "q-1",
  generation: 1,
  pricingRevision: "rev-1",
  timezone: "Asia/Shanghai",
  asOf: "2026-10-05T00:00:00+08:00",
  range: null,
};

function okMock(overrides: Partial<Record<string, () => Promise<unknown>>> = {}): void {
  invokeMock.mockImplementation((cmd: string) => {
    if (overrides[cmd]) return overrides[cmd]!();
    if (cmd === "query_begin") return Promise.resolve(handle);
    if (cmd === "view_cache_load") return Promise.resolve(null);
    if (cmd === "query_summary") return Promise.resolve(reportA);
    if (cmd === "query_events") return Promise.resolve(emptyList);
    if (cmd === "source_status") return Promise.resolve([]);
    return Promise.resolve(null);
  });
}

let api: UsageQueryApi;
let goSettingsCalls = 0;

const Host = defineComponent({
  props: { refreshKey: { type: Number, default: 0 } },
  setup(props) {
    api = useUsageQuery({
      refreshKey: () => props.refreshKey,
      goSettings: () => {
        goSettingsCalls += 1;
      },
    });
    return () => h("div", "host");
  },
});

function mountHost(refreshKey = 0): VueWrapper {
  return mount(Host, { props: { refreshKey } });
}

function callsOf(cmd: string): unknown[][] {
  return invokeMock.mock.calls.filter((c) => c[0] === cmd);
}

function beginCalls(): number {
  return callsOf("query_begin").length;
}

function savePayloads(): { value: Record<string, unknown> }[] {
  return callsOf("view_cache_save").map((c) => c[1] as { value: Record<string, unknown> });
}

beforeEach(() => {
  invokeMock.mockReset();
  localStorage.clear();
  resetSnapshotQueueForTests();
  resetTimezoneForTests();
  goSettingsCalls = 0;
});

describe("useUsageQuery 单一查询身份（U04）", () => {
  it("stale_drill_page_is_dropped_in_same_query_id：旧下钻明细晚到被丢弃，且不新建批次", async () => {
    const gates: ((v: EventList) => void)[] = [];
    okMock({
      query_events: () => new Promise<EventList>((r) => gates.push(r)),
    });
    const w = mountHost();
    await flushPromises();
    expect(gates.length, "首批明细已发出").toBe(1);

    api.drill.value = { type: "model", key: "m1" };
    await nextTick();
    await flushPromises();
    api.drill.value = { type: "model", key: "m2" };
    await nextTick();
    await flushPromises();
    expect(gates.length, "两次下钻各发一次明细").toBe(3);

    gates[2](listOf(["m2-row"]));
    await flushPromises();
    expect(api.events.value?.rows.map((r) => r.record_id)).toEqual(["m2-row"]);

    // 旧的两条明细晚到：不得覆盖当前下钻结果
    gates[1](listOf(["m1-row"]));
    gates[0](listOf(["all-row"]));
    await flushPromises();
    expect(api.events.value?.rows.map((r) => r.record_id)).toEqual(["m2-row"]);
    expect(beginCalls(), "下钻复用同一 query_id，不新建批次").toBe(1);
    w.unmount();
  });

  it("late_append_page_is_not_merged_after_filter_change：筛选变化后的旧追加页不被拼接", async () => {
    let appendGate: ((v: EventList) => void) | null = null;
    let eventsSeq = 0;
    invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "query_begin") return Promise.resolve(handle);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.resolve(reportA);
      if (cmd === "query_events") {
        eventsSeq += 1;
        if (eventsSeq === 2) return new Promise<EventList>((r) => (appendGate = r)); // 追加页挂起
        if (args?.before) return Promise.resolve(listOf(["appended"]));
        return Promise.resolve(listOf([eventsSeq === 1 ? "first" : "fresh"], 3));
      }
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountHost();
    await flushPromises();
    expect(api.events.value?.rows.map((r) => r.record_id)).toEqual(["first"]);
    expect(api.hasMore.value).toBe(true);

    api.loadMoreEvents();
    await flushPromises();

    // 筛选变化：新批次，明细回到第一页
    api.by.value = "model";
    await nextTick();
    await flushPromises();
    expect(api.events.value?.rows.map((r) => r.record_id)).toEqual(["fresh"]);

    // 旧批次的追加页晚到：不得追加、不得覆盖
    appendGate!(listOf(["appended"]));
    await flushPromises();
    expect(api.events.value?.rows.map((r) => r.record_id)).toEqual(["fresh"]);
    w.unmount();
  });

  it("local_retry_resends_only_failed_side：局部重试只重发失败的一侧", async () => {
    let failEvents = true;
    let failSummary = false;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(handle);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "source_status") return Promise.resolve([]);
      if (cmd === "query_summary") {
        return failSummary ? Promise.reject(new Error("sum boom")) : Promise.resolve(reportA);
      }
      if (cmd === "query_events") {
        return failEvents ? Promise.reject(new Error("ev boom")) : Promise.resolve(listOf(["a"]));
      }
      return Promise.resolve(null);
    });
    const w = mountHost();
    await flushPromises();
    expect(api.eventsError.value).toBe("ev boom");
    expect(beginCalls()).toBe(1);

    const summaryBefore = callsOf("query_summary").length;
    failEvents = false;
    api.retryAfterFailure("events");
    await flushPromises();
    expect(callsOf("query_summary").length, "明细局部重试不得重发汇总").toBe(summaryBefore);
    expect(api.eventsError.value).toBeNull();
    expect(api.events.value?.rows.map((r) => r.record_id)).toEqual(["a"]);

    const eventsBefore = callsOf("query_events").length;
    failSummary = true;
    api.manualRefresh();
    await flushPromises();
    expect(api.summaryError.value).toBe("sum boom");
    failSummary = false;
    const eventsAfterRefresh = callsOf("query_events").length;
    expect(eventsAfterRefresh, "手动刷新两侧各一次").toBeGreaterThan(eventsBefore);
    api.retryAfterFailure("summary");
    await flushPromises();
    expect(callsOf("query_events").length, "汇总局部重试不得重发明细").toBe(eventsAfterRefresh);
    expect(api.summaryError.value).toBeNull();
    w.unmount();
  });

  it("snapshot_never_mixes_generations：快照不落盘不同代的两侧", async () => {
    let summaryGate: ((v: SummaryReport) => void) | null = null;
    let summaryCalls = 0;
    let eventsCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(handle);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "source_status") return Promise.resolve([]);
      if (cmd === "query_summary") {
        summaryCalls += 1;
        if (summaryCalls === 1) return Promise.resolve(reportA);
        return new Promise<SummaryReport>((r) => (summaryGate = r));
      }
      if (cmd === "query_events") {
        eventsCalls += 1;
        return Promise.resolve(eventsCalls === 1 ? listOf(["old"]) : listOf(["new"]));
      }
      return Promise.resolve(null);
    });
    const w = mountHost();
    await flushPromises();
    await snapshotQueueIdle();
    expect(savePayloads().length, "首批两侧成功 → 落盘").toBe(1);

    // 新批次：明细成功、汇总仍挂起 → 不得把新一代明细与旧汇总拼成快照
    api.by.value = "model";
    await nextTick();
    await flushPromises();
    await snapshotQueueIdle();
    expect(savePayloads().length, "汇总未落地前不得落盘").toBe(1);

    summaryGate!({ ...reportB, by: "model" });
    await flushPromises();
    await snapshotQueueIdle();
    const last = savePayloads().at(-1)!.value as {
      filters: { by: string; preset: unknown };
      report: SummaryReport;
      events: EventList;
    };
    expect(last.report.by).toBe("model");
    expect(last.filters.by).toBe("model");
    expect(last.events.rows.map((r) => r.record_id)).toEqual(["new"]);
    w.unmount();
  });
});

describe("useUsageQuery 切页保留与待刷新标记（U04）", () => {
  it("hidden_triggers_coalesce_into_one_batch：隐藏期间多次触发只合并成一批", async () => {
    okMock();
    const w = mountHost();
    await flushPromises();
    expect(beginCalls()).toBe(1);

    api.setVisible(false);
    await nextTick();
    // 隐藏期间的多种触发：筛选、时区、refreshKey、程序触发的自动刷新
    api.by.value = "model";
    await nextTick();
    api.requestAutoRefresh();
    api.notifyUsageUpdated(7);
    await w.setProps({ refreshKey: 1 });
    useTimezone().tz.value = "UTC";
    await nextTick();
    await flushPromises();

    expect(beginCalls(), "隐藏期间不得发查询").toBe(1);
    expect(api.pendingRefresh.value, "必须记下待刷新标记").toBe(true);

    api.setVisible(true);
    await flushPromises();
    expect(beginCalls(), "回到页面只发一批").toBe(2);
    expect(api.pendingRefresh.value).toBe(false);
    expect(api.by.value, "筛选保留").toBe("model");
    expect(api.latestGeneration.value).toBe(7);

    api.setVisible(true);
    await flushPromises();
    expect(beginCalls(), "无待刷新标记的再次激活不重复查询").toBe(2);
    w.unmount();
  });

  it("activation_without_pending_marker_keeps_state_and_sends_nothing：无标记时切回不发查询且保留下钻", async () => {
    okMock();
    const w = mountHost();
    await flushPromises();
    const begins = beginCalls();
    api.drill.value = { type: "model", key: "m1", label: "M1" };
    await nextTick();
    await flushPromises();

    api.setVisible(false);
    await nextTick();
    api.setVisible(true);
    await flushPromises();
    expect(beginCalls(), "无待刷新标记时不发查询").toBe(begins);
    expect(api.drill.value?.key, "下钻保留").toBe("m1");
    expect(api.pendingRefresh.value).toBe(false);
    w.unmount();
  });

  it("filters_persist_across_instances：筛选偏好写盘，新实例同步读取", async () => {
    okMock();
    const w = mountHost();
    await flushPromises();
    api.by.value = "project";
    api.agent.value = "codex";
    api.preset.value = { kind: "days", days: 14 };
    api.setRollupsMode("detail_only");
    api.drill.value = { type: "day", key: "2026-10-01" };
    await nextTick();
    await flushPromises();
    const stored = JSON.parse(localStorage.getItem(VIEW_FILTERS_KEY)!);
    expect(stored).toMatchObject({
      by: "project",
      agent: "codex",
      preset: { kind: "days", days: 14 },
      rollupsMode: "detail_only",
      drill: { type: "day", key: "2026-10-01" },
    });
    w.unmount();

    // 新实例：偏好决定"查什么"（首批查询就带上下钻与维度）
    const w2 = mountHost();
    await flushPromises();
    expect(api.by.value).toBe("project");
    expect(api.agent.value).toBe("codex");
    expect(api.preset.value).toEqual({ kind: "days", days: 14 });
    expect(api.rollupsMode.value).toBe("detail_only");
    expect(api.drill.value?.key).toBe("2026-10-01");
    const begin = callsOf("query_begin").at(-1)![1] as Record<string, unknown>;
    expect(begin).toMatchObject({
      by: "project",
      agent: "codex",
      days: 14,
      rollups: "detail_only",
    });
    const events = callsOf("query_events").at(-1)![1] as Record<string, unknown>;
    expect(events).toMatchObject({ day: "2026-10-01" });
    w2.unmount();
  });
});

describe("useUsageQuery 快照首帧（U04）", () => {
  function snapshotPayload(report: SummaryReport, filters: Record<string, unknown>) {
    return {
      v: SNAPSHOT_VERSION,
      saved_at: "2026-10-05T00:00:00Z",
      filters,
      rollups_mode: "auto",
      report,
      events: listOf(["snap"]),
    };
  }

  it("snapshot_used_only_when_preferences_and_timezone_match：偏好与时区逐项相等才用于首帧", async () => {
    localStorage.setItem(
      VIEW_FILTERS_KEY,
      JSON.stringify({
        by: "model",
        agent: "claude",
        preset: null,
        rollupsMode: "auto",
        drill: { type: "model", key: "m1" },
      }),
    );
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(handle);
      if (cmd === "source_status") return Promise.resolve([]);
      if (cmd === "view_cache_load")
        return Promise.resolve(
          snapshotPayload(reportA, {
            by: "model",
            agent: "claude",
            preset: null,
            drill: { type: "model", key: "m1" },
          }),
        );
      if (cmd === "query_summary") return new Promise<SummaryReport>(() => {}); // 后台刷新挂起
      if (cmd === "query_events") return new Promise<EventList>(() => {});
      return Promise.resolve(null);
    });
    const w = mountHost();
    await flushPromises();
    expect(api.report.value, "首帧用快照兜底").not.toBeNull();
    expect(api.stale.value, "快照必须标记为过期").toBe(true);
    expect(api.by.value).toBe("model");
    expect(api.drill.value?.key).toBe("m1");
    w.unmount();
  });

  it("snapshot_ignored_when_timezone_differs：时区不同则快照不用于首帧（且不改写时区）", async () => {
    // 偏好与快照 filters 一致，但快照的统计时区是 Asia/Shanghai，当前时区 UTC。
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") return Promise.resolve({ timezone: "UTC" });
      if (cmd === "query_begin") return Promise.resolve({ ...handle, timezone: "UTC" });
      if (cmd === "source_status") return Promise.resolve([]);
      if (cmd === "view_cache_load")
        return Promise.resolve(
          snapshotPayload(reportA, { by: "day", agent: "all", preset: null, drill: null }),
        );
      if (cmd === "query_summary") return new Promise<SummaryReport>(() => {});
      if (cmd === "query_events") return new Promise<EventList>(() => {});
      return Promise.resolve(null);
    });
    const { initTimezone } = await import("./timezone");
    await initTimezone();
    await flushPromises();
    expect(useTimezone().tz.value).toBe("UTC");

    const w = mountHost();
    await flushPromises();
    expect(api.report.value, "时区不同的快照不得展示").toBeNull();
    expect(api.stale.value).toBe(false);
    expect(useTimezone().tz.value, "快照绝不改写时区").toBe("UTC");
    w.unmount();
  });
});

describe("useUsageQuery 通知合并（U04）", () => {
  it("notices_merge_into_one_list_with_actions：三类问题合并为一条列表，动作各自保留", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "query_begin") return Promise.resolve(handle);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "query_summary") return Promise.reject(new Error("sum boom"));
      if (cmd === "query_events") return Promise.reject(new Error("ev boom"));
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
    const w = mountHost();
    await flushPromises();
    const notices = api.notices.value;
    expect(notices.map((n) => n.text).join("\n")).toContain("汇总加载失败");
    expect(notices.map((n) => n.text).join("\n")).toContain("明细加载失败");
    expect(notices.map((n) => n.text).join("\n")).toContain("Codex 数据目录不存在");
    // 最高严重级别在首位
    expect(notices[0].severity).toBe("error");
    // 各自的动作都在自己的条目上
    const labels = notices.flatMap((n) => n.actions.map((a) => a.label));
    expect(labels).toContain("重试");
    expect(labels).toContain("去设置 ›");
    const goSettings = notices.find((n) => n.text.includes("数据目录不存在"))!.actions[0];
    goSettings.run();
    expect(goSettingsCalls).toBe(1);
    w.unmount();
  });

  it("clean_result_has_no_notice：干净结果没有通知（空区间不误报）", async () => {
    okMock();
    const w = mountHost();
    await flushPromises();
    expect(api.notices.value).toEqual([]);
    w.unmount();
  });

  it("instant_range_hint_enters_merged_notice：时刻模式提示进入合并通知", async () => {
    okMock({
      query_summary: () =>
        Promise.resolve({
          ...reportA,
          rollup_coverage: {
            detail_buckets: 1,
            rollup_buckets: 2,
            unresolved_buckets: 0,
            unresolved_covered_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
            rollups_excluded_for_instant_range: true,
          },
        }),
    });
    const w = mountHost();
    await flushPromises();
    const texts = api.notices.value.map((n) => n.text);
    expect(texts.some((t) => t.includes("精确到时刻"))).toBe(true);
    expect(api.collectionDiagnostics.value.clean, "时刻提示不是采集警告").toBe(true);
    w.unmount();
  });
});
