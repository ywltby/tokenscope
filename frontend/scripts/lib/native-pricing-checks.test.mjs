// @vitest-environment node
import { describe, expect, it } from "vitest";
import { pricingVerdict } from "./native-pricing-checks.mjs";

const at = "2026-10-22T05:00:00Z";
const view = { modelsdev_count: 2, openrouter_count: 2, modelsdev_synced_at: at, openrouter_synced_at: at };
const rates = values => ["input", "output", "cache_write", "cache_read"].map((kind, i) => ({ kind, unit_price: values[i], rate_kind: kind === "cache_read" ? "same_as_input" : "fixed", priced: true }));
const peak = { basis_value: 4300, matched: { raw_key: "external/claude-opus-5-5", source: "external", candidate_count: 4, complete_candidate_count: 3, incomplete_candidate_count: 1, incomplete_candidates_excluded: 1, reason: "highest_complete_cost" }, lines: rates([12, 35, 10, 12]), excluded_candidate_warning: "已排除 1 个不完整候选" };
const settingsEntries = [
  { prefix: "external/gpt-5.6", has_tiered_pricing: true, segments: [{ label: "不超过272K", min_tokens: 0, max_tokens: 272001, prices: { input: 4, output: 20, cache_write: 5, cache_read: 0.4 } }, { label: "超过272K", min_tokens: 272001, prices: { input: 8, output: 30, cache_write: 10, cache_read: 0.8 } }], schedules: [] },
  { prefix: "external/schedule-only-probe", has_tiered_pricing: true, segments: [], schedules: [{ label: "B2峰时", timezone: "Asia/Shanghai", periods: [{ start_time: "09:00", end_time: "18:00" }] }, { label: "B2谷时", timezone: "Asia/Shanghai", periods: [{ start_time: "00:00", end_time: "09:00" }] }] },
];
const cases = [
  ["first_run_banner_without_snapshots", { snapshotFiles: [], externalExists: false, bannerVisible: true, bannerText: "尚未获取定价 当前费用仅能显示为未知", status: { hasAnyPricing: false, needsSync: true }, totals: { cost_usd: 0, unknown_pricing: true }, eventCosts: [null, null] }, o => { o.eventCosts[0] = 0; }],
  ["manual_sync_both_sources", { beforeHits: { modelsdev: 1, openrouter: 1 }, afterHits: { modelsdev: 2, openrouter: 2 }, call: { status: "fulfilled", value: [{ source: "models.dev", count: 2, synced_at: at }, { source: "OpenRouter", count: 2, synced_at: at }] }, view, timeText: `models.dev 上次同步：${at} OpenRouter 上次同步：${at}`, bannerVisible: false, cost: 0.1 }, o => { o.afterHits.openrouter--; }],
  ["single_source_failure_is_partial", { call: { status: "rejected", error: "OpenRouter: status 500" }, notice: "同步部分失败：OpenRouter status 500", retryVisible: true, retryEnabled: true, retryClicked: true, view: { ...view, openrouter_count: 0, openrouter_synced_at: null }, snapshotCount: 2, snapshotTime: at }, o => { o.retryClicked = false; }],
  ["offline_restart_uses_snapshots", { cleanExit: true, beforeCost: 0.15, afterCost: 0.15, beforeSnapshots: ["a".repeat(64), "b".repeat(64)], afterSnapshots: ["a".repeat(64), "b".repeat(64)], beforeHits: { modelsdev: 2, openrouter: 2 }, afterHits: { modelsdev: 2, openrouter: 2 }, bannerVisible: false, hostAlive: true }, o => { o.afterCost = 0; }],
  ["segment_boundary_272000_272001", { boundaries: [{ breakdown: { basis_value: 272000, segment_label: "不超过272K", lines: rates([4, 20, 5, 0.4]) }, tooltip: "命中档位 不超过272K prompt tokens 272,000" }, { breakdown: { basis_value: 272001, segment_label: "超过272K", lines: rates([8, 30, 10, 0.8]) }, tooltip: "命中档位 超过272K prompt tokens 272,001" }] }, o => { o.boundaries[1].breakdown.lines[0].unit_price = 4; }],
  ["candidate_highest_without_cross_source_mix", { breakdown: peak, tooltip: "候选 4 条（完整 3/不完整 1），在完整候选中取最高费用" }, o => { o.breakdown.lines[1].unit_price = 25; }],
  ["schedule_by_event_time", { periods: [{ breakdown: { matched: { source: "external", schedule_label: "B2峰时", schedule_timezone: "Asia/Shanghai", request_at: "2026-10-08T01:00:00Z" }, lines: rates([12, 35, 1, 0.5]) }, tooltip: "B2峰时 Asia/Shanghai 2026-10-08T01:00:00Z" }, { breakdown: { matched: { source: "external", schedule_label: "B2谷时", schedule_timezone: "Asia/Shanghai", request_at: "2026-10-08T17:00:00Z" }, lines: rates([2, 10, 1, 0.5]) }, tooltip: "B2谷时 Asia/Shanghai 2026-10-08T17:00:00Z" }] }, o => { o.periods[1].breakdown.matched.schedule_label = "B2峰时"; }],
  ["settings_segment_tags_expand_all_tiers_and_schedule", { tieredCount: 2, expanded: [{ entry: settingsEntries[0], tag: true, tooltip: "计价依据\n分段 不超过272K [0, 272,001) 输入 $4 · 输出 $20 · 缓存写 $5 · 缓存命中 $0.4\n分段 超过272K [272,001, ∞) 输入 $8 · 输出 $30 · 缓存写 $10 · 缓存命中 $0.8" }, { entry: settingsEntries[1], tag: true, tooltip: "计价依据\n峰谷 B2峰时 Asia/Shanghai 09:00–18:00 输入 未知 输出 未知 缓存写 未知 缓存命中 未知\n峰谷 B2谷时 Asia/Shanghai 00:00–09:00 输入 未知 输出 未知 缓存写 未知 缓存命中 未知" }] }, o => { o.expanded.pop(); }],
  ["same_as_input_and_excluded_incomplete_in_tooltip", { breakdown: peak, tooltip: "缓存命中 输入价 $12 已排除 1 个不完整候选" }, o => { o.breakdown.lines[3].rate_kind = "fixed"; }],
  ["open_pricing_file_creates_template_in_root", { existedBefore: false, created: true, content: '[[model]]\nprefix = "synthetic"', expectedPath: "C:/isolated/tokenscope/pricing.toml", returnedPath: "C:/isolated/tokenscope/pricing.toml", openerPaths: ["C:/isolated/tokenscope/pricing.toml"], recorded: true, processProbeOk: true, foreignProcesses: [] }, o => { o.foreignProcesses.push("editor.exe"); }],
];
for (const [name, observed] of cases) {
  if (name === "first_run_banner_without_snapshots") Object.assign(observed,
    { summaryCost: "未知†", eventCostTexts: ["未知", "未知"], groupCostTexts: ["未知†"] });
  if (name === "manual_sync_both_sources") Object.assign(observed, { externalExists: false,
    pricedSources: ["models.dev", "openrouter"], settingsText: "models.dev（2 条，主源）+ OpenRouter（2 条，补充源）", summaryCostText: "$0.10†" });
  if (name === "single_source_failure_is_partial") Object.assign(observed,
    { settingsText: "models.dev（2 条，主源）+ OpenRouter（0 条，补充源）", timeText: `models.dev 上次同步：${at}` });
  if (name === "offline_restart_uses_snapshots") Object.assign(observed,
    { externalExists: false, beforeSources: ["models.dev", "openrouter"], afterSources: ["models.dev", "openrouter"],
      beforeRequestCosts: [{ key: "gpt", cost: 0.1, source: "models.dev" }, { key: "opus", cost: 0.05, source: "openrouter" }],
      afterRequestCosts: [{ key: "gpt", cost: 0.1, source: "models.dev" }, { key: "opus", cost: 0.05, source: "openrouter" }],
      beforeSummaryCostText: "$0.1500", afterSummaryCostText: "$0.1500" });
  if (name === "candidate_highest_without_cross_source_mix") observed.formulaRows = observed.breakdown.lines.map((line, i) => {
    line.tokens = [1200, 640, 100, 3000][i];
    line.subtotal = [1200, 640, 100, 3000][i] / 1_000_000 * [12, 35, 10, 12][i];
    observed.breakdown.cost_usd = 0.0738;
    return { label: ["输入（扣除缓存）", "输出", "缓存写", "缓存命中"][i],
      value: `${line.tokens} × ${line.rate_kind === "same_as_input" ? "输入价 " : ""}$${line.unit_price}/M = ${["$0.0144", "$0.0224", "$0.0010", "$0.0360"][i]}` };
  });
  if (name === "candidate_highest_without_cross_source_mix") observed.formulaRows.push({ label: "估算合计", value: "$0.0738" });
  if (name === "settings_segment_tags_expand_all_tiers_and_schedule") observed.expectedRules = structuredClone(settingsEntries);
}
describe.each(cases)("%s", (name, good, corrupt) => {
  it("完整实测证据通过", () => { expect(pricingVerdict(name, structuredClone(good)).ok).toBe(true); });
  it("违反该要求的反例失败", () => { const bad = structuredClone(good); corrupt(bad); expect(pricingVerdict(name, bad).ok).toBe(false); });
  it("缺探针数据必须失败", () => { expect(pricingVerdict(name, {}).ok).toBe(false); });
});

