import { describe, expect, it } from "vitest";
import type { EventRow } from "../types";
import { stableEventRowKey } from "./eventRowKey";

function baseRow(overrides: Partial<EventRow> = {}): EventRow {
  return {
    ts: "2026-10-07 10:48:00",
    record_id: "rid-1",
    cursor: JSON.stringify({
      v: 2,
      qid: "q-1",
      mfp: "m-1",
      dfp: "d-1",
      ts: "2026-10-07T02:48:00Z",
      rid: "rid-1",
      seq: 0,
    }),
    agent: "claude-code",
    model: "claude-sonnet-4-5",
    session_id: "s-1",
    project: "/workspace/app",
    input: 100,
    output: 50,
    cache_write: 0,
    cache_read: 0,
    cost_usd: 0.01,
    ...overrides,
  };
}

describe("stableEventRowKey（跨刷新行标识与会话游标解耦）", () => {
  it("原生请求优先使用后端 row_id，游标随 query_id 切换仍保持一致", () => {
    const q1 = baseRow({ row_id: "event:42" });
    const q2 = baseRow({
      row_id: "event:42",
      cursor: JSON.stringify({
        v: 2,
        qid: "q-2",
        mfp: "m-1",
        dfp: "d-1",
        ts: "2026-10-07T02:48:00Z",
        rid: "rid-1",
        seq: 5,
      }),
    });
    expect(q1.cursor).not.toBe(q2.cursor);
    expect(stableEventRowKey(q1)).toBe("event:42");
    expect(stableEventRowKey(q2)).toBe("event:42");
  });

  it("CCS 日汇总使用 row_id 或四元组，游标随 query_id 与 ridx 变化仍保持一致", () => {
    const rollupMeta = {
      day: "2026-10-05",
      source_tz: "Asia/Shanghai",
      source: "ccs",
      requests: 12,
      label: "Claude Opus 4.7",
      models: ["claude-opus-4-7"],
      cost_usd: 1.5,
      cost_detail: "rollup_only" as const,
      unknown_pricing: false,
      unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
      cost_partially_unknown: false,
      has_estimated_cost: true,
      cache_write_split_estimated: false,
      cache_price_zero_estimated: false,
    };
    const expectedId = `rollup:${JSON.stringify(["2026-10-05", "claudeopus47", "Asia/Shanghai", "claude-code"])}`;
    const q1 = baseRow({
      ts: "2026-10-05",
      record_id: "",
      model: "claudeopus47",
      row_id: expectedId,
      cursor: JSON.stringify({
        v: 2,
        qid: "q-1",
        mfp: "m-1",
        dfp: "d-1",
        ts: "2026-10-05",
        rid: "",
        seq: 0,
        kind: "rollup",
        ridx: 1,
      }),
      rollup: rollupMeta,
    });
    const q2 = baseRow({
      ts: "2026-10-05",
      record_id: "",
      model: "claudeopus47",
      row_id: undefined, // 旧快照缺 row_id 时回退四元组，与新批次 row_id 完全一致
      cursor: JSON.stringify({
        v: 2,
        qid: "q-2",
        mfp: "m-1",
        dfp: "d-1",
        ts: "2026-10-05",
        rid: "",
        seq: 0,
        kind: "rollup",
        ridx: 4,
      }),
      rollup: rollupMeta,
    });
    expect(q1.cursor).not.toBe(q2.cursor);
    expect(stableEventRowKey(q1)).toBe(expectedId);
    expect(stableEventRowKey(q2)).toBe(expectedId);
  });

  it("旧快照原生请求缺 row_id 时从 v2 JSON 游标剥离 qid，非 JSON 游标回退原串", () => {
    const q1 = baseRow({ row_id: undefined });
    const q2 = baseRow({
      row_id: undefined,
      cursor: JSON.stringify({
        v: 2,
        qid: "q-99",
        mfp: "m-9",
        dfp: "d-9",
        ts: "2026-10-07T02:48:00Z",
        rid: "rid-1",
        seq: 0,
      }),
    });
    expect(q1.cursor).not.toBe(q2.cursor);
    expect(stableEventRowKey(q1)).toBe(stableEventRowKey(q2));

    const plain = baseRow({ row_id: undefined, cursor: "2026-10-05T10:00:00.123450Z|m1" });
    expect(stableEventRowKey(plain)).toBe("2026-10-05T10:00:00.123450Z|m1");
  });
});
