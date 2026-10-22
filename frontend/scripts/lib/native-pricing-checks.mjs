// AC05：根据真实回调、页面与隔离快照计算结论；缺少探针数据一律失败。
const kinds = ["input", "output", "cache_write", "cache_read"];
const prices = breakdown => kinds.map(kind => breakdown?.lines?.find(line => line.kind === kind)?.unit_price);
const ratesEqual = (breakdown, expected) => prices(breakdown).every((rate, i) => rate === expected[i]);
// Jiff 的 Zoned 字符串保留 [Asia/Shanghai]；只在校验时去除注记，页面仍须原文呈现。
const time = value => typeof value === "string" && Number.isFinite(Date.parse(value.replace(/\[[^\]]+\]$/, "")));
const hitsEqual = (a, b, delta = 0) => a && b && ["modelsdev", "openrouter"].every(source =>
  Number.isSafeInteger(a[source]) && a[source] >= 0 && b[source] === a[source] + delta);
const pathKey = value => typeof value === "string" ? value.replaceAll("\\", "/").toLowerCase() : null;
const sha = value => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
const withoutGrouping = text => String(text ?? "").replaceAll(",", "");
const labels = ["输入", "输出", "缓存写", "缓存命中"];
function ratesVisible(text, rates = {}) {
  return kinds.every((kind, i) => {
    const value = rates[kind];
    const token = text.match(new RegExp(`${labels[i]}\\s+(未知|同输入价|\\$[0-9]+(?:\\.[0-9]+)?)(?=\\s|[·；;]|$)`))?.[1];
    return value == null ? token === "未知" : value === "same_as_input" ? token === "同输入价"
      : token?.startsWith("$") && Number(token.slice(1)) === value;
  });
}
const snapshotSources = sources => Array.isArray(sources) && sources.length > 0
  && sources.every(source => ["models.dev", "openrouter"].includes(source));
function sameRequestCosts(before, after) {
  const valid = rows => Array.isArray(rows) && rows.length > 0
    && rows.every(row => typeof row.key === "string" && row.key.length > 0 && (row.cost === null || Number.isFinite(row.cost)))
    && new Set(rows.map(row => row.key)).size === rows.length;
  const sorted = rows => [...rows].sort((a, b) => a.key.localeCompare(b.key));
  return valid(before) && valid(after) && JSON.stringify(sorted(before)) === JSON.stringify(sorted(after));
}
const settingsCounts = (text, md, or) => {
  const compact = String(text ?? "").replace(/\s/g, "");
  return compact.includes(`models.dev（${md}条，主源）`) && compact.includes(`OpenRouter（${or}条，补充源）`);
};
function formulaVisible(breakdown, rows) {
  if (!Array.isArray(rows) || !Array.isArray(breakdown?.lines)) return false;
  // 独立于 DTO 的固定 Opus 请求事实与结果；不把串行到另一请求的 DTO 当作期望。
  const tokens = [1200, 640, 100, 3000];
  // 固定 fixture 的 f64 运算次序与后端一致，精确比较；不引入金额误差容限。
  const subtotals = tokens.map((value, i) => value / 1_000_000 * [12, 35, 10, 12][i]);
  const shown = ["$0.0144", "$0.0224", "$0.0010", "$0.0360"];
  const total = rows.filter(row => row.label === "估算合计");
  return breakdown.cost_usd === 0.0738 && total.length === 1 && total[0].value === "$0.0738"
    && kinds.every((kind, i) => {
    const line = breakdown.lines.find(row => row.kind === kind);
    const found = rows.filter(row => row.label === (kind === "input" ? "输入（扣除缓存）" : labels[i]));
    if (!line?.priced || !Number.isFinite(line.unit_price) || found.length !== 1) return false;
    const match = withoutGrouping(found[0].value).match(/^(\d+)\s*×\s*(输入价\s+)?\$([0-9]+(?:\.[0-9]+)?)\/M\s*=\s*(\S+)$/);
    return Boolean(match && Number(match[1]) === line.tokens && Number(match[3]) === line.unit_price
      && line.tokens === tokens[i] && line.subtotal === subtotals[i] && match[4] === shown[i]
      && Boolean(match[2]) === (line.rate_kind === "same_as_input"));
  });
}
const ruleRates = value => Object.fromEntries(kinds.map(kind => [kind, value?.[kind] ?? null]));
const rules = entry => ({
  prefix: entry.prefix,
  segments: (entry.segments ?? []).map(segment => ({ label: segment.label ?? null,
    min_tokens: segment.min_tokens, max_tokens: segment.max_tokens ?? null, prices: ruleRates(segment.prices) })),
  schedules: (entry.schedules ?? []).map(schedule => ({ label: schedule.label ?? null, timezone: schedule.timezone ?? "UTC",
    prices: ruleRates(schedule.prices), periods: (schedule.periods ?? []).map(period => ({
      start_time: period.start_time, end_time: period.end_time, weekdays: period.weekdays ?? [], prices: ruleRates(period.prices) })) })),
});
function tierVisible({ entry, tag, tooltip }) {
  if (!entry?.has_tiered_pricing || tag !== true || !/计价依据/.test(tooltip ?? "")) return false;
  const text = withoutGrouping(tooltip);
  const segments = entry.segments ?? [];
  const schedules = entry.schedules ?? [];
  if (!segments.length && !schedules.length) return false;
  const lines = text.split("\n");
  return segments.every(segment => lines.some(line => line.includes(String(segment.min_tokens))
    && (segment.max_tokens == null ? line.includes("∞") : line.includes(String(segment.max_tokens)))
    && (!segment.label || line.includes(segment.label)) && ratesVisible(line, segment.prices)))
    && schedules.every(schedule => lines.some(line => line.includes(schedule.label ?? "时间规则")
      && line.includes(schedule.timezone ?? "UTC") && ratesVisible(line, schedule.prices))
      && (schedule.periods ?? []).every(period => lines.some(line => line.includes(period.start_time)
        && line.includes(period.end_time) && ratesVisible(line, period.prices))));
}

