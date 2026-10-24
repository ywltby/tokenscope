// AC04：所有结论都由实际 UI/IPC/隔离库观察值计算，缺少证据不得通过。
const pathKey = (value) =>
  typeof value === "string" ? value.replaceAll("\\", "/").toLowerCase() : null;
const finiteCount = (value) => Number.isSafeInteger(value) && value >= 0;
const tokens = (value) => Array.isArray(value) && value.length === 4 && value.every(finiteCount);
const validStats = (value) =>
  value &&
  [value.requests, value.rollups, value.rollupRequests].every(finiteCount) &&
  tokens(value.requestTokens) &&
  tokens(value.rollupTokens);
const sameUsage = (a, b) =>
  validStats(a) &&
  validStats(b) &&
  a.requests === b.requests &&
  a.rollups === b.rollups &&
  a.rollupRequests === b.rollupRequests &&
  a.requestTokens.every((value, index) => value === b.requestTokens[index]) &&
  a.rollupTokens.every((value, index) => value === b.rollupTokens[index]);
const addedTokens = (a, b, delta) =>
  tokens(a) && tokens(b) && a.every((value, i) => b[i] === value + delta[i]);
const previewMatches = (value) =>
  Boolean(
    value?.plan_id &&
    value.requests_total === 2 &&
    value.requests_importable === 2 &&
    value.rollups_total === 1,
  );

export function ccsVerdict(name, observed) {
  const o = observed ?? {};
  let ok = false;
  switch (name) {
    case "no_ccs_access_before_click":
      ok =
        [o.beforeSettings, o.beforePreview].every(
          (rows) =>
            Array.isArray(rows) && rows.length > 0 && rows.every((row) => row.event !== "ccs_open"),
        ) &&
        Boolean(pathKey(o.expectedPath)) &&
        pathKey(o.shownPath) === pathKey(o.expectedPath);
      break;
    case "preview_counts_match_fixture": {
      const text = String(o.text ?? "").replace(/\s/g, "");
      ok = previewMatches(o.preview) && /明细2条/.test(text) && /日汇总1条/.test(text);
      break;
    }
    case "cancel_discards_without_writing":
      ok =
        sameUsage(o.before, o.after) &&
        finiteCount(o.before?.batches) &&
        o.after.batches === o.before.batches &&
        o.discarded === true &&
        o.planCleared === true &&
        o.commits === 0;
      break;
    case "confirm_imports_one_batch_as_union":
      ok =
        validStats(o.before) &&
        validStats(o.after) &&
        o.commits === 1 &&
        o.after.batches === o.before.batches + 1 &&
        o.report?.run_id > 0 &&
        o.report.requests_inserted === 1 &&
        o.after.requests === o.before.requests + 1 &&
        o.after.rollups === o.before.rollups + 1 &&
        o.after.rollupRequests === o.before.rollupRequests + 3 &&
        addedTokens(o.before.requestTokens, o.after.requestTokens, [700, 90, 10, 30]) &&
        addedTokens(o.before.rollupTokens, o.after.rollupTokens, [900, 60, 10, 20]) &&
        o.detailTotal === o.after.requests &&
        o.nativeOverlapRows === 1 &&
        o.ccsOnlyRows === 1;
      break;
    case "repeat_import_adds_nothing":
      ok =
        sameUsage(o.before, o.after) &&
        o.after.batches === o.before.batches + 1 &&
        o.report?.run_id > 0 &&
        o.report.requests_inserted === 0 &&
        o.report.requests_updated === 0;
      break;
    case "source_timezone_change_is_idempotent":
      ok =
        sameUsage(o.before, o.after) &&
        o.after.batches === o.before.batches + 1 &&
        o.chosenTimezone === "UTC" &&
        previewMatches(o.preview) &&
        o.preview.source_day_timezone === "UTC" &&
        Array.isArray(o.storedTimezones) &&
        o.storedTimezones.length === 1 &&
        o.storedTimezones[0] === "UTC";
      break;
    case "failure_recovers_by_new_preview":
      ok =
        o.failure?.status === "rejected" &&
        Boolean(o.failure.error) &&
        /CCS\s*导入未完成/.test(o.errorText ?? "") &&
        o.planCleared === true &&
        previewMatches(o.recovered) &&
        o.previewVisible === true;
      break;
    case "detail_only_hides_rollups_and_discloses_granularity":
      ok =
        o.autoRequests === 14 &&
        o.detailRequests === 11 &&
        o.restoredRequests === o.autoRequests &&
        o.autoBy === "agent" &&
        o.detailBy === o.autoBy &&
        o.restoredBy === o.autoBy &&
        Array.isArray(o.domMatches) &&
        o.domMatches.length === 3 &&
        o.domMatches.every((value) => value === true) &&
        o.initialAutoMode === null &&
        o.detailMode === "detail_only" &&
        o.autoMode === null &&
        /日粒度/.test(o.unknownText ?? "") &&
        /未知/.test(o.unknownText ?? "") &&
        /只统计请求明细/.test(o.detailText ?? "") &&
        /不含只有日粒度/.test(o.detailText ?? "");
      break;
    case "ccs_fixture_unchanged":
      ok = typeof o.before === "string" && /^[a-f0-9]{64}$/.test(o.before) && o.after === o.before;
      break;
  }
  return { ok: Boolean(ok), basis: JSON.stringify(o) };
}
