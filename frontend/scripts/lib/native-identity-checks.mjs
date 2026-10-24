const C = "C:/acceptance/workspace/tokenscope";
const D = "D:/acceptance/workspace/tokenscope";
const B = "C:/acceptance/workspace/bee";
const buckets = ["input", "output", "cache_write", "cache_read"];
const rows = (snapshot) => snapshot?.events?.rows ?? [];
const groups = (snapshot) =>
  (snapshot?.summary?.groups ?? []).filter((group) => group.key !== "合计");
const same = (snapshot) =>
  Boolean(
    snapshot?.sameQuery &&
    snapshot.domMatches === true &&
    snapshot.summaryQuery &&
    snapshot.summaryQuery === snapshot.eventsQuery &&
    snapshot.summary?.query_id === snapshot.summaryQuery &&
    snapshot.events?.query_id === snapshot.eventsQuery,
  );
const totalsEqual = (a, b) =>
  Boolean(
    a &&
    b &&
    Number.isFinite(a.requests) &&
    a.requests === b.requests &&
    buckets.every(
      (bucket) => Number.isFinite(a.tokens?.[bucket]) && a.tokens[bucket] === b.tokens?.[bucket],
    ),
  );
const preserved = (before, after) =>
  same(before) &&
  same(after) &&
  before.events.total === after.events.total &&
  totalsEqual(before.summary.totals, after.summary.totals);
const raw = (snapshot, id) => rows(snapshot).find((row) => row.record_id === id);
const modelKey = (value) =>
  String(value)
    .replace(/[^a-z0-9]/gi, "")
    .toLowerCase();
function dayOf(row, timezone) {
  const date = new Date(row.ts);
  if (!Number.isFinite(date.valueOf())) return null;
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: timezone ?? "UTC",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(date);
  const part = (type) => parts.find((p) => p.type === type)?.value;
  return `${part("year")}-${part("month")}-${part("day")}`;
}

