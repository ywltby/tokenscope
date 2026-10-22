// AC05：价格只来自隔离文件与回环桩；所有结论取自真实 UI、IPC 响应和落盘快照。
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createUnifiedContext } from "../lib/native-unified.mjs";
import { pricingVerdict } from "../lib/native-pricing-checks.mjs";
import { readAcceptanceTrace } from "../lib/acceptance-io.mjs";
import { collectDescendants } from "../lib/native-instance.mjs";
import { spawnSyncBounded } from "../lib/native-scenario.mjs";

const stagedFile = root => path.join(root, "pricing-rules-staged.toml");
const pricingFile = root => path.join(root, "tokenscope", "pricing.toml");
const snapshotFiles = root => ["pricing-modelsdev.json", "pricing-openrouter.json"]
  .map(name => path.join(root, "tokenscope", name)).filter(file => fs.existsSync(file));
const sha = file => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const RULE_CATALOG = `
import json, sys, tomllib
from pathlib import Path
config = tomllib.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
kinds = ('input', 'output', 'cache_write', 'cache_read')
def rates(value): return {k:value.get(k) for k in kinds}
entries = []
for model in config['model']:
    if not (model.get('segment') or model.get('schedule')): continue
    entries.append(dict(prefix=model['prefix'],
        segments=[dict(label=s.get('label'),min_tokens=s['min_tokens'],max_tokens=s.get('max_tokens'),prices=rates(s)) for s in model.get('segment',[])],
        schedules=[dict(label=s.get('label'),timezone=s.get('timezone','UTC'),prices=rates(s),
            periods=[dict(start_time=p['start_time'],end_time=p['end_time'],weekdays=p.get('weekdays',[]),prices=rates(p)) for p in s.get('period',[])]) for s in model.get('schedule',[])]))
print(json.dumps(entries))
`;
function expectedRuleCatalog(root) {
  const source = stagedFile(root);
  const result = spawnSyncBounded("python", ["-B", "-c", RULE_CATALOG, source], { action: "read-isolated-pricing-rule-catalog" });
  if (result.status !== 0) throw new Error(`隔离价格 fixture 清单解析失败：${result.stderr}`);
  const entries = JSON.parse(result.stdout);
  const expected = ["external/claude-opus-5-5", "external/gpt-5.6", "external/schedule-only-probe"];
  if (entries.length !== expected.length || !expected.every(prefix => entries.some(entry => entry.prefix === prefix))) {
    throw new Error("场景价格 fixture 不含完整的三个分段/峰谷模型");
  }
  return { source, sha256: sha(source), entries };
}

// 本场景专属数据：保留统一 fixture 原有 10 条，追加只受峰谷条件影响的两条请求。
// 定价规则暂存于输入目录之外，首轮确实没有快照或外置价格。
export function preparePricingScenario(root) {
  const pricing = pricingFile(root);
  const extra = `
[[model]]
prefix = "external/schedule-only-probe"
input = 4.0
output = 20.0
cache_write = 1.0
cache_read = 0.5
[[model.schedule]]
label = "B2峰时"
timezone = "Asia/Shanghai"
[[model.schedule.period]]
start_time = "09:00"
end_time = "18:00"
input = 12.0
output = 35.0
[[model.schedule]]
label = "B2谷时"
timezone = "Asia/Shanghai"
[[model.schedule.period]]
start_time = "00:00"
end_time = "09:00"
input = 2.0
output = 10.0
`;
  fs.writeFileSync(stagedFile(root), fs.readFileSync(pricing, "utf8") + extra, "utf8");
  fs.unlinkSync(pricing);
  const directory = path.join(root, "sources", "claude", "projects", "-acceptance-workspace-tokenscope");
  if (!fs.existsSync(directory)) throw new Error("统一 Claude fixture 目录缺失");
  const session = "aaaaaaaa-bbbb-cccc-dddd-ffffffffffff";
  const events = ["2026-10-08T01:00:00.000Z", "2026-10-08T17:00:00.000Z"].map((timestamp, index) => ({
    type: "assistant", isSidechain: false, sessionId: session, timestamp,
    cwd: "C:\\acceptance\\workspace\\pricing",
    message: { id: `msg-pricing-schedule-${index}`, role: "assistant", model: "schedule-only-probe",
      content: [{ type: "text", text: "acceptance synthetic pricing reply" }],
      usage: { input_tokens: 1000, output_tokens: 100, cache_read_input_tokens: 50, cache_creation_input_tokens: 10 } },
  }));
  fs.writeFileSync(path.join(directory, `${session}.jsonl`), events.map(row => JSON.stringify(row)).join("\n") + "\n", "utf8");
}