it("峰谷时段费率不能被规则级未知占位替代", () => {
  const entry = structuredClone(settingsEntries[1]);
  entry.schedules[0].periods[0].prices = { input: 12, output: 35, cache_write: null, cache_read: null };
  const observed = { tieredCount: 1, expanded: [{ entry, tag: true,
    tooltip: "计价依据 峰谷 B2峰时 Asia/Shanghai 09:00–18:00 B2谷时 Asia/Shanghai 00:00–09:00 输入 未知 输出 未知 缓存写 未知 缓存命中 未知" }] };
  expect(pricingVerdict("settings_segment_tags_expand_all_tiers_and_schedule", observed).ok).toBe(false);
});

it("真实排除诊断必须原文呈现，不能只按测试自造文案匹配", () => {
  const breakdown = structuredClone(peak);
  breakdown.excluded_candidate_warning = "有 1 个候选因缺少价格未参与主估算（incomplete）；若其缺失分项有价，实际最高费用可能更高";
  expect(pricingVerdict("same_as_input_and_excluded_incomplete_in_tooltip", {
    breakdown, tooltip: `缓存命中 输入价 $12 候选 4 条（完整 3/不完整 1） ${breakdown.excluded_candidate_warning}`,
  }).ok).toBe(true);
});

it("同步时间保留后端的时区注记", () => {
  const observed = structuredClone(cases.find(row => row[0] === "manual_sync_both_sources")[1]);
  const zoned = "2026-10-22T13:00:00+08:00[Asia/Shanghai]";
  for (const report of observed.call.value) report.synced_at = zoned;
  observed.view.modelsdev_synced_at = observed.view.openrouter_synced_at = zoned;
  observed.timeText = `models.dev ${zoned} OpenRouter ${zoned}`;
  expect(pricingVerdict("manual_sync_both_sources", observed).ok).toBe(true);
});

