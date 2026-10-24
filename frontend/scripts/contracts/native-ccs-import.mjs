// AC04：真实设置 UI 的 CCS 预览/取消/确认/失败恢复；来源仅为隔离合成库。
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createUnifiedContext } from "../lib/native-unified.mjs";
import { ccsVerdict } from "../lib/native-ccs-checks.mjs";
import { readAcceptanceTrace } from "../lib/acceptance-io.mjs";
import { spawnSyncBounded } from "../lib/native-scenario.mjs";

// 只读取本场景隔离 history.db 的已提交事实，日汇总不伪装成请求明细。
const HISTORY_COUNTS = `
import json, sqlite3, sys
from pathlib import Path
from contextlib import closing
with closing(sqlite3.connect(Path(sys.argv[1]).resolve().as_uri() + '?mode=ro', uri=True)) as db:
    events = db.execute('SELECT input_tokens,output_tokens,cache_write_tokens,cache_read_tokens FROM usage_events').fetchall()
    rollups = db.execute('SELECT request_count,input_tokens,output_tokens,cache_write_tokens,cache_read_tokens FROM ccs_daily_usage').fetchall()
    result = dict(requests=len(events),rollups=len(rollups),rollupRequests=sum(int(r[0]) for r in rollups),
        requestTokens=[sum(int(r[i]) for r in events) for i in range(4)],
        rollupTokens=[sum(int(r[i+1]) for r in rollups) for i in range(4)],
        batches=db.execute('SELECT COUNT(*) FROM import_runs WHERE committed_utc IS NOT NULL').fetchone()[0],
        timezones=[r[0] for r in db.execute('SELECT DISTINCT source_tz FROM ccs_daily_usage').fetchall()])
    print(json.dumps(result))
`;

function historyCounts(root) {
  const database = path.join(fs.realpathSync.native(root), "tokenscope", "history.db");
  const result = spawnSyncBounded("python", ["-B", "-c", HISTORY_COUNTS, database], {
    action: "read-isolated-history-counts",
  });
  if (result.status !== 0) throw new Error(`隔离历史取证失败：${result.stderr}`);
  return JSON.parse(result.stdout);
}

