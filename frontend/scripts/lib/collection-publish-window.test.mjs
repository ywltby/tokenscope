import { describe, expect, it } from "vitest";
import { queryPublishesDuringReading } from "./collection-publish-window.mjs";

const baseline = { requests: 252, generation: 1, runId: 3, summaryQuery: "old" };
// 39363530556 的轮询形态：最后读取样本和落地只差 63ms，不能推断调用顺序。
const timeline = [
  { at_ms: 20892, state: "reading", runId: 4, generation: 1, requests: 252 },
  { at_ms: 21851, state: "reading", runId: 4, generation: 1, requests: 252 },
  {
    at_ms: 21914,
    state: "confirmed",
    runId: 4,
    generation: 2,
    requests: 253,
    summaryQuery: "new",
    eventsQuery: "new",
  },
];
const trace = (start, end, queryAt) => [
  { kind: "status-enter", at_ms: start, phase: "running", runId: 4, latestGeneration: 1 },
  { kind: "status-exit", at_ms: end, phase: "succeeded", runId: 4, latestGeneration: 2 },
  { kind: "query", at_ms: queryAt, resolved_ms: end + 20, queryId: "new", generation: 2 },
];
describe("查询不等待读取动画的直接证据", () => {
  it.each([50, 1000, 2500])("采集耗时 %dms：终态处理期间已经发起查询", (duration) => {
    const result = queryPublishesDuringReading({
      timeline,
      baseline,
      trace: trace(100, 100 + duration, 99 + duration),
    });
    expect(result.ok).toBe(true);
    expect(result.queryStartMs).toBe(99 + duration);
  });
  it("快轮次等待动画期满再查询必须失败", () => {
    expect(
      queryPublishesDuringReading({ timeline, baseline, trace: trace(100, 150, 1100) }).ok,
    ).toBe(false);
  });
  it("慢轮次结束后另加动画等待也必须失败", () => {
    expect(
      queryPublishesDuringReading({ timeline, baseline, trace: trace(100, 2600, 3600) }).ok,
    ).toBe(false);
  });
  it("仅有轮询、甚至落地仍为 reading，也不算调用证据", () => {
    expect(
      queryPublishesDuringReading({
        timeline: timeline.map((s) => ({ ...s, state: "reading" })),
        baseline,
      }).ok,
    ).toBe(false);
  });
  it.each([
    "wrong-query",
    "wrong-generation",
    "missing-status",
    "missing-start",
    "missing-publish",
  ])("证据不完整或串批次：%s", (kind) => {
    let records = trace(100, 150, 149);
    if (kind === "wrong-query") records[2].queryId = "other";
    if (kind === "wrong-generation") records[2].generation = 3;
    if (kind === "missing-status") records = records.filter((s) => s.kind !== "status-exit");
    if (kind === "missing-start") records = records.filter((s) => s.kind !== "status-enter");
    expect(
      queryPublishesDuringReading({
        timeline: kind === "missing-publish" ? [] : timeline,
        baseline,
        trace: records,
      }).ok,
    ).toBe(false);
  });
});