export async function nativePricing(context) {
  const { inst, ck, ev } = context;
  const u = createUnifiedContext(context);
  ev.pricing = { observations: {} };
  const check = (name, observed) => {
    ev.pricing.observations[name] = observed;
    const verdict = pricingVerdict(name, observed);
    ck.add(name, verdict.ok, verdict.basis);
  };
  const hits = () => Object.fromEntries(Object.entries(inst.opts.controlledIo.snapshot().hits).map(([key, rows]) => [key, rows.length]));
  const calls = async command => (await u.readProbe()).invokes.filter(row => row.command === command);
  const settledAfter = async (command, count) => {
    await u.waitUntil(async () => {
      const rows = await calls(command);
      return rows.length > count && rows.at(-1).status !== "pending";
    }, 20000, `${command} 的真实回调`);
    return u.last(command);
  };
  const query = async predicate => {
    let snapshot;
    await u.waitUntil(async () => {
      snapshot = await u.querySnapshot();
      return snapshot.sameQuery && snapshot.domMatches && (!predicate || predicate(snapshot));
    }, 20000, "价格查询与真实明细 DOM 一致");
    return snapshot;
  };
  const pricingView = async () => {
    await u.nav("设置");
    await u.section("价格");
    // 首次进入会消费启动预读；真实切页重新激活，取写操作之后的列表与同步时间。
    await u.nav("汇总");
    const count = (await calls("pricing_entries")).length;
    await u.nav("设置");
    await u.section("价格");
    return (await settledAfter("pricing_entries", count)).value;
  };
  const settingsEvidence = async () => {
    const view = await pricingView();
    const settingsText = await u.page().locator('[data-section="pricing"]').innerText();
    await u.section("用量数据");
    const collapse = u.page().locator('[data-section="usage"] .tech-collapse');
    if (!(await collapse.locator(".tech-line").last().isVisible())) {
      await u.click(u.page().getByText("技术详情", { exact: true }));
    }
    return { view, settingsText, timeText: await collapse.innerText() };
  };
  const pricedSources = snapshot => snapshot.events.rows.filter(row => row.cost_usd > 0)
    .map(row => row.cost_breakdown?.matched?.source ?? null);
  const requestCosts = snapshot => snapshot.events.rows.map(row => ({
    key: JSON.stringify([row.agent, row.session_id, row.record_id, row.model, row.ts,
      row.input, row.output, row.cache_write, row.cache_read]),
    cost: row.cost_usd, source: row.cost_breakdown?.matched?.source ?? null,
  }));
  const tooltip = async trigger => {
    await u.click(trigger);
    let id;
    await u.waitUntil(async () => {
      id = await trigger.getAttribute("aria-describedby");
      return id && await u.page().locator(`[id="${id}"]`).isVisible();
    }, 20000, "真实说明浮层展开");
    const body = u.page().locator(`[id="${id}"]`);
    const text = await body.innerText();
    const formulaRows = await body.locator(".bd-row").evaluateAll(rows => rows.map(row => ({
      label: row.querySelector(".bd-label")?.textContent?.trim(),
      value: row.querySelector(".bd-value")?.textContent?.trim(),
    })));
    await trigger.press("Escape");
    return { tooltip: text, formulaRows };
  };
  const eventTooltip = async (snapshot, row) => {
    const index = snapshot.events.rows.indexOf(row);
    if (index < 0) throw new Error("待核对的请求不在当前真实查询内");
    return tooltip(u.page().locator(".events-card tbody tr").nth(index).locator(".cost-trigger"));
  };
  let failure;
  try {
    const catalog = expectedRuleCatalog(inst.root);
    ev.pricing.rule_catalog = catalog;
    // 重载只重新挂载真实应用，不改 DOM/业务状态；Node 旁路监听仍记录整个初始 IPC。
    await u.installProbe();
    await u.page().reload({ waitUntil: "domcontentloaded" });
    await inst.waitForData();
    await inst.setWindowSize(1280, 820);
    await u.click(u.page().getByRole("radio", { name: "按应用", exact: true }));
    const first = await query(snapshot => snapshot.summary.by === "agent" && snapshot.events.total === 12);
    await u.waitUntil(async () => (await u.last("pricing_status"))?.status === "fulfilled");
    check("first_run_banner_without_snapshots", { snapshotFiles: snapshotFiles(inst.root), externalExists: fs.existsSync(pricingFile(inst.root)),
      bannerVisible: await u.page().locator(".pricing-notice:not(.is-error)").isVisible(),
      bannerText: await u.page().locator(".pricing-notice:not(.is-error)").innerText(),
      status: (await u.last("pricing_status"))?.value, totals: first.summary.totals,
      eventCosts: first.events.rows.map(row => row.cost_usd),
      summaryCost: await u.page().locator(".metric-value-main").innerText(),
      eventCostTexts: await u.page().locator(".events-card tbody tr td:nth-child(9)").allInnerTexts(),
      groupCostTexts: await u.page().locator(".usage-card tbody tr td:nth-child(8)").allInnerTexts() });

    inst.opts.controlledIo.setMode("openrouter", "500");
    let count = (await calls("sync_pricing_openrouter")).length;
    await u.click(u.page().getByRole("button", { name: "立即同步", exact: true }));
    const failed = await settledAfter("sync_pricing_openrouter", count);
    const notice = u.page().locator(".pricing-notice.is-error");
    await notice.waitFor({ state: "visible" });
    const retry = notice.getByRole("button", { name: "重试同步", exact: true });
    const partial = { call: failed, notice: await notice.innerText(),
      retryVisibleOnFirstNotice: await retry.isVisible() };
    const md = JSON.parse(fs.readFileSync(path.join(inst.root, "tokenscope", "pricing-modelsdev.json"), "utf8"));
    partial.snapshotCount = md.entries.length;
    partial.snapshotTime = md.synced_at;
    Object.assign(partial, await settingsEvidence());
    await u.nav("汇总");
    inst.opts.controlledIo.setMode("openrouter", "ok");
    const beforeHits = hits();
    count = (await calls("sync_pricing_openrouter")).length;
    await u.waitUntil(async () => await retry.isVisible() && await retry.isEnabled(), 20000, "实际重试按钮可见可点击");
    partial.retryVisible = await retry.isVisible();
    partial.retryEnabled = await retry.isEnabled();
    await u.click(u.page().getByRole("button", { name: "重试同步", exact: true }));
    partial.retryClicked = true;
    check("single_source_failure_is_partial", partial);
    const synced = await settledAfter("sync_pricing_openrouter", count);
    await u.waitUntil(async () => (await u.page().locator(".pricing-notice").count()) === 0);
    const bothHits = hits();
    const settings = await settingsEvidence();
    await u.nav("汇总");
    await u.click(u.page().getByRole("radio", { name: "按模型", exact: true }));
    const snapshotOnly = await query(snapshot => snapshot.summary.by === "model" && pricedSources(snapshot).length > 0);
    check("manual_sync_both_sources", { beforeHits, afterHits: bothHits, call: synced, ...settings,
      externalExists: fs.existsSync(pricingFile(inst.root)), pricedSources: pricedSources(snapshotOnly),
      summaryCostText: await u.page().locator(".metric-value-main").innerText(),
      bannerVisible: (await u.page().locator(".pricing-notice").count()) > 0, cost: snapshotOnly.summary.totals.cost_usd });

    // 同步费用先在完全没有外置规则时核对；条件价格阶段才恢复受控外置输入。
    fs.copyFileSync(stagedFile(inst.root), pricingFile(inst.root));
    await u.click(u.page().getByRole("radio", { name: "按项目", exact: true }));
    const priced = await query(snapshot => snapshot.summary.by === "project" && snapshot.events.rows.some(row => row.cost_breakdown?.matched?.raw_key === "external/claude-opus-5-5"));

    const boundaries = [];
    for (const tokens of [272000, 272001]) {
      const row = priced.events.rows.find(value => value.model === "gpt-5.6" && value.input === tokens);
      if (!row) throw new Error(`缺少 ${tokens} token 边界请求`);
      boundaries.push({ breakdown: row.cost_breakdown, ...await eventTooltip(priced, row) });
    }
    check("segment_boundary_272000_272001", { boundaries });
    const opus = priced.events.rows.find(row => row.record_id === "msg-acceptance-1");
    if (!opus) throw new Error("合成 Opus 请求缺失");
    const opusObservation = { breakdown: opus.cost_breakdown, ...await eventTooltip(priced, opus) };
    check("candidate_highest_without_cross_source_mix", opusObservation);
    check("same_as_input_and_excluded_incomplete_in_tooltip", opusObservation);
    const periods = [];
    for (const id of ["msg-pricing-schedule-0", "msg-pricing-schedule-1"]) {
      const row = priced.events.rows.find(value => value.record_id === id);
      if (!row) throw new Error(`缺少峰谷请求 ${id}`);
      periods.push({ breakdown: row.cost_breakdown, ...await eventTooltip(priced, row) });
    }
    check("schedule_by_event_time", { periods });
    await u.page().screenshot({ path: path.join(inst.opts.shots, "pricing-breakdown.png") });

    const settingsView = await pricingView();
    const tiered = settingsView.entries.filter(entry => entry.segments?.length || entry.schedules?.length);
    const expanded = [];
    for (const entry of tiered) {
      const trigger = u.page().getByRole("button", { name: `${entry.prefix} 档位计价明细`, exact: true });
      expanded.push({ entry, tag: await trigger.getByText("分段", { exact: true }).isVisible(), ...await tooltip(trigger) });
    }
    check("settings_segment_tags_expand_all_tiers_and_schedule", {
      expectedRules: catalog.entries, tieredCount: tiered.length, expanded });
    // 离线基线移除外置文件，不能让外置规则代替快照掩盖回归。
    fs.unlinkSync(pricingFile(inst.root));
    await u.nav("汇总");
    await u.click(u.page().getByRole("radio", { name: "按模型", exact: true }));
    const beforeOffline = await query(snapshot => snapshot.summary.by === "model" && pricedSources(snapshot).length > 0
      && pricedSources(snapshot).every(source => ["models.dev", "openrouter"].includes(source)));
    const beforeSummaryCostText = await u.page().locator(".metric-value-main").innerText();
    await u.saveProbe("before-offline-restart");
    const beforeSnapshots = snapshotFiles(inst.root).map(sha);
    const offlineHits = hits();
    inst.opts.controlledIo.setMode("modelsdev", "down");
    inst.opts.controlledIo.setMode("openrouter", "down");
    await u.cleanup();
    await u.closeForRestart();
    const cleanExit = inst.proc.exitCode === 0;
    await inst.launch();
    await inst.waitForData();
    await u.installProbe();
    await u.click(u.page().getByRole("radio", { name: "按应用", exact: true }));
    const offline = await query(snapshot => snapshot.summary.by === "agent");
    check("offline_restart_uses_snapshots", { cleanExit, hostAlive: inst.proc.exitCode === null,
      beforeCost: beforeOffline.summary.totals.cost_usd, afterCost: offline.summary.totals.cost_usd,
      beforeRequestCosts: requestCosts(beforeOffline), afterRequestCosts: requestCosts(offline),
      beforeSummaryCostText, afterSummaryCostText: await u.page().locator(".metric-value-main").innerText(),
      externalExists: fs.existsSync(pricingFile(inst.root)), beforeSources: pricedSources(beforeOffline), afterSources: pricedSources(offline),
      beforeSnapshots, afterSnapshots: snapshotFiles(inst.root).map(sha), beforeHits: offlineHits, afterHits: hits(),
      bannerVisible: (await u.page().locator(".pricing-notice").count()) > 0 });

    await pricingView();
    const file = pricingFile(inst.root);
    if (fs.existsSync(file)) throw new Error("离线阶段本应没有外置价格文件");
    const existedBefore = fs.existsSync(file);
    count = (await calls("open_pricing_file")).length;
    await u.click(u.page().getByRole("button", { name: "打开 / 创建外置价格文件", exact: true }));
    const opened = await settledAfter("open_pricing_file", count);
    const processSnapshot = await inst.refreshDescendants(inst.current);
    const descendants = collectDescendants(processSnapshot.processes ?? [], inst.current.hostPid);
    const foreignProcesses = (processSnapshot.processes ?? []).filter(row => descendants.includes(row.pid)
      && row.name.toLowerCase() !== "msedgewebview2.exe");
    const openers = readAcceptanceTrace(inst.root).records.filter(row => row.event === "opener");
    check("open_pricing_file_creates_template_in_root", { existedBefore, created: fs.existsSync(file),
      content: fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "", expectedPath: file,
      returnedPath: opened.status === "fulfilled" ? opened.value : null, openerPaths: openers.map(row => row.path),
      recorded: openers.length === 1 && inst.opts.controlledIo.env.TOKENSCOPE_ACCEPTANCE_OPENER === "record",
      processProbeOk: processSnapshot.ok, foreignProcesses });
    await u.page().screenshot({ path: path.join(inst.opts.shots, "pricing-template.png") });
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    const finishErrors = [];
    try { await u.saveProbe("pricing-final"); }
    catch (error) { finishErrors.push(error); }
    try { await u.cleanup(); }
    catch (error) { finishErrors.push(error); }
    if (finishErrors.length) {
      ev.pricing.probe_cleanup_errors = finishErrors.map(error => String(error?.message ?? error));
      if (!failure) throw finishErrors[0];
    }
  }
}