it("外置规则兜底不能冒充同步快照已经参与费用", () => {
  const observed = structuredClone(cases.find(row => row[0] === "manual_sync_both_sources")[1]);
  Object.assign(observed, { externalExists: true, pricedSources: ["external"] });
  expect(pricingVerdict("manual_sync_both_sources", observed).ok).toBe(false);
});
it("保留外置规则的离线成功不能证明快照被使用", () => {
  const observed = structuredClone(cases.find(row => row[0] === "offline_restart_uses_snapshots")[1]);
  Object.assign(observed, { externalExists: true, beforeSources: ["external"], afterSources: ["external"] });
  expect(pricingVerdict("offline_restart_uses_snapshots", observed).ok).toBe(false);
});
it("DTO条数正确而设置页不显示条数时失败", () => {
  const observed = structuredClone(cases.find(row => row[0] === "manual_sync_both_sources")[1]);
  observed.settingsText = "";
  expect(pricingVerdict("manual_sync_both_sources", observed).ok).toBe(false);
});
it("部分失败时缺设置页成功源时间也失败", () => {
  const observed = structuredClone(cases.find(row => row[0] === "single_source_failure_is_partial")[1]);
  observed.timeText = "";
  expect(pricingVerdict("single_source_failure_is_partial", observed).ok).toBe(false);
});
it("后端未知而页面显示零费用时失败", () => {
  const observed = structuredClone(cases.find(row => row[0] === "first_run_banner_without_snapshots")[1]);
  Object.assign(observed, { summaryCost: "$0.00", eventCostTexts: ["$0.00", "$0.00"], groupCostTexts: ["$0.00"] });
  expect(pricingVerdict("first_run_banner_without_snapshots", observed).ok).toBe(false);
});
it("DTO候选四桶正确而浮层混入其他单价时失败", () => {
  const observed = structuredClone(cases.find(row => row[0] === "candidate_highest_without_cross_source_mix")[1]);
  observed.formulaRows = [{ label: "输入（扣除缓存）", value: "1,200 × $5/M = $0.006" }];
  expect(pricingVerdict("candidate_highest_without_cross_source_mix", observed).ok).toBe(false);
});
it("设置费率必须完整匹配，不能把40当4或0.45当0.4", () => {
  const observed = structuredClone(cases.find(row => row[0] === "settings_segment_tags_expand_all_tiers_and_schedule")[1]);
  observed.expanded[0].tooltip = observed.expanded[0].tooltip.replace("输入 $4", "输入 $40").replace("缓存命中 $0.4", "缓存命中 $0.45");
  expect(pricingVerdict("settings_segment_tags_expand_all_tiers_and_schedule", observed).ok).toBe(false);
});