export async function nativeCcsImport(context) {
  const { inst, ck, ev } = context;
  const u = createUnifiedContext(context);
  const source = path.join(inst.root, "sources", "cc-switch", "cc-switch.db");
  const hash = () => crypto.createHash("sha256").update(fs.readFileSync(source)).digest("hex");
  const beforeHash = hash();
  ev.ccs = { source, observations: {} };
  const check = (name, observed) => {
    ev.ccs.observations[name] = observed;
    const verdict = ccsVerdict(name, observed);
    ck.add(name, verdict.ok, verdict.basis);
  };
  const panel = () => u.page().locator('[data-section="usage"]');
  const pendingCalls = async (command) =>
    (await u.readProbe()).invokes.filter((row) => row.command === command);
  const settledAfter = async (command, count) => {
    await u.waitUntil(
      async () => {
        const rows = await pendingCalls(command);
        return rows.length > count && rows.at(-1).status !== "pending";
      },
      20000,
      `${command} 的真实回调`,
    );
    return u.last(command);
  };
  const usagePanel = async () => {
    await u.nav("设置");
    await u.section("用量数据");
  };
  const query = async (predicate) => {
    let snapshot;
    await u.waitUntil(
      async () => {
        snapshot = await u.querySnapshot();
        return snapshot.sameQuery && snapshot.domMatches && (!predicate || predicate(snapshot));
      },
      20000,
      "已提交的汇总与明细查询",
    );
    return snapshot;
  };
  const dashboard = async () => {
    await u.nav("汇总");
    return query(
      (snapshot) => snapshot.summary.by === "agent" && snapshot.summary.totals.requests === 14,
    );
  };
  const preview = async () => {
    const count = (await pendingCalls("ccs_import_preview")).length;
    await u.click(panel().getByRole("button", { name: "从 CCS 导入用量", exact: true }));
    return settledAfter("ccs_import_preview", count);
  };
  const commit = async () => {
    const count = (await pendingCalls("ccs_import_commit")).length;
    await u.click(
      panel().locator(".ccs-preview").getByRole("button", { name: "导入", exact: true }),
    );
    const call = await settledAfter("ccs_import_commit", count);
    await u.waitUntil(async () => (await panel().locator(".ccs-preview").count()) === 0);
    if (call.status !== "fulfilled") throw new Error(`CCS 提交失败：${call.error}`);
    return call.value;
  };
  const revealNoticeAction = async (label) => {
    const action = u.page().getByRole("button", { name: label, exact: true });
    if (!(await action.count())) {
      const detail = u
        .page()
        .locator(".source-notice")
        .getByRole("button", { name: "详情", exact: true });
      if (await detail.count()) await u.click(detail);
    }
    await u.click(action);
  };
  let failure;
  try {
    await inst.waitForData();
    await inst.setWindowSize(1280, 820);
    await u.installProbe();
    await u.click(u.page().getByRole("radio", { name: "按应用", exact: true }));
    await query(
      (snapshot) => snapshot.summary.by === "agent" && snapshot.summary.totals.requests === 10,
    );
    const beforeSettings = readAcceptanceTrace(inst.root).records;
    const baseline = historyCounts(inst.root);

    await usagePanel();
    await u.waitUntil(async () =>
      (await panel().locator("#ccs-import-path").inputValue()).endsWith("cc-switch.db"),
    );
    const shownPath = await panel().locator("#ccs-import-path").inputValue();
    const beforePreview = readAcceptanceTrace(inst.root).records;
    check("no_ccs_access_before_click", {
      beforeSettings,
      beforePreview,
      shownPath,
      expectedPath: fs.realpathSync.native(source),
    });
    await u.select('[aria-label="来源统计时区"]', "Asia/Shanghai");
    const firstPreview = await preview();
    check("preview_counts_match_fixture", {
      preview: firstPreview.value,
      text: await panel().locator(".ccs-preview").innerText(),
    });
    const discardCount = (await pendingCalls("ccs_import_discard")).length;
    await u.click(
      panel().locator(".ccs-preview").getByRole("button", { name: "取消", exact: true }),
    );
    const discard = await settledAfter("ccs_import_discard", discardCount);
    check("cancel_discards_without_writing", {
      before: baseline,
      after: historyCounts(inst.root),
      discarded: discard.status === "fulfilled" && discard.value === true,
      planCleared: (await panel().locator(".ccs-preview").count()) === 0,
      commits: (await pendingCalls("ccs_import_commit")).length,
    });

    await preview();
    const report = await commit();
    const imported = historyCounts(inst.root);
    const importedQuery = await dashboard();
    check("confirm_imports_one_batch_as_union", {
      before: baseline,
      after: imported,
      report,
      commits: (await pendingCalls("ccs_import_commit")).length,
      nativeOverlapRows: importedQuery.events.rows.filter(
        (row) => row.record_id === "msg-acceptance-1",
      ).length,
      ccsOnlyRows: importedQuery.events.rows.filter((row) => row.session_id === "ccs-only-session")
        .length,
      detailTotal: importedQuery.events.total,
    });
    await u.page().screenshot({ path: path.join(inst.opts.shots, "ccs-union.png") });

    await usagePanel();
    await preview();
    const repeated = await commit();
    const repeatedCounts = historyCounts(inst.root);
    check("repeat_import_adds_nothing", {
      before: imported,
      after: repeatedCounts,
      report: repeated,
    });

    await u.select('[aria-label="来源统计时区"]', "UTC");
    const timezonePreview = await preview();
    await commit();
    const timezoneCounts = historyCounts(inst.root);
    check("source_timezone_change_is_idempotent", {
      before: repeatedCounts,
      after: timezoneCounts,
      chosenTimezone: "UTC",
      preview: timezonePreview.value,
      storedTimezones: timezoneCounts.timezones,
    });

    await panel()
      .locator("#ccs-import-path")
      .fill(path.join(inst.root, "sources", "cc-switch", "missing.db"));
    const failed = await preview();
    const errorText = await panel().locator(".block-error").innerText();
    const planCleared = (await panel().locator(".ccs-preview").count()) === 0;
    await panel().locator("#ccs-import-path").fill(source);
    const recovered = await preview();
    check("failure_recovers_by_new_preview", {
      failure: failed,
      errorText,
      planCleared,
      recovered: recovered.value,
      previewVisible: (await panel().locator(".ccs-preview").count()) === 1,
    });
    await u.click(
      panel().locator(".ccs-preview").getByRole("button", { name: "取消", exact: true }),
    );
    const automatic = await dashboard();
    const unknownText = await u.page().locator("body").innerText();
    await u.click(u.page().getByRole("radio", { name: "按时间", exact: true }));
    await query(
      (snapshot) =>
        snapshot.summary.by === "day" &&
        Boolean(snapshot.summary.rollup_coverage?.timezone_mismatch),
    );
    await revealNoticeAction("只看请求明细");
    await query(
      (snapshot) =>
        snapshot.summary.by === "day" && snapshot.beginCall?.args?.rollups === "detail_only",
    );
    // 按日遇到时区冲突时原本就排除日汇总，不能拿它证明 detail_only。
    // 三份最终比较都使用按应用：auto 14 -> detail_only 11 -> auto 14。
    await u.click(u.page().getByRole("radio", { name: "按应用", exact: true }));
    const detail = await query(
      (snapshot) =>
        snapshot.summary.by === "agent" &&
        snapshot.beginCall?.args?.rollups === "detail_only" &&
        snapshot.summary.totals.requests === 11,
    );
    const detailText = await u.page().locator("body").innerText();
    await revealNoticeAction("恢复自动参与");
    const restored = await query(
      (snapshot) =>
        snapshot.summary.by === "agent" &&
        snapshot.beginCall?.args?.rollups === null &&
        snapshot.summary.totals.requests === 14,
    );
    check("detail_only_hides_rollups_and_discloses_granularity", {
      autoRequests: automatic.summary.totals.requests,
      detailRequests: detail.summary.totals.requests,
      restoredRequests: restored.summary.totals.requests,
      autoBy: automatic.summary.by,
      detailBy: detail.summary.by,
      restoredBy: restored.summary.by,
      domMatches: [automatic.domMatches, detail.domMatches, restored.domMatches],
      initialAutoMode: automatic.beginCall?.args?.rollups,
      detailMode: detail.beginCall?.args?.rollups,
      autoMode: restored.beginCall?.args?.rollups,
      unknownText,
      detailText,
    });
    await u.page().screenshot({ path: path.join(inst.opts.shots, "ccs-restored.png") });
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    const finishErrors = [];
    try { check("ccs_fixture_unchanged", { before: beforeHash, after: hash() }); }
    catch (error) { finishErrors.push(error); }
    try { await u.saveProbe("ccs-final"); }
    catch (error) { finishErrors.push(error); }
    try { await u.cleanup(); }
    catch (error) { finishErrors.push(error); }
    if (finishErrors.length) {
      ev.ccs.probe_cleanup_errors = finishErrors.map(error => String(error?.message ?? error));
      if (!failure) throw finishErrors[0];
    }
  }
}
