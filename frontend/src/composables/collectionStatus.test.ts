// PR2（2026-10-15 计划 F03）：collectionStatus 组合式的行为回归。
//
// 覆盖计划 §6.3 反例表的可控时钟场景：最短 1 秒显示期、慢轮次持续到真实
// 完成、连续两轮不中断、查询不等待动画、代次去重、旧 revision 不倒退、
// 失败立即停止、隐藏/恢复不补播、卸载清理、通道不可用与重试。
// 1000ms 精确阈值由这里的可控单调时钟证明；原生 E2E 只按低频轮询取证。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, ref } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
const listenMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  useCollectionStatus,
  type CollectionPillView,
  type CollectionStatusPayload,
} from "./collectionStatus";
import type { NoticeItem } from "./useUsageQuery";

/// 可控单调时钟：测试手动推进（与 vi 的假定时器同步走同一个增量）。
let now = 0;
const fakeNow = () => now;
/** 推进 `ms`：先走单调时钟，再把假定时器推进同样的毫秒。 */
async function tick(ms: number): Promise<void> {
  now += ms;
  await vi.advanceTimersByTimeAsync(ms);
}

function status(overrides: Partial<CollectionStatusPayload>): CollectionStatusPayload {
  return {
    revision: 1,
    runId: 1,
    phase: "running",
    trigger: "auto",
    latestGeneration: null,
    changed: false,
    hasWarnings: false,
    lastCompletedAt: null,
    error: null,
    ...overrides,
  };
}