it("分项单价正确但等号右侧金额错误必须失败", () => {
  const observed = structuredClone(cases.find(row => row[0] === "candidate_highest_without_cross_source_mix")[1]);
  observed.formulaRows[0].value = observed.formulaRows[0].value.replace(/= .+$/, "= $999");
  expect(pricingVerdict("candidate_highest_without_cross_source_mix", observed).ok).toBe(false);
});
it("合计错误必须失败", () => {
  const observed = structuredClone(cases.find(row => row[0] === "candidate_highest_without_cross_source_mix")[1]);
  observed.formulaRows.find(row => row.label === "估算合计").value = "$999";
  expect(pricingVerdict("candidate_highest_without_cross_source_mix", observed).ok).toBe(false);
});
it("DTO和页面一起串到另一请求也不能冒充固定Opus样本", () => {
  const observed = structuredClone(cases.find(row => row[0] === "candidate_highest_without_cross_source_mix")[1]);
  observed.breakdown.lines[0].tokens = 9;
  observed.formulaRows[0].value = observed.formulaRows[0].value.replace(/^1200/, "9");
  expect(pricingVerdict("candidate_highest_without_cross_source_mix", observed).ok).toBe(false);
});
it("整个规则条目从DTO和页面同时消失不能通过自洽数量", () => {
  const observed = structuredClone(cases.find(row => row[0] === "settings_segment_tags_expand_all_tiers_and_schedule")[1]);
  observed.expectedRules = structuredClone(settingsEntries);
  observed.expanded.pop(); observed.tieredCount--;
  expect(pricingVerdict("settings_segment_tags_expand_all_tiers_and_schedule", observed).ok).toBe(false);
});

it("后端真实f64运算值与四位金额展示分别核对", () => {
  const observed = structuredClone(cases.find(row => row[0] === "candidate_highest_without_cross_source_mix")[1]);
  for (const [i, line] of observed.breakdown.lines.entries()) line.subtotal = [1200, 640, 100, 3000][i] / 1_000_000 * [12, 35, 10, 12][i];
  expect(pricingVerdict("candidate_highest_without_cross_source_mix", observed).ok).toBe(true);
});

it("离线逐请求费用和显示严格相同，跨维度总和仅有f64累加次序差异", () => {
  const observed = structuredClone(cases.find(row => row[0] === "offline_restart_uses_snapshots")[1]);
  Object.assign(observed, { beforeCost: 2.274514, afterCost: 2.2745140000000004,
    beforeSummaryCostText: "$2.27†", afterSummaryCostText: "$2.27†" });
  expect(pricingVerdict("offline_restart_uses_snapshots", observed).ok).toBe(true);
});
it("离线总和碰巧相同但单请求费用或页面合计改变必须失败", () => {
  const observed = structuredClone(cases.find(row => row[0] === "offline_restart_uses_snapshots")[1]);
  observed.afterRequestCosts[0].cost = 0.2;
  expect(pricingVerdict("offline_restart_uses_snapshots", observed).ok).toBe(false);
  observed.afterRequestCosts = structuredClone(observed.beforeRequestCosts);
  observed.afterSummaryCostText = "$999";
  expect(pricingVerdict("offline_restart_uses_snapshots", observed).ok).toBe(false);
});
