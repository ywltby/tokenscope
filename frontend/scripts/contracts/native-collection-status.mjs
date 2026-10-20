// PR2（2026-10-15 计划 F04）：真实采集生命周期 + 读取显示期的原生验收。
//
// 真 WebView2 + 真 Rust 后端 + 隔离根。所有轮次都是后台定时线程对合成
// JSONL 的真实采集（不伪造前端事件）；慢轮次与失败由验收构建的受控注入
// 驱动（TOKENSCOPE_ACCEPTANCE_COLLECT_DELAY_MS / _COLLECT_FAIL_ON_ROUND，
// 生产构建没有这条通道）。1000ms 精确阈值由可控时钟单测证明；这里按
// 低频轮询取证并记录观察误差，不把截图间隔当毫秒证据。
// S00（PR3 计划 §9.1 第 3、4 条）：重启段阶段标记、有界同步命令与有界 CDP 关闭。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  observeCollectionCallbacks,
  queryPublishesDuringReading,
} from "../lib/collection-publish-window.mjs";
import {
  closeBrowserBounded,
  ensureNotCanceled,
  spawnSyncBounded,
} from "../lib/native-scenario.mjs";

const POLL_MS = 100;
const POLL_FAST_MS = 40;

export async function nativeCollectionStatus({
  inst,
  ck,
  ev,
  markStage,
  markStageDone,
  isCanceled,
}) {
  // 场景级统一纪元：所有时间线的 at_ms 可跨启动比较（轮询窗口不重置）。
  const EPOCH = Date.now();
  const atNow = () => Date.now() - EPOCH;
  const page = () => inst.page;
  const root = () => page().locator("[data-summary-query]");
  const click = async (locator) => {
    await locator.scrollIntoViewIfNeeded();
    await locator.evaluate((el) => el.scrollIntoView({ block: "center" }));
    await locator.click();
  };
  const navigate = (label) =>
    click(
      page().locator('[aria-label="页面切换"]').getByRole("radio", { name: label, exact: true }),
    );
  const section = (label) =>
    click(
      page()
        .locator('[aria-label="设置分区"]')
        .getByRole("button", { name: label, exact: true })
        .or(
          page()
            .locator('[aria-label="设置分区"]')
            .getByRole("radio", { name: label, exact: true }),
        ),
    );
  const interval = async (label) => {
    await click(page().locator('[aria-label="自动读取间隔"]'));
    await click(
      page()
        .locator(".n-base-select-option")
        .filter({ hasText: new RegExp(`^${label}$`) }),
    );
  };
  const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
  const savedInterval = (secs) =>
    new RegExp(`^collect_interval_secs\\s*=\\s*${secs}\\s*$`, "m").test(
      fs.readFileSync(settingsPath, "utf8"),
    );
  const waitUntil = async (predicate, timeout = 30000, step = POLL_MS) => {
    const deadline = Date.now() + timeout;
    while (!(await predicate())) {
      if (Date.now() >= deadline) throw new Error("native UI condition timed out");
      if (inst.proc.exitCode !== null) throw new Error("native application exited");
      await new Promise((resolve) => setTimeout(resolve, step));
    }
  };
  /** 一次 DOM 取样：真实采集状态（runId/revision 由事件驱动更新）与查询身份。
   *  data-collection-* 挂在状态胶囊上；data-requests / query 身份在汇总容器上。 */
  const sample = async (atMs) =>
    page().evaluate((at_ms) => {
      const pill = document.querySelector(".ts-status-pill");
      const root = document.querySelector("[data-summary-query]");
      if (!pill || !root) throw new Error("collection sample DOM missing");
      return {
        at_ms,
        page_ms: performance.now(),
        state: pill.getAttribute("data-collection-state"),
        text: pill.textContent.trim(),
        revision: Number(pill.getAttribute("data-collection-revision") || 0),
        runId: Number(pill.getAttribute("data-collection-run-id") || 0),
        generation: Number(pill.getAttribute("data-collection-generation") || 0),
        requests: Number(root.getAttribute("data-requests") || 0),
        summaryQuery: root.getAttribute("data-summary-query"),
        eventsQuery: root.getAttribute("data-events-query"),
      };
    }, atMs);
  /** 连续取样直到 predicate 成立；全部样本进时间线（证据不打折）。 */
  const pollUntil = async (timeline, predicate, timeout, step = POLL_MS) => {
    const started = Date.now();
    for (;;) {
      const s = await sample(atNow());
      timeline.push(s);
      if (predicate(s)) return s;
      if (Date.now() - started >= timeout) throw new Error("poll condition timed out");
      if (inst.proc.exitCode !== null) throw new Error("native application exited");
      await new Promise((resolve) => setTimeout(resolve, step));
    }
  };
  const appendClaudeEvent = (id, input, output) => {
    const dir = path.join(inst.root, "sources", "claude", "projects", "collection-status");
    fs.mkdirSync(dir, { recursive: true });
    const event = {
      type: "assistant",
      timestamp: new Date().toISOString(),
      sessionId: `collection-status-${id}`,
      cwd: "C:/acceptance/workspace/collection-status",
      message: {
        id,
        model: "claude-sonnet-4-5",
        usage: {
          input_tokens: input,
          output_tokens: output,
          cache_creation_input_tokens: 7,
          cache_read_input_tokens: 11,
        },
      },
    };
    fs.writeFileSync(path.join(dir, `${id}.jsonl`), JSON.stringify(event) + "\n", "utf8");
    return { id, input_tokens: input, output_tokens: output, cache_write: 7, cache_read: 11 };
  };
  /** 从时间线按 runId 分组，给出每个真实轮次的显示窗口（观察值）。 */
  const roundsOf = (timeline) => {
    const rounds = [];
    for (const s of timeline) {
      if (!s.runId) continue;
      let r = rounds.find((x) => x.runId === s.runId);
      if (!r) {
        r = { runId: s.runId, firstSeenMs: s.at_ms, states: new Set() };
        rounds.push(r);
      }
      r.states.add(s.state);
      if (s.state === "reading" && r.readingStartMs === undefined) r.readingStartMs = s.at_ms;
      if (r.readingStartMs !== undefined && s.state !== "reading" && r.displayEndMs === undefined)
        r.displayEndMs = s.at_ms;
      r.lastSeenMs = s.at_ms;
      r.generation = s.generation || r.generation;
    }
    for (const r of rounds)
      r.observedDisplayMs =
        r.readingStartMs !== undefined && r.displayEndMs !== undefined
          ? r.displayEndMs - r.readingStartMs
          : null;
    return rounds;
  };
  const closeForRestart = async () => {
    const action = fileURLToPath(
      new URL("../../../scripts/native-window-action.ps1", import.meta.url),
    );
    ensureNotCanceled(isCanceled, "close-command");
    markStage("close-command");
    const result = spawnSyncBounded(
      "pwsh",
      ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", "close"],
      { action: "window-close", stage: "close-command" },
    );
    markStageDone("close-command");
    if (result.status !== 0 || !/CLOSED true/i.test(result.stdout))
      throw new Error(`WM_CLOSE failed: ${result.stderr} ${result.stdout}`);
    ensureNotCanceled(isCanceled, "click-quit");
    markStage("click-quit");
    await click(
      page().locator(".close-dialog").getByRole("button", { name: "直接退出", exact: true }),
    );
    markStageDone("click-quit");
    markStage("wait-host-exit");
    const deadline = Date.now() + 15000;
    while (inst.proc.exitCode === null && Date.now() < deadline)
      await new Promise((resolve) => setTimeout(resolve, 100));
    markStageDone("wait-host-exit");
    if (inst.proc.exitCode !== 0)
      throw new Error(`restart requires clean exit, got ${inst.proc.exitCode}`);
    // S00：CDP 关闭 10 秒上限，超时记为 close timeout 后继续（不悬挂）
    await closeBrowserBounded(inst.browser);
  };

  ev.collection_status = { launches: [], appended_events: [] };

  // ── 启动 1：正常构建路径——无变化两轮 + 有变化快轮（查询先于动画结束）──
  await inst.waitForData();
  await inst.setWindowSize(980, 620);
  ev.initialViewport = await page().evaluate(() => ({
    width: innerWidth,
    height: innerHeight,
    dpr: devicePixelRatio,
  }));
  await navigate("设置");
  await section("数据源");
  await interval("5 秒");
  await waitUntil(
    async () => savedInterval(5) && (await page().locator(".interval-row .tz-state").count()) === 0,
  );
  await navigate("汇总");

  // 无变化：至少两个真实轮次（后台定时采集对未变来源），统计与查询身份不动。
  const baseline = await sample(atNow());
  const t1 = [];
  await pollUntil(
    t1,
    (s) => {
      const rounds = roundsOf(t1).filter((r) => r.observedDisplayMs !== null);
      return rounds.length >= 2;
    },
    30000,
  );
  const rounds1 = roundsOf(t1);
  const fullRounds = rounds1.filter((r) => r.observedDisplayMs !== null);
  ck.add(
    "no_change_rounds_start_end_pairs",
    fullRounds.length >= 2,
    `观察到的完整显示轮次=${fullRounds.length}（runId=${fullRounds.map((r) => r.runId).join(",")}）`,
  );
  ck.add(
    "no_change_display_at_least_one_second",
    fullRounds.every((r) => r.observedDisplayMs >= 900),
    `各轮显示窗口(ms)=${fullRounds.map((r) => r.observedDisplayMs).join(",")}（轮询 100ms，1000ms 精确阈值由可控时钟单测证明）`,
  );
  const lastSample = t1.at(-1);
  ck.add(
    "no_change_keeps_stats_and_query_identity",
    lastSample.requests === baseline.requests &&
      lastSample.summaryQuery === baseline.summaryQuery &&
      lastSample.eventsQuery === baseline.eventsQuery,
    `requests=${baseline.requests}→${lastSample.requests}；query 不变=${lastSample.summaryQuery === baseline.summaryQuery}`,
  );
  ck.add(
    "no_change_generation_stable",
    lastSample.generation === baseline.generation,
    `generation=${baseline.generation}→${lastSample.generation}`,
  );

  // 有变化：追加一条身份写死的合成用量，不点刷新——统计 +1、代次前进、
  // 汇总与明细同 query_id，且终态处理结束前已经发起查询（查询不等待动画）。
  // 仅观察回调；不延长动画、不伪造状态、不改变真实 IPC。
  await page().evaluate(
    `window.__collectionProbe = (${observeCollectionCallbacks.toString()})(window.__TAURI_INTERNALS__, () => performance.now())`,
  );
  const appended = appendClaudeEvent("cs-fast-1", 310, 17);
  ev.collection_status.appended_events.push({ phase: "fast", ...appended });
  const beforeFast = await sample(atNow());
  const t2 = [];
  try {
    await pollUntil(t2, (s) => s.requests === beforeFast.requests + 1, 25000, POLL_FAST_MS);
  } finally {
    ev.collection_status.callback_trace = await page().evaluate(() => {
      const trace = window.__collectionProbe.read();
      window.__collectionProbe.dispose();
      delete window.__collectionProbe;
      return trace;
    });
  }
  const bump = t2.find((s) => s.requests === beforeFast.requests + 1);
  ck.add(
    "changed_round_bumps_stats_without_manual_refresh",
    true,
    `requests ${beforeFast.requests} → ${beforeFast.requests + 1}；身份=${appended.id}`,
  );
  // 不以 DOM 轮询推断 IPC 开始：用同一页面时钟的真实回调边界与查询身份。
  const publishJudgment = queryPublishesDuringReading({
    timeline: t2,
    baseline: beforeFast,
    trace: ev.collection_status.callback_trace,
  });
  ck.add("query_publishes_within_reading_window", publishJudgment.ok, publishJudgment.basis);
  ck.add(
    "changed_round_advances_generation_and_run",
    bump.generation === beforeFast.generation + 1 && bump.runId > beforeFast.runId,
    `generation=${beforeFast.generation}→${bump.generation} runId=${beforeFast.runId}→${bump.runId}`,
  );
  ck.add(
    "summary_and_events_share_new_query_id",
    bump.summaryQuery === bump.eventsQuery && bump.summaryQuery !== beforeFast.summaryQuery,
    bump.summaryQuery,
  );
  ev.collection_status.launches.push({
    launch: 1,
    baseline,
    rounds: rounds1,
    timeline: t1.concat(t2),
  });

  // ── 启动 2：验收注入——慢轮次持续到真实完成、失败立即停、减少动态效果 ──
  inst.opts.env = {
    TOKENSCOPE_ACCEPTANCE_COLLECT_DELAY_MS: "2500",
    TOKENSCOPE_ACCEPTANCE_COLLECT_FAIL_ON_ROUND: "2",
  };
  await closeForRestart();
  await inst.launch();
  await inst.waitForData(60000);
  // 减少动态效果：Playwright 媒体仿真（与浏览器契约同一机制）。
  await page().emulateMedia({ reducedMotion: "reduce" });
  ev.reduced_motion = "emulated";
  const t3 = [];
  // 第 2 轮（启动采集是第 1 轮）：注入延迟 2.5s 后失败——显示持续到真实
  // 结束、失败立即停止（不用最短期掩盖）、读取失败通知可见。
  await pollUntil(t3, (s) => s.state === "failed", 40000);
  const rounds2 = roundsOf(t3);
  const failedRun = rounds2.find((r) => r.states.has("failed"));
  ck.add(
    "slow_round_displays_until_real_end",
    failedRun && failedRun.observedDisplayMs !== null && failedRun.observedDisplayMs >= 2300,
    `失败轮显示窗口(ms)=${failedRun?.observedDisplayMs}（注入 2500ms；轮询 100ms）`,
  );
  const failedSample = t3.find((s) => s.state === "failed");
  ck.add(
    "failure_shows_operable_notice",
    failedSample.text === "读取失败" ||
      (await page().locator(".ts-notice").first().innerText()).includes("读取失败"),
    `胶囊="${failedSample.text}"`,
  );
  // 第 3 轮：同样注入延迟但正常成功——减少动态效果下静态图标 + 文字仍
  // 满足最短期（轮询显示 reading ≥ 2.3s），随后恢复"已更新"并清除采集错误。
  const t4 = [];
  await pollUntil(t4, (s) => s.state === "reading" && s.runId > failedRun.runId, 40000);
  // 读取期间取样旋转动画：减少动态效果时必须是静态（animation-name: none）。
  const spinnerAnim = await page().evaluate(() => {
    const el = document.querySelector(".ts-pill-spinner");
    return el ? getComputedStyle(el).animationName : "<no-spinner>";
  });
  ck.add(
    "reduced_motion_spinner_not_animated",
    spinnerAnim === "none",
    `animation-name=${spinnerAnim}`,
  );
  await pollUntil(t4, (s) => s.state === "confirmed" && s.runId > failedRun.runId, 40000);
  const rounds3 = roundsOf(t4);
  const recoveredRun = rounds3.find((r) => r.runId > failedRun.runId && r.states.has("confirmed"));
  ck.add(
    "reduced_motion_display_still_minimum",
    recoveredRun &&
      recoveredRun.observedDisplayMs !== null &&
      recoveredRun.observedDisplayMs >= 2300,
    `恢复轮显示窗口(ms)=${recoveredRun?.observedDisplayMs}（注入 2500ms）`,
  );
  await waitUntil(async () => {
    const notice = await page()
      .locator(".ts-notice")
      .first()
      .innerText()
      .catch(() => "");
    return !notice.includes("读取失败");
  }, 15000);
  const afterRecovery = await sample(atNow());
  ck.add(
    "failure_recovers_on_next_success",
    afterRecovery.state === "confirmed" && afterRecovery.text === "已更新",
    `恢复后=${afterRecovery.state}/${afterRecovery.text}`,
  );
  ev.collection_status.launches.push({ launch: 2, timeline: t3.concat(t4), rounds: rounds2 });

  // ── 启动 3：正常路径——已加载更多明细时，新数据只提示不刷新 ──
  inst.opts.env = {};
  await closeForRestart();
  await inst.launch();
  await inst.waitForData(60000);
  await page().emulateMedia({ reducedMotion: "no-preference" });
  const list = page().getByRole("region", { name: "请求明细列表" });
  await list.focus();
  const rowsBefore = await page().locator(".events-card tbody tr").count();
  await list.press("End");
  await waitUntil(
    async () => (await page().locator(".events-card tbody tr").count()) > rowsBefore,
    20000,
  );
  const paged = await sample(atNow());
  ck.add(
    "paged_session_has_more_rows",
    paged.requests > 200 && (await page().locator(".events-card tbody tr").count()) > 200,
    `requests=${paged.requests}，已渲染行=${await page().locator(".events-card tbody tr").count()}`,
  );
  // 追加新事实：翻页保护让位——出现"有新数据 · 刷新"提示而不是自动刷新。
  const appendedPaged = appendClaudeEvent("cs-paged-1", 220, 9);
  ev.collection_status.appended_events.push({ phase: "paged", ...appendedPaged });
  const t5 = [];
  await pollUntil(t5, (s) => s.state === "reading" || s.text === "读取失败", 20000);
  await waitUntil(
    async () => (await page().getByRole("button", { name: "有新数据 · 刷新" }).count()) > 0,
    20000,
  );
  const hintSample = await sample(atNow());
  ck.add(
    "paged_new_data_shows_hint_not_auto_refresh",
    hintSample.requests === paged.requests &&
      hintSample.requests === paged.requests &&
      hintSample.summaryQuery === paged.summaryQuery,
    `提示可见且统计未动：requests=${paged.requests}→${hintSample.requests} query=${hintSample.summaryQuery}`,
  );
  ck.add(
    "paged_round_still_shows_reading",
    t5.some((s) => s.state === "reading"),
    `轮次显示态样本=${t5.filter((s) => s.state === "reading").length}`,
  );
  await click(page().getByRole("button", { name: "有新数据 · 刷新" }));
  await waitUntil(async () => {
    const s = await sample(0);
    return s.requests === paged.requests + 1 && s.summaryQuery !== paged.summaryQuery;
  }, 25000);
  const refreshed = await sample(atNow());
  ck.add(
    "paged_hint_click_refreshes_one_batch",
    refreshed.requests === paged.requests + 1 && refreshed.summaryQuery === refreshed.eventsQuery,
    `requests=${paged.requests}→${refreshed.requests}；query=${refreshed.summaryQuery}`,
  );
  ev.collection_status.launches.push({ launch: 3, timeline: t5, paged_before: paged, refreshed });
  await page().screenshot({
    path: path.join(inst.opts.shots, "ui-collection-status-final.png"),
  });
}
