import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import type { EventList, EventRow, Group, SummaryReport } from "../types";
import { useUsageQuery, type UsageQueryApi } from "./useUsageQuery";
import { resetTimezoneForTests } from "./timezone";
import { resetSnapshotQueueForTests } from "../lib/viewSnapshot";
import { VIEW_FILTERS_KEY, saveViewFilters } from "../lib/viewFilters";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));

const OLD = "opencodego/spacebunny";
const NEW = "spacebunny";
const initial = {
  by: "model" as const,
  agent: "claude" as const,
  preset: { kind: "days" as const, days: 7 as const },
  rollupsMode: "detail_only" as const,
  drill: { type: "model" as const, key: OLD, label: "Space Bunny" },
};
const tokens = { input: 100, output: 20, cache_write: 0, cache_read: 0 };
const model: Group = {
  key: NEW,
  label: "Space Bunny",
  requests: 1,
  tokens,
  cost_usd: 0.01,
  unknown_pricing: false,
  unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
};
const row: EventRow = {
  ts: "2026-11-20 10:00:00",
  record_id: "space-msg",
  cursor: "space-cursor",
  agent: "claude-code",
  model: NEW,
  session_id: "space-session",
  project: "synthetic",
  ...tokens,
  cost_usd: 0.01,
};
const handle = {
  queryId: "q-space",
  generation: 1,
  pricingRevision: "rev-space",
  timezone: "Asia/Shanghai",
  asOf: "2026-11-21T00:00:00+08:00",
  range: null,
};
const report: SummaryReport = {
  query_id: handle.queryId,
  pricing_revision: handle.pricingRevision,
  timezone: handle.timezone,
  generated_at: handle.asOf,
  by: "model",
  groups: [model],
  totals: model,
  sources: [],
  warnings: [],
};
let api: UsageQueryApi;
const wrappers: VueWrapper[] = [];
const Host = defineComponent({
  setup() {
    api = useUsageQuery({ refreshKey: () => 0 });
    return () => h("div");
  },
});

function boot() {
  const w = mount(Host);
  wrappers.push(w);
  return w;
}
function stubBackend(target: string | null = NEW, emptyRange = false) {
  invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "query_begin") return Promise.resolve(handle);
    if (cmd === "query_summary") return Promise.resolve(report);
    if (cmd === "query_events") {
      const migrating = args?.migrateLegacyModel === true;
      const key = migrating ? target : args?.model;
      const other = { ...row, model: "other", record_id: "other-msg", cursor: "other-cursor" };
      const rows = emptyRange
        ? []
        : key == null
          ? [row, other]
          : key === NEW
            ? [row]
            : key === "other"
              ? [other]
              : [];
      const list: EventList = {
        query_id: handle.queryId,
        pricing_revision: handle.pricingRevision,
        rows,
        total: rows.length,
        warnings: [],
      };
      return Promise.resolve({
        ...list,
        ...(migrating ? { model_filter_migration: { from: String(args?.model), to: target } } : {}),
      });
    }
    if (cmd === "source_status") return Promise.resolve([]);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  localStorage.clear();
  invokeMock.mockReset();
  resetTimezoneForTests();
  resetSnapshotQueueForTests();
});
afterEach(() => {
  for (const w of wrappers.splice(0)) w.unmount();
});

it("旧 Space Bunny 偏好首次打开必须从 0 行迁移到 1 行，不需要再点表格", async () => {
  localStorage.setItem(VIEW_FILTERS_KEY, JSON.stringify(initial));
  stubBackend();
  boot();
  await flushPromises();
  expect(api.events.value?.total, "旧键原样查询会返回 0 行，须显式请求一次迁移").toBe(1);
  expect(api.events.value?.rows[0].model).toBe(NEW);
  expect(api.drill.value?.key).toBe(NEW);
  expect(JSON.parse(localStorage.getItem(VIEW_FILTERS_KEY)!)).toMatchObject({
    modelKeyVersion: 1,
    drill: { key: NEW },
  });
  expect(invokeMock.mock.calls.filter(([cmd]) => cmd === "query_events")).toHaveLength(1);
  expect(invokeMock.mock.calls.find(([cmd]) => cmd === "query_begin")?.[1]).toMatchObject({
    by: "model",
    agent: "claude",
    days: 7,
    rollups: "detail_only",
    tz: "Asia/Shanghai",
  });
});

