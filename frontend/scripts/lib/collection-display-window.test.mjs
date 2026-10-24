// @vitest-environment node
import { describe, expect, it } from "vitest";
import { collectionDisplayWindow, legacyCollectionWindow } from "./collection-display-window.mjs";

const runId = 2;
const callbacks = (start = 100, end = 2600, phase = "failed") => [
  { kind: "status-enter", at_ms: start, phase: "running", runId },
  { kind: "status-exit", at_ms: start + 1, phase: "running", runId },
  { kind: "status-enter", at_ms: end, phase, runId },
  { kind: "status-exit", at_ms: end + 1, phase, runId },
];
const transitions = (start = 101, end = 2602, state = "failed") => [
  { at_ms: start - 1, state: "confirmed", runId: 1, initial: true },
  { at_ms: start, state: "reading", runId },
  { at_ms: end, state, runId },
];
const judge = (options = {}) =>
  collectionDisplayWindow({
    runId,
    callbacks: callbacks(),
    transitions: transitions(),
    ...options,
  });

describe("真实回调与精确显示边界（同一页面单调时钟）", () => {
  it("读取持续到失败回调处理结束后且真实轮次满 2300ms", () => {
    expect(judge()).toMatchObject({ ok: true, backendElapsedMs: 2502, displayMs: 2501 });
  });
  it("查询记录回填注册时刻，不参与状态回调的时间顺序检查", () => {
    expect(
      judge({ callbacks: [...callbacks(), { kind: "query", at_ms: 105, resolved_ms: 2700 }] }).ok,
    ).toBe(true);
  });
  it("界面在失败回调之前结束，即使窗口超过 2300ms 也失败", () => {
    expect(judge({ callbacks: callbacks(100, 2800) }).ok).toBe(false);
  });
  it("有启动延迟但持续到真实失败，不能把启动延迟误判成提前结束", () => {
    expect(judge({ transitions: transitions(350, 2602) })).toMatchObject({
      ok: true,
      displayMs: 2252,
      backendElapsedMs: 2502,
    });
  });
  it("失败后才开始显示 reading 不能冒充覆盖真实轮次", () => {
    expect(judge({ transitions: transitions(2700, 5100) }).ok).toBe(false);
  });
  it("真实开始至界面结束仍不得短于 2300ms", () => {
    expect(judge({ callbacks: callbacks(100, 2398), transitions: transitions(101, 2399) }).ok).toBe(
      false,
    );
  });
  it.each([undefined, null, []])("缺少或空的回调探针 %s 必须失败", (trace) => {
    expect(judge({ callbacks: trace }).ok).toBe(false);
  });
  it.each([undefined, null, []])("缺少或空的状态探针 %s 必须失败", (trace) => {
    expect(judge({ transitions: trace }).ok).toBe(false);
  });
  it("探针装在 reading 中途，初始快照不能冒充精确开始", () => {
    const trace = transitions();
    trace[1].initial = true;
    expect(judge({ transitions: trace }).ok).toBe(false);
  });
  it("同一轮读取中断再恢复，不能只取首尾掩盖中断", () => {
    const trace = transitions();
    trace.splice(
      2,
      0,
      { at_ms: 1000, state: "confirmed", runId },
      { at_ms: 1100, state: "reading", runId },
    );
    expect(judge({ transitions: trace }).ok).toBe(false);
  });
  it.each(["missing-start", "missing-exit", "wrong-run", "invalid-time", "reversed-time"])(
    "证据无效：%s",
    (kind) => {
      const trace = callbacks();
      if (kind === "missing-start") trace.shift();
      if (kind === "missing-exit") trace.pop();
      if (kind === "wrong-run") trace.forEach((s) => (s.runId = 3));
      if (kind === "invalid-time") trace[0].at_ms = NaN;
      if (kind === "reversed-time") trace.reverse();
      expect(judge({ callbacks: trace }).ok).toBe(false);
    },
  );
  it("恢复轮按精确的 UI 显示时长验收，2300ms 边界保留", () => {
    const options = { phase: "succeeded", callbacks: callbacks(100, 2600, "succeeded") };
    expect(judge({ ...options, transitions: transitions(302, 2602, "confirmed") }).ok).toBe(true);
    expect(judge({ ...options, transitions: transitions(303, 2602, "confirmed") }).ok).toBe(false);
  });
  it("恢复轮同样不能在完成回调前结束", () => {
    expect(
      judge({
        phase: "succeeded",
        callbacks: callbacks(100, 2800, "succeeded"),
        transitions: transitions(101, 2602, "confirmed"),
      }).ok,
    ).toBe(false);
  });
  it("39720973124 的旧证据只能给区间，不能据轮询追认新断言通过", () => {
    // 公开 CI 合成场景的原始数值；不含真实用户数据。
    const timeline = [
      { at_ms: 38490, page_ms: 5411.300000000047, state: "confirmed", runId: 1 },
      { at_ms: 39049, page_ms: 5561.300000000047, state: "reading", runId: 2 },
      { at_ms: 41232, page_ms: 7744.100000000093, state: "reading", runId: 2 },
      { at_ms: 41339, page_ms: 7851.900000000023, state: "failed", runId: 2 },
    ];
    const legacy = legacyCollectionWindow(timeline, runId);
    expect(legacy.observedDisplayMs).toBe(2290);
    expect(legacy.pageObservedDisplayMs).toBeCloseTo(2290.6);
    expect(legacy.displayLowerBoundMs).toBeCloseTo(2182.8);
    expect(legacy.displayUpperBoundMs).toBeCloseTo(2440.6);
    const result = judge({ callbacks: [], transitions: [] });
    expect(result.ok).toBe(false);
    expect(result.basis).toContain("缺失");
  });
});
