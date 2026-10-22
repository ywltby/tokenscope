// @vitest-environment node
import { describe, expect, it } from "vitest";
import { judgeIdentity } from "./native-identity-checks.mjs";

const C = "C:/acceptance/workspace/tokenscope";
const D = "D:/acceptance/workspace/tokenscope";
const B = "C:/acceptance/workspace/bee";
const tokens = { input: 100, output: 40, cache_write: 3, cache_read: 20 };
const totals = { requests: 10, tokens };
const snapshot = (overrides = {}) =>
  structuredClone({
    sameQuery: true,
    domMatches: true,
    summaryQuery: "q",
    eventsQuery: "q",
    eventArgs: { queryId: "q" },
    ...overrides,
    summary: {
      query_id: "q",
      by: "project",
      timezone: "UTC",
      groups: [],
      totals,
      ...overrides.summary,
    },
    events: { query_id: "q", rows: [], total: 10, ...overrides.events },
  });
const good = {
  project_same_path_merges_across_agents: {
    project: snapshot({
      summary: {
        groups: [{ key: C, label: "tokenscope", requests: 4, agents: ["claude-code", "codex"] }],
        totals,
      },
    }),
    detail: snapshot({
      events: { rows: [{ agent: "claude-code" }, { agent: "codex" }], total: 4 },
    }),
    detailText: "Claude Code Codex",
  },
  project_subdir_stays_in_root: {
    all: snapshot({
      events: {
        rows: [
          { record_id: "msg-acceptance-1", project: C, event_cwd: C },
          { record_id: "msg-acceptance-2", project: C, event_cwd: C + "/src" },
        ],
        total: 10,
      },
    }),
  },
  project_out_of_root_switches: {
    all: snapshot({
      events: {
        rows: [
          { record_id: "msg-acceptance-3", project: B, event_cwd: B },
          { record_id: "msg-acceptance-4", project: B, event_cwd: B + "/123" },
        ],
        total: 10,
      },
    }),
  },
  project_label_is_last_segment: {
    project: snapshot({
      summary: {
        groups: [
          { key: C, label: "tokenscope" },
          { key: D, label: "tokenscope" },
          { key: B, label: "bee" },
        ],
        totals,
      },
    }),
    visibleLabels: ["tokenscope", "tokenscope", "bee"],
  },
  model_variants_merge_with_friendly_name: {
    model: snapshot({
      summary: {
        groups: [{ key: "claudeopus55", label: "Synthetic Opus 5.5", requests: 6 }],
        totals,
      },
    }),
    visibleLabels: ["Synthetic Opus 5.5"],
    usageText: "Synthetic Opus 5.5",
  },
  model_detail_keeps_raw_names: {
    detail: snapshot({
      events: { rows: [{ model: "claude-opus-5-5" }, { model: "Claude_Opus_5.5" }], total: 6 },
    }),
    detailText: "claude-opus-5-5 Claude_Opus_5.5",
  },
  drill_from_day_model_project_rows: {
    drills: ["day", "model", "project"].map((type) => ({
      type,
      key: type === "day" ? "2026-10-06" : "selected",
      expectedRequests: 2,
      snapshot: snapshot({
        eventArgs: { queryId: "q", [type]: type === "day" ? "2026-10-06" : "selected" },
        events: { total: 2, rows: [{ [type]: "selected", ts: "2026-10-06T01:00:00Z" }] },
      }),
      filtered: true,
      cleared: snapshot(),
      clearedFilter: true,
    })),
  },
  source_file_deletion_keeps_history: {
    before: snapshot(),
    after: snapshot(),
    deletedFiles: ["synthetic.jsonl"],
    fileAbsent: true,
    deletedAtMs: 100,
    firstProcess: true,
    traceBoundarySeq: 5,
    backendTrace: [
      { seq: 1, event: "business_ready" },
      { seq: 2, event: "collect_round_start" },
      { seq: 3, event: "history_open" },
      { seq: 4, event: "collect_round_start" },
      { seq: 5, event: "source_root_read" },
      { seq: 6, event: "collect_round_start" },
    ],
    round: {
      start: { kind: "status-enter", phase: "running", at_ms: 101, runId: 3 },
      end: { kind: "status-exit", at_ms: 102, runId: 3, phase: "succeeded", latestGeneration: 3 },
    },
    queriedAtMs: 103,
    afterQueryCall: {
      command: "query_begin",
      status: "fulfilled",
      args: { collect: true },
      value: { queryId: "q", generation: 4 },
    },
  },
  rescan_and_rebuild_keep_history: {
    before: snapshot(),
    rescanned: snapshot(),
    rebuilt: snapshot(),
    rescanCall: { command: "refresh_cache", status: "fulfilled" },
    indexDeleted: true,
    indexRecreated: true,
    cleanExit: true,
  },
  source_disable_keeps_history: {
    disabledSwitch: true,
    disableCall: {
      command: "source_config_set",
      status: "fulfilled",
      args: { agent: "claude", enabled: false },
    },
    sourceRows: [{ agent: "claude", enabled: false }],
    claude: snapshot({
      summary: { totals: { requests: 8, tokens } },
      events: { rows: [{ agent: "claude-code" }], total: 8 },
    }),
    beginArgs: { agent: "claude" },
    beforeClaude: { requests: 8, tokens },
  },
};