it.each(["missing", "ambiguous"])("旧关系 %s 时只清该模型下钻，正常返回未下钻明细", async () => {
  localStorage.setItem(VIEW_FILTERS_KEY, JSON.stringify(initial));
  stubBackend(null);
  boot();
  await flushPromises();
  expect(api.events.value?.total).toBe(2);
  expect(api.drill.value).toBeNull();
  const saved = JSON.parse(localStorage.getItem(VIEW_FILTERS_KEY)!);
  expect(saved).toMatchObject({ ...initial, drill: null, modelKeyVersion: 1 });
  expect(api.by.value).toBe(initial.by);
  expect(api.agent.value).toBe(initial.agent);
  expect(api.preset.value).toEqual(initial.preset);
  expect(api.rollupsMode.value).toBe(initial.rollupsMode);
});

it("唯一关系迁移后即使当前日期无请求也保留新筛选", async () => {
  localStorage.setItem(VIEW_FILTERS_KEY, JSON.stringify(initial));
  stubBackend(NEW, true);
  boot();
  await flushPromises();
  expect(api.events.value?.total).toBe(0);
  expect(api.drill.value?.key).toBe(NEW);
  expect(JSON.parse(localStorage.getItem(VIEW_FILTERS_KEY)!)).toMatchObject({
    modelKeyVersion: 1,
    drill: { key: NEW },
  });
});

it("迁移后的第二次启动直接用新键，不再请求兼容", async () => {
  localStorage.setItem(VIEW_FILTERS_KEY, JSON.stringify(initial));
  stubBackend();
  const first = boot();
  await flushPromises();
  first.unmount();
  wrappers.splice(wrappers.indexOf(first), 1);
  invokeMock.mockClear();
  boot();
  await flushPromises();
  const args = invokeMock.mock.calls.find(([cmd]) => cmd === "query_events")?.[1];
  expect(args).toMatchObject({ model: NEW });
  expect(args.migrateLegacyModel).toBeUndefined();
  expect(api.events.value?.total).toBe(1);
});

it.each([false, true])("新保存模型筛选正常恢复，日期为空=%s 也不清除", async (empty) => {
  saveViewFilters({ ...initial, drill: { ...initial.drill, key: NEW } });
  stubBackend(NEW, empty);
  boot();
  await flushPromises();
  expect(api.events.value?.total).toBe(empty ? 0 : 1);
  expect(api.drill.value?.key).toBe(NEW);
  const args = invokeMock.mock.calls.find(([cmd]) => cmd === "query_events")?.[1];
  expect(args.migrateLegacyModel).toBeUndefined();
});

it("新版偏好即使显式保存旧形键也不执行日常兼容匹配", async () => {
  saveViewFilters(initial);
  stubBackend();
  boot();
  await flushPromises();
  expect(api.events.value?.total).toBe(0);
  expect(api.drill.value?.key).toBe(OLD);
  expect(
    invokeMock.mock.calls.find(([cmd]) => cmd === "query_events")?.[1].migrateLegacyModel,
  ).toBeUndefined();
});

it("晚到的旧键迁移结果不能覆盖用户已选择的新模型", async () => {
  localStorage.setItem(VIEW_FILTERS_KEY, JSON.stringify(initial));
  stubBackend();
  const backend = invokeMock.getMockImplementation()!;
  let resolve!: (value: EventList) => void;
  invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "query_events" && args?.migrateLegacyModel)
      return new Promise<EventList>((r) => {
        resolve = r;
      });
    return backend(cmd, args);
  });
  boot();
  await flushPromises();
  api.drill.value = { type: "model", key: "other" };
  await flushPromises();
  resolve({
    query_id: handle.queryId,
    pricing_revision: handle.pricingRevision,
    rows: [row],
    total: 1,
    warnings: [],
    model_filter_migration: { from: OLD, to: NEW },
  });
  await flushPromises();
  expect(api.drill.value?.key).toBe("other");
  expect(api.events.value?.rows[0].model).toBe("other");
  expect(JSON.parse(localStorage.getItem(VIEW_FILTERS_KEY)!)).toMatchObject({
    modelKeyVersion: 1,
    drill: { key: "other" },
  });
});
