// query_publishes_within_reading_window 判定的纯函数单测。
// 用例 1 原样回放 Native UI E2E 39177008549 失败运行的 timeline 形态；
// 用例 3 是反例：查询在读取态结束后才开始时必须判失败。
import { describe, expect, it } from "vitest";
import { queryPublishesDuringReading } from "./collection-publish-window.mjs";

/** 39177008549 launches[0].timeline 的关键取样（原值）：读取开始 →
 *  读取期内新代次查询开始（generation 前进 + "刷新中…"，统计未动）→
 *  落地与 1000ms 最短显示期同时到点（取样已翻到 confirmed）。 */
const FAILED_RUN_TIMELINE = [
  {
    at_ms: 20550,
    state: "confirmed",
    text: "已更新",
    revision: 6,
    runId: 3,
    generation: 1,
    requests: 252,
    summaryQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-0-g1",
    eventsQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-0-g1",
  },
  {
    at_ms: 21308,
    state: "reading",
    text: "正在读取",
    revision: 7,
    runId: 4,
    generation: 1,
    requests: 252,
    summaryQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-0-g1",
    eventsQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-0-g1",
  },
  {
    at_ms: 21773,
    state: "reading",
    text: "刷新中…",
    revision: 8,
    runId: 4,
    generation: 2,
    requests: 252,
    summaryQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-1-g2",
    eventsQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-1-g2",
  },
  {
    at_ms: 22290,
    state: "confirmed",
    text: "已更新",
    revision: 8,
    runId: 4,
    generation: 2,
    requests: 253,
    summaryQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-1-g2",
    eventsQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-1-g2",
  },
];

const BASELINE = {
  at_ms: 20400,
  state: "confirmed",
  text: "已更新",
  runId: 3,
  generation: 1,
  requests: 252,
  summaryQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-0-g1",
  eventsQuery: "q1ffbcc55d7da6f1bebbd91fcd1b407af-0-g1",
};

describe("query_publishes_within_reading_window 判定", () => {
  it("39177008549 失败运行形态：读取期内查询已开始 → 通过并写明依据", () => {
    const j = queryPublishesDuringReading({
      timeline: FAILED_RUN_TIMELINE,
      baseline: BASELINE,
    });
    expect(j.ok).toBe(true);
    expect(j.readingStartMs).toBe(21308);
    expect(j.queryStartMs).toBe(21773);
    expect(j.basis).toContain("读取开始=21308ms");
    expect(j.basis).toContain("查询开始=21773ms");
    expect(j.basis).toContain("落地=22290ms");
    expect(j.basis).toContain("落地时状态=confirmed/已更新");
  });

  it("查询快于最短显示期：落地取样仍是读取态 → 通过", () => {
    const timeline = [
      { at_ms: 100, state: "reading", text: "正在读取", runId: 2, generation: 1, requests: 10 },
      { at_ms: 500, state: "reading", text: "刷新中…", runId: 2, generation: 2, requests: 10 },
      { at_ms: 900, state: "reading", text: "刷新中…", runId: 2, generation: 2, requests: 11 },
    ];
    const j = queryPublishesDuringReading({
      timeline,
      baseline: { requests: 10, generation: 1, runId: 1 },
    });
    expect(j.ok).toBe(true);
    expect(j.publish.state).toBe("reading");
    expect(j.basis).toContain("落地时状态=reading/刷新中…");
  });

  it("反例：读取期内未见新代次查询、落地已是 confirmed → 失败", () => {
    const timeline = [
      { at_ms: 100, state: "reading", text: "正在读取", runId: 2, generation: 1, requests: 10 },
      { at_ms: 200, state: "reading", text: "正在读取", runId: 2, generation: 1, requests: 10 },
      { at_ms: 1500, state: "confirmed", text: "已更新", runId: 2, generation: 2, requests: 11 },
    ];
    const j = queryPublishesDuringReading({
      timeline,
      baseline: { requests: 10, generation: 1, runId: 1 },
    });
    expect(j.ok).toBe(false);
    expect(j.readingStartMs).toBe(100);
    expect(j.queryStartMs).toBeNull();
    expect(j.basis).toContain("读取期内未见新代次查询开始");
  });

  it("时间线中没有统计落地取样 → 失败", () => {
    const j = queryPublishesDuringReading({
      timeline: FAILED_RUN_TIMELINE.filter((s) => s.requests === 252),
      baseline: BASELINE,
    });
    expect(j.ok).toBe(false);
    expect(j.publish).toBeNull();
    expect(j.basis).toContain("没有统计落地取样");
  });
});
