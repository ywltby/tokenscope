// @vitest-environment node
import { describe, expect, it } from "vitest";
import { ccsVerdict } from "./native-ccs-checks.mjs";

const counts = {
  requests: 11,
  rollups: 1,
  rollupRequests: 3,
  requestTokens: [547700, 2670, 240, 6130],
  rollupTokens: [900, 60, 10, 20],
};
const before = {
  ...counts,
  requests: 10,
  rollups: 0,
  rollupRequests: 0,
  requestTokens: [547000, 2580, 230, 6100],
  rollupTokens: [0, 0, 0, 0],
  batches: 0,
};
const imported = { ...counts, batches: 1 };
const preview = {
  plan_id: "new-plan",
  requests_total: 2,
  requests_importable: 2,
  rollups_total: 1,
};
const cases = [
  [
    "no_ccs_access_before_click",
    {
      beforeSettings: [{ event: "history_open" }],
      beforePreview: [{ event: "history_open" }],
      shownPath: "C:/isolated/sources/cc-switch/cc-switch.db",
      expectedPath: "C:/isolated/sources/cc-switch/cc-switch.db",
    },
    (o) => {
      o.beforePreview.push({ event: "ccs_open" });
    },
  ],
  [
    "preview_counts_match_fixture",
    { preview, text: "明细 2 条：可导入 2；日汇总 1 条" },
    (o) => {
      o.preview.requests_total = 1;
    },
  ],
  [
    "cancel_discards_without_writing",
    { before, after: structuredClone(before), discarded: true, planCleared: true, commits: 0 },
    (o) => {
      o.after.requests++;
    },
  ],
  [
    "confirm_imports_one_batch_as_union",
    {
      before,
      after: imported,
      report: { run_id: 1, requests_inserted: 1 },
      commits: 1,
      nativeOverlapRows: 1,
      ccsOnlyRows: 1,
      detailTotal: 11,
    },
    (o) => {
      o.nativeOverlapRows = 2;
    },
  ],
  [
    "repeat_import_adds_nothing",
    {
      before: imported,
      after: { ...counts, batches: 2 },
      report: { run_id: 2, requests_inserted: 0, requests_updated: 0 },
    },
    (o) => {
      o.after.rollupRequests++;
    },
  ],
  [
    "source_timezone_change_is_idempotent",
    {
      before: { ...counts, batches: 2 },
      after: { ...counts, batches: 3 },
      chosenTimezone: "UTC",
      preview: { ...preview, source_day_timezone: "UTC" },
      storedTimezones: ["UTC"],
    },
    (o) => {
      o.after.rollups++;
    },
  ],
  [
    "failure_recovers_by_new_preview",
    {
      failure: { status: "rejected", error: "missing isolated source" },
      errorText: "CCS 导入未完成：missing isolated source",
      planCleared: true,
      recovered: preview,
      previewVisible: true,
    },
    (o) => {
      o.planCleared = false;
    },
  ],
  [
    "detail_only_hides_rollups_and_discloses_granularity",
    {
      autoRequests: 14,
      detailRequests: 11,
      restoredRequests: 14,
      autoBy: "agent",
      detailBy: "agent",
      restoredBy: "agent",
      domMatches: [true, true, true],
      initialAutoMode: null,
      detailMode: "detail_only",
      autoMode: null,
      unknownText: "含只有日粒度的历史（这部分费用按未知披露）",
      detailText: "当前只统计请求明细（不含只有日粒度的历史）",
    },
    (o) => {
      o.detailRequests = 14;
    },
  ],
  [
    "ccs_fixture_unchanged",
    { before: "a".repeat(64), after: "a".repeat(64) },
    (o) => {
      o.after = "b".repeat(64);
    },
  ],
];

describe.each(cases)("%s", (name, good, corrupt) => {
  it("完整实测证据通过", () => {
    expect(ccsVerdict(name, structuredClone(good)).ok).toBe(true);
  });
  it("违反该要求的反例失败", () => {
    const bad = structuredClone(good);
    corrupt(bad);
    expect(ccsVerdict(name, bad).ok).toBe(false);
  });
  it("没有探针数据必须失败", () => {
    expect(ccsVerdict(name, {}).ok).toBe(false);
  });
});

it("时区冲突的按日排除不能冒充 detail_only 生效", () => {
  const observed = structuredClone(
    cases.find((row) => row[0] === "detail_only_hides_rollups_and_discloses_granularity")[1],
  );
  Object.assign(observed, {
    autoBy: "agent",
    detailBy: "day",
    restoredBy: "agent",
    domMatches: [true, true, true],
  });
  expect(ccsVerdict("detail_only_hides_rollups_and_discloses_granularity", observed).ok).toBe(
    false,
  );
});

it("三次同维度查询有任一次DOM未对应实际响应都不能通过", () => {
  for (const index of [0, 1, 2]) {
    const observed = structuredClone(
      cases.find((row) => row[0] === "detail_only_hides_rollups_and_discloses_granularity")[1],
    );
    observed.domMatches[index] = false;
    expect(ccsVerdict("detail_only_hides_rollups_and_discloses_granularity", observed).ok).toBe(
      false,
    );
  }
});