describe("AC03 原生身份具名判定", () => {
  for (const [name, evidence] of Object.entries(good)) {
    it(`${name}：完整真实观察形状通过`, () => {
      expect(judgeIdentity(name, structuredClone(evidence)).ok).toBe(true);
    });
    it(`${name}：缺少探针或响应不冒充通过`, () => {
      expect(judgeIdentity(name, {}).ok).toBe(false);
    });
  }
  it("同名项目不可以合为一行", () => {
    const evidence = structuredClone(good.project_label_is_last_segment);
    evidence.project.summary.groups.splice(1, 1);
    expect(judgeIdentity("project_label_is_last_segment", evidence).ok).toBe(false);
  });
  it("只有第二条的子目录归属，不能证明初始根仍正确", () => {
    const evidence = structuredClone(good.project_subdir_stays_in_root);
    evidence.all.events.rows = evidence.all.events.rows.filter(
      (row) => row.record_id !== "msg-acceptance-1",
    );
    expect(judgeIdentity("project_subdir_stays_in_root", evidence).ok).toBe(false);
  });
  it("IPC筛选正确但DOM仍是旧全集或错误行时，三个具名下钻检查都失败", () => {
    for (const [name, property] of [
      ["project_same_path_merges_across_agents", "detail"],
      ["model_detail_keeps_raw_names", "detail"],
    ]) {
      const evidence = structuredClone(good[name]);
      evidence[property].domMatches = false;
      expect(judgeIdentity(name, evidence).ok).toBe(false);
    }
    const evidence = structuredClone(good.drill_from_day_model_project_rows);
    evidence.drills[0].snapshot.domMatches = false;
    expect(judgeIdentity("drill_from_day_model_project_rows", evidence).ok).toBe(false);
  });
  it("四桶任一变化，删除和重建历史检查都拒绝通过", () => {
    const deletion = structuredClone(good.source_file_deletion_keeps_history);
    deletion.after.summary.totals.tokens.cache_read++;
    expect(judgeIdentity("source_file_deletion_keeps_history", deletion).ok).toBe(false);
    const rebuild = structuredClone(good.rescan_and_rebuild_keep_history);
    rebuild.rebuilt.summary.totals.requests++;
    expect(judgeIdentity("rescan_and_rebuild_keep_history", rebuild).ok).toBe(false);
  });
  it("只有删除前的采集终态不能证明删除后的下一轮", () => {
    const evidence = structuredClone(good.source_file_deletion_keeps_history);
    evidence.round.start.at_ms = 99;
    evidence.backendTrace = evidence.backendTrace.filter(
      (row) => row.seq <= evidence.traceBoundarySeq,
    );
    expect(judgeIdentity("source_file_deletion_keeps_history", evidence).ok).toBe(false);
  });
  it("旧轮次回调晚送达，仍不能替代删除后真正开始的后端轮次", () => {
    const evidence = structuredClone(good.source_file_deletion_keeps_history);
    evidence.round.start.runId = 2;
    evidence.round.end.runId = 2;
    expect(judgeIdentity("source_file_deletion_keeps_history", evidence).ok).toBe(false);
  });
  it("后端start在删除边界之前或探针缺失，都不能证明新轮次", () => {
    for (const mutate of [
      (e) => (e.traceBoundarySeq = 6),
      (e) => delete e.backendTrace,
      (e) => (e.firstProcess = false),
      (e) => (e.afterQueryCall.args.collect = false),
      (e) => (e.afterQueryCall.value.queryId = "old"),
    ]) {
      const evidence = structuredClone(good.source_file_deletion_keeps_history);
      mutate(evidence);
      expect(judgeIdentity("source_file_deletion_keeps_history", evidence).ok).toBe(false);
    }
  });
  it("文件仍存在不能证明来源删除后历史保留", () => {
    const evidence = structuredClone(good.source_file_deletion_keeps_history);
    evidence.fileAbsent = false;
    expect(judgeIdentity("source_file_deletion_keeps_history", evidence).ok).toBe(false);
  });
  it("任一维度下钻参数、query_id 或清除结果不符即失败", () => {
    for (const mutate of [
      (e) => (e.drills[0].snapshot.eventArgs.day = null),
      (e) => (e.drills[1].snapshot.sameQuery = false),
      (e) => (e.drills[2].cleared.summary.totals.requests = 9),
    ]) {
      const evidence = structuredClone(good.drill_from_day_model_project_rows);
      mutate(evidence);
      expect(judgeIdentity("drill_from_day_model_project_rows", evidence).ok).toBe(false);
    }
  });
});