export function pricingVerdict(name, observed) {
  const o = observed ?? {};
  let ok = false;
  switch (name) {
    case "first_run_banner_without_snapshots":
      ok = Array.isArray(o.snapshotFiles) && o.snapshotFiles.length === 0 && o.externalExists === false
        && o.bannerVisible === true && /费用.*未知/.test(o.bannerText ?? "")
        && o.status?.hasAnyPricing === false && o.status.needsSync === true
        && o.totals?.cost_usd === 0 && o.totals.unknown_pricing === true
        && Array.isArray(o.eventCosts) && o.eventCosts.length > 0 && o.eventCosts.every(value => value === null)
        && o.summaryCost === "未知†" && Array.isArray(o.eventCostTexts) && o.eventCostTexts.length === o.eventCosts.length
        && o.eventCostTexts.every(value => value === "未知") && Array.isArray(o.groupCostTexts) && o.groupCostTexts.length > 0
        && o.groupCostTexts.every(value => value === "未知†");
      break;
    case "manual_sync_both_sources": {
      const reports = o.call?.value;
      ok = hitsEqual(o.beforeHits, o.afterHits, 1) && o.call?.status === "fulfilled"
        && Array.isArray(reports) && reports.length === 2
        && [["models.dev", "modelsdev"], ["OpenRouter", "openrouter"]].every(([source, key]) => {
          const report = reports.find(value => value.source === source);
          return report?.count === 2 && time(report.synced_at) && o.view?.[`${key}_count`] === 2
            && o.view[`${key}_synced_at`] === report.synced_at && String(o.timeText ?? "").includes(report.synced_at);
        }) && settingsCounts(o.settingsText, 2, 2) && o.externalExists === false && snapshotSources(o.pricedSources)
        && o.bannerVisible === false && Number.isFinite(o.cost) && o.cost > 0 && /^\$/.test(o.summaryCostText ?? "");
      break;
    }
    case "single_source_failure_is_partial":
      ok = o.call?.status === "rejected" && /OpenRouter/.test(o.call.error ?? "")
        && /同步部分失败/.test(o.notice ?? "") && /OpenRouter/.test(o.notice ?? "")
        && o.retryVisible === true && o.retryEnabled === true && o.retryClicked === true
        && o.view?.modelsdev_count === 2 && o.snapshotCount === 2
        && time(o.snapshotTime) && o.view.modelsdev_synced_at === o.snapshotTime
        && settingsCounts(o.settingsText, 2, 0) && String(o.timeText ?? "").includes(o.snapshotTime);
      break;
    case "offline_restart_uses_snapshots":
      ok = o.cleanExit === true && o.hostAlive === true && o.bannerVisible === false
        && Number.isFinite(o.beforeCost) && o.beforeCost > 0 && Number.isFinite(o.afterCost) && o.afterCost > 0
        && sameRequestCosts(o.beforeRequestCosts, o.afterRequestCosts)
        && /^\$/.test(o.beforeSummaryCostText ?? "") && o.beforeSummaryCostText === o.afterSummaryCostText
        && Array.isArray(o.beforeSnapshots) && o.beforeSnapshots.length === 2 && o.beforeSnapshots.every(sha)
        && Array.isArray(o.afterSnapshots) && o.beforeSnapshots.every((value, i) => value === o.afterSnapshots[i])
        && hitsEqual(o.beforeHits, o.afterHits) && o.externalExists === false
        && snapshotSources(o.beforeSources) && snapshotSources(o.afterSources);
      break;
    case "segment_boundary_272000_272001":
      ok = Array.isArray(o.boundaries) && o.boundaries.length === 2
        && o.boundaries.every((item, index) => item.breakdown?.basis_value === 272000 + index
          && item.breakdown.segment_label === ["不超过272K", "超过272K"][index]
          && ratesEqual(item.breakdown, [[4, 20, 5, 0.4], [8, 30, 10, 0.8]][index])
          && withoutGrouping(item.tooltip).includes(String(272000 + index))
          && String(item.tooltip ?? "").includes(item.breakdown.segment_label));
      break;
    case "candidate_highest_without_cross_source_mix": {
      const matched = o.breakdown?.matched;
      ok = matched?.source === "external" && matched.raw_key === "external/claude-opus-5-5"
        && matched.candidate_count === 4 && matched.complete_candidate_count === 3
        && matched.incomplete_candidate_count === 1 && matched.reason === "highest_complete_cost"
        && ratesEqual(o.breakdown, [12, 35, 10, 12]) && formulaVisible(o.breakdown, o.formulaRows)
        && /候选\s*4\s*条/.test(o.tooltip ?? "")
        && /取最高费用/.test(o.tooltip ?? "");
      break;
    }
    case "schedule_by_event_time":
      ok = Array.isArray(o.periods) && o.periods.length === 2 && o.periods.every((item, index) => {
        const matched = item.breakdown?.matched;
        return matched?.source === "external" && matched.schedule_label === ["B2峰时", "B2谷时"][index]
          && matched.schedule_timezone === "Asia/Shanghai"
          && Date.parse(matched.request_at) === Date.parse(["2026-10-08T01:00:00Z", "2026-10-08T17:00:00Z"][index])
          && ratesEqual(item.breakdown, [[12, 35, 1, 0.5], [2, 10, 1, 0.5]][index])
          && String(item.tooltip ?? "").includes(matched.schedule_label)
          && String(item.tooltip ?? "").includes("Asia/Shanghai")
          && String(item.tooltip ?? "").includes(matched.request_at);
      });
      break;
    case "settings_segment_tags_expand_all_tiers_and_schedule":
      ok = Number.isSafeInteger(o.tieredCount) && o.tieredCount > 0
        && Array.isArray(o.expectedRules) && o.expectedRules.length === o.tieredCount
        && new Set(o.expectedRules.map(entry => entry.prefix)).size === o.tieredCount
        && Array.isArray(o.expanded) && o.expanded.length === o.tieredCount && o.expanded.every(tierVisible)
        && o.expectedRules.every(expected => {
          const entries = o.expanded.filter(item => item.entry?.prefix === expected.prefix);
          return entries.length === 1 && JSON.stringify(rules(entries[0].entry)) === JSON.stringify(rules(expected));
        });
      break;
    case "same_as_input_and_excluded_incomplete_in_tooltip": {
      const read = o.breakdown?.lines?.find(line => line.kind === "cache_read");
      const input = o.breakdown?.lines?.find(line => line.kind === "input");
      ok = read?.rate_kind === "same_as_input" && Number.isFinite(read.unit_price) && read.unit_price === input?.unit_price
        && o.breakdown?.matched?.incomplete_candidates_excluded > 0
        && typeof o.breakdown.excluded_candidate_warning === "string" && o.breakdown.excluded_candidate_warning.length > 0
        && /输入价/.test(o.tooltip ?? "") && String(o.tooltip ?? "").includes(o.breakdown.excluded_candidate_warning);
      break;
    }
    case "open_pricing_file_creates_template_in_root":
      ok = o.existedBefore === false && o.created === true && /\[\[model\]\]/.test(o.content ?? "")
        && Boolean(pathKey(o.expectedPath)) && pathKey(o.returnedPath) === pathKey(o.expectedPath)
        && Array.isArray(o.openerPaths) && o.openerPaths.length === 1 && pathKey(o.openerPaths[0]) === pathKey(o.expectedPath)
        && o.recorded === true && o.processProbeOk === true && Array.isArray(o.foreignProcesses) && o.foreignProcesses.length === 0;
      break;
  }
  return { ok: Boolean(ok), basis: JSON.stringify(o) };
}