// 这些纯判定直接供原生场景使用；缺响应、缺回调或身份不一致一律失败。
export function judgeIdentity(name, evidence = {}) {
  let ok = false;
  const e = evidence;
  switch (name) {
    case "project_same_path_merges_across_agents": {
      const matches = groups(e.project).filter((group) => group.key === C);
      ok =
        same(e.project) &&
        same(e.detail) &&
        matches.length === 1 &&
        matches[0].requests === 4 &&
        ["claude-code", "codex"].every(
          (agent) =>
            matches[0].agents?.includes(agent) && rows(e.detail).some((row) => row.agent === agent),
        ) &&
        ["Claude Code", "Codex"].every((label) => e.detailText?.includes(label));
      break;
    }
    case "project_subdir_stays_in_root": {
      const first = raw(e.all, "msg-acceptance-1");
      const row = raw(e.all, "msg-acceptance-2");
      ok =
        same(e.all) &&
        first?.project === C &&
        first.event_cwd?.replaceAll("\\", "/") === C &&
        row?.project === C &&
        row.event_cwd?.replaceAll("\\", "/") === C + "/src";
      break;
    }
    case "project_out_of_root_switches": {
      const first = raw(e.all, "msg-acceptance-3");
      const child = raw(e.all, "msg-acceptance-4");
      ok =
        same(e.all) &&
        first?.project === B &&
        child?.project === B &&
        first.event_cwd?.replaceAll("\\", "/") === B &&
        child.event_cwd?.replaceAll("\\", "/") === B + "/123";
      break;
    }
    case "project_label_is_last_segment": {
      const list = groups(e.project);
      ok =
        same(e.project) &&
        [C, D].every((key) => list.filter((group) => group.key === key).length === 1) &&
        list.length > 1 &&
        list.every((group) => group.label === group.key.split("/").at(-1)) &&
        list.every((group) => e.visibleLabels?.includes(group.label)) &&
        e.visibleLabels?.filter((label) => label === "tokenscope").length === 2;
      break;
    }
    case "model_variants_merge_with_friendly_name": {
      const matches = groups(e.model).filter((group) => group.key === "claudeopus55");
      ok =
        same(e.model) &&
        matches.length === 1 &&
        matches[0].requests === 6 &&
        matches[0].label === "Synthetic Opus 5.5" &&
        e.visibleLabels?.includes("Synthetic Opus 5.5") &&
        typeof e.usageText === "string" &&
        !e.usageText.includes("claudeopus55");
      break;
    }
    case "model_detail_keeps_raw_names":
      ok =
        same(e.detail) &&
        e.detail.events.total === 6 &&
        ["claude-opus-5-5", "Claude_Opus_5.5"].every(
          (model) =>
            rows(e.detail).some((row) => row.model === model) && e.detailText?.includes(model),
        );
      break;
    case "drill_from_day_model_project_rows":
      ok =
        Array.isArray(e.drills) &&
        e.drills.length === 3 &&
        ["day", "model", "project"].every((type) => {
          const drill = e.drills.find((item) => item.type === type);
          const snapshot = drill?.snapshot;
          return Boolean(
            drill &&
            same(snapshot) &&
            snapshot.eventArgs?.[type] === drill.key &&
            drill.filtered &&
            snapshot.events.total === drill.expectedRequests &&
            rows(snapshot).length > 0 &&
            rows(snapshot).every((row) =>
              type === "day"
                ? dayOf(row, snapshot.summary.timezone) === drill.key
                : type === "model"
                  ? modelKey(row.model) === drill.key
                  : row.project === drill.key,
            ) &&
            same(drill.cleared) &&
            drill.cleared.summary.totals.requests === 10 &&
            drill.cleared.events.total === 10 &&
            drill.clearedFilter,
          );
        });
      break;
    case "source_file_deletion_keeps_history": {
      // 首进程内每个真实领队恰有一次 begin_run + collect_round_start；
      // ordinal 对应 runId，不把旧轮次晚送达的页面时间当作后端起点。
      const starts = (e.backendTrace ?? []).filter((row) => row.event === "collect_round_start");
      const next = starts.find((row) => row.seq > e.traceBoundarySeq);
      const runId = next ? starts.indexOf(next) + 1 : null;
      ok =
        preserved(e.before, e.after) &&
        e.deletedFiles?.length === 1 &&
        e.fileAbsent === true &&
        e.firstProcess === true &&
        e.backendTrace?.filter((row) => row.event === "business_ready").length === 1 &&
        Number.isSafeInteger(e.traceBoundarySeq) &&
        e.traceBoundarySeq >= 0 &&
        Boolean(next) &&
        Number.isFinite(e.deletedAtMs) &&
        e.round?.start?.kind === "status-enter" &&
        e.round.start.phase === "running" &&
        e.round.start.runId === runId &&
        e.round.end?.runId === runId &&
        e.round.end.kind === "status-exit" &&
        e.round.end.phase === "succeeded" &&
        e.round.end.at_ms >= e.round.start.at_ms &&
        e.queriedAtMs >= e.round.end.at_ms &&
        e.afterQueryCall?.command === "query_begin" &&
        e.afterQueryCall.status === "fulfilled" &&
        e.afterQueryCall.args?.collect === true &&
        e.afterQueryCall.value?.queryId === e.after.summaryQuery &&
        Number.isFinite(e.round.end.latestGeneration) &&
        e.afterQueryCall.value.generation >= e.round.end.latestGeneration;
      break;
    }
    case "rescan_and_rebuild_keep_history":
      ok =
        preserved(e.before, e.rescanned) &&
        preserved(e.before, e.rebuilt) &&
        e.rescanCall?.command === "refresh_cache" &&
        e.rescanCall.status === "fulfilled" &&
        e.indexDeleted === true &&
        e.indexRecreated === true &&
        e.cleanExit === true;
      break;
    case "source_disable_keeps_history":
      ok =
        e.disabledSwitch === true &&
        e.disableCall?.command === "source_config_set" &&
        e.disableCall.status === "fulfilled" &&
        e.disableCall.args?.agent === "claude" &&
        e.disableCall.args.enabled === false &&
        e.sourceRows?.some((row) => row.agent === "claude" && row.enabled === false) &&
        same(e.claude) &&
        e.beginArgs?.agent === "claude" &&
        e.claude.summary.totals.requests === 8 &&
        totalsEqual(e.beforeClaude, e.claude.summary.totals) &&
        e.claude.events.total === 8 &&
        rows(e.claude).length > 0 &&
        rows(e.claude).every((row) => row.agent === "claude-code");
      break;
    default:
      throw new Error(`未知 AC03 断言：${name}`);
  }
  return { ok: Boolean(ok), basis: JSON.stringify(e) };
}