function makeHarness() {
  const visible = ref(true);
  const pill = ref<CollectionPillView | null>(null);
  const notice = ref<NoticeItem | null>(null);
  const generations: number[] = [];
  const manualRefreshes: number[] = [];
  let emitHandler: ((s: CollectionStatusPayload) => void) | null = null;
  const unlistens: number[] = [];
  let queryImpl: () => Promise<CollectionStatusPayload> = () =>
    Promise.reject(new Error("未配置状态查询"));
  let subscribeImpl: (handler: (s: CollectionStatusPayload) => void) => Promise<() => void> = (
    handler,
  ) => {
    emitHandler = handler;
    return Promise.resolve(() => {
      unlistens.push(1);
      emitHandler = null;
    });
  };

  const Host = defineComponent({
    setup() {
      useCollectionStatus({
        visible,
        onGeneration: (g) => generations.push(g),
        onManualRefresh: () => manualRefreshes.push(1),
        pillOut: pill,
        noticeOut: notice,
        nowMs: fakeNow,
        queryStatus: () => queryImpl(),
        subscribe: (h) => subscribeImpl(h),
      });
      return () => h("div", "host");
    },
  });

  return {
    visible,
    pill,
    notice,
    generations,
    manualRefreshes,
    unlistens,
    emit: (s: CollectionStatusPayload) => emitHandler?.(s),
    setQuery: (impl: () => Promise<CollectionStatusPayload>) => {
      queryImpl = impl;
    },
    setSubscribe: (impl: typeof subscribeImpl) => {
      subscribeImpl = impl;
    },
    wrapper: mount(Host),
  };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
  now = 0;
  invokeMock.mockReset();
  listenMock.mockReset();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

afterEach(() => {
  vi.useRealTimers();
});

describe("collectionStatus 最短显示期（可控时钟）", () => {
  it("fast_round_shows_at_least_1000ms：50ms 完成仍显示到 1 秒，且不发任何查询", async () => {
    const h = makeHarness();
    await flushPromises();
    expect(h.pill.value?.text).toBe("正在确认状态");

    h.emit(status({ revision: 1, phase: "running", runId: 1 }));
    await flushPromises();
    expect(h.pill.value).toMatchObject({ text: "正在读取", spin: true });

    // t=50ms：后端已完成（无变化）——显示期未满，继续读取反馈。
    h.emit(status({ revision: 2, phase: "succeeded", runId: 1, changed: false }));
    await flushPromises();
    await tick(999); // start 起第 999ms（running 在 t=0 可见）
    expect(h.pill.value?.text).toBe("正在读取");
    await tick(1); // t=1000ms
    expect(h.pill.value?.text).toBe("已更新");
    expect(h.pill.value?.spin).toBe(false);
    // 无新用量：零查询、零代次转发（统计刷新与动画完全解耦）。
    expect(invokeMock.mock.calls.length).toBe(0);
    expect(h.generations).toEqual([]);
    h.wrapper.unmount();
  });

  it("slow_round_stops_at_real_end_not_plus_1000ms：3 秒完成只等到真实结束", async () => {
    const h = makeHarness();
    await flushPromises();
    h.emit(status({ revision: 1, phase: "running", runId: 1 }));
    await flushPromises();
    await tick(1000);
    expect(h.pill.value?.text).toBe("正在读取");
    await tick(1500);
    expect(h.pill.value?.text).toBe("正在读取");
    // t=3000ms 真实结束：立即停止（不再多等 1 秒）。
    h.emit(
      status({
        revision: 2,
        phase: "succeeded",
        runId: 1,
        changed: false,
        lastCompletedAt: new Date(Date.UTC(2026, 9, 15, 0, 1, 0)).toISOString(),
      }),
    );
    await flushPromises();
    expect(h.pill.value?.text).toBe("已更新");
    expect(h.pill.value?.recent).toContain("最近读取");
    h.wrapper.unmount();
  });

  it("back_to_back_rounds_stay_continuous：1 秒内两轮连续显示不闪断", async () => {
    const h = makeHarness();
    await flushPromises();
    h.emit(status({ revision: 1, phase: "running", runId: 1 }));
    await flushPromises();
    await tick(500);
    // 第二轮 start：不重置显示期，只延长截止。
    h.emit(status({ revision: 2, phase: "running", runId: 2 }));
    await flushPromises();
    expect(h.pill.value?.text).toBe("正在读取");
    h.emit(status({ revision: 3, phase: "succeeded", runId: 2, changed: false }));
    await flushPromises();
    await tick(999); // 第二轮 start 后 999ms
    expect(h.pill.value?.text).toBe("正在读取");
    await tick(1);
    expect(h.pill.value?.text).toBe("已更新");
    h.wrapper.unmount();
  });

  it("query_does_not_wait_for_animation：有新数据立即转发代次，反馈继续到期满", async () => {
    const h = makeHarness();
    await flushPromises();
    h.emit(status({ revision: 1, phase: "running", runId: 1 }));
    await flushPromises();
    await tick(50);
    // t=50ms 有新事实并结束：代次立即转发（统计刷新不等动画）。
    h.emit(
      status({ revision: 2, phase: "succeeded", runId: 1, changed: true, latestGeneration: 5 }),
    );
    await flushPromises();
    expect(h.generations).toEqual([5]);
    // t=100ms 查询"完成"（真实接线由 useUsageQuery 负责）——读取反馈继续。
    await tick(50);
    expect(h.pill.value?.text).toBe("正在读取");
    await tick(900);
    expect(h.pill.value?.text).toBe("已更新");
    h.wrapper.unmount();
  });

  it("same_generation_is_forwarded_once：状态恢复与 usage-updated 交叉只建一个批次", async () => {
    const h = makeHarness();
    await flushPromises();
    h.emit(
      status({ revision: 1, phase: "succeeded", runId: 1, changed: true, latestGeneration: 9 }),
    );
    h.emit(
      status({ revision: 2, phase: "succeeded", runId: 1, changed: true, latestGeneration: 9 }),
    );
    await flushPromises();
    expect(h.generations).toEqual([9]);
    // 更高代次只补一次。
    h.emit(
      status({ revision: 3, phase: "succeeded", runId: 2, changed: true, latestGeneration: 10 }),
    );
    await flushPromises();
    expect(h.generations).toEqual([9, 10]);
    h.wrapper.unmount();
  });

  it("older_revision_is_ignored：旧快照晚到不倒退阶段、不重新计时", async () => {
    const h = makeHarness();
    await flushPromises();
    h.emit(status({ revision: 3, phase: "succeeded", runId: 2, changed: false }));
    await flushPromises();
    h.emit(status({ revision: 2, phase: "running", runId: 1 }));
    await flushPromises();
    expect(h.pill.value?.text).toBe("已更新");
    h.wrapper.unmount();
  });
});

describe("collectionStatus 失败、隐藏与清理", () => {
  it("failed_stops_immediately_and_recovers_on_next_success：失败不等最短期，且只清采集错误", async () => {
    const h = makeHarness();
    await flushPromises();
    h.emit(status({ revision: 1, phase: "running", runId: 1 }));
    await flushPromises();
    await tick(50);
    h.emit(
      status({
        revision: 2,
        phase: "failed",
        runId: 1,
        error: "来源目录冲突（合成）",
      }),
    );
    await flushPromises();
    expect(h.pill.value?.text).toBe("读取失败");
    expect(h.pill.value?.spin).toBe(false);
    expect(h.notice.value?.severity).toBe("error");
    expect(h.notice.value?.text).toContain("来源目录冲突");
    expect(h.notice.value?.actions[0]?.label).toBe("手动刷新");
    h.notice.value?.actions[0]?.run();
    expect(h.manualRefreshes.length).toBe(1);
    // 随后一次成功：清除采集错误（notice 由真实状态派生）。
    h.emit(status({ revision: 3, phase: "succeeded", runId: 2, changed: false }));
    await flushPromises();
    expect(h.notice.value).toBeNull();
    expect(h.pill.value?.text).toBe("已更新");
    h.wrapper.unmount();
  });

  it("hidden_rounds_are_not_replayed_and_running_resume_display：隐藏不计时，恢复按当前状态", async () => {
    const h = makeHarness();
    await flushPromises();
    h.emit(status({ revision: 1, phase: "running", runId: 1 }));
    await flushPromises();
    // 隐藏：清除显示计时（不补播已完成轮次）。
    h.visible.value = false;
    await flushPromises();
    expect(h.pill.value?.text).toBe("已更新");
    // 隐藏期间一轮完成 + 新一轮开始（真实状态照常更新）。
    h.emit(
      status({ revision: 2, phase: "succeeded", runId: 1, changed: true, latestGeneration: 3 }),
    );
    h.emit(status({ revision: 3, phase: "running", runId: 2 }));
    await flushPromises();
    await tick(5000);
    expect(h.pill.value?.text).not.toBe("正在读取");
    expect(h.generations).toEqual([3]);
    // 恢复：仍在 running 的轮次按当前运行状态展示（从现在起满足最短期）。
    h.visible.value = true;
    await flushPromises();
    expect(h.pill.value?.text).toBe("正在读取");
    h.emit(status({ revision: 4, phase: "succeeded", runId: 2, changed: false }));
    await flushPromises();
    await tick(1000);
    expect(h.pill.value?.text).toBe("已更新");
    h.wrapper.unmount();
  });

  it("dispose_clears_timers_and_unlistens_and_ignores_late_ipc：卸载无遗留", async () => {
    const h = makeHarness();
    await flushPromises();
    h.emit(status({ revision: 1, phase: "running", runId: 1 }));
    await flushPromises();
    h.wrapper.unmount();
    expect(h.unlistens.length).toBe(1);
    // 晚到的 IPC 与事件不得更新卸载后的组件。
    h.emit(status({ revision: 2, phase: "failed", runId: 1, error: "晚到" }));
    await tick(2000);
    expect(h.pill.value?.text).toBe("正在读取");
  });

  it("channel_failure_shows_unavailable_with_retry：订阅失败不得静默显示正常状态", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const h = makeHarness();
    h.setSubscribe(() => Promise.reject(new Error("通道不可用")));
    h.setQuery(() => Promise.reject(new Error("状态查询失败")));
    await flushPromises();
    expect(h.pill.value?.text).toBe("读取状态不可用");
    expect(h.notice.value?.severity).toBe("warning");
    expect(h.notice.value?.actions[0]?.label).toBe("重试");
    // 重试：查询恢复后回到正常状态。
    h.setQuery(() =>
      Promise.resolve(status({ revision: 1, phase: "succeeded", runId: 1, changed: false })),
    );
    h.notice.value?.actions[0]?.run();
    await flushPromises();
    expect(h.pill.value?.text).toBe("已更新");
    expect(h.notice.value).toBeNull();
    h.wrapper.unmount();
  });

  it("browser_preview_degrades_silently：非 Tauri 宿主的通道失败不显示不可用", async () => {
    const h = makeHarness();
    h.setSubscribe(() => Promise.reject(new Error("没有事件总线")));
    h.setQuery(() => Promise.reject(new Error("没有后端")));
    await flushPromises();
    expect(h.pill.value?.text).not.toBe("读取状态不可用");
    expect(h.notice.value).toBeNull();
    h.wrapper.unmount();
  });
});
