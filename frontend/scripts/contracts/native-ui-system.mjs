// 真实桌面窗口恢复验收；不调用系统休眠、不改系统时钟或显示缩放。
// S00（PR3 计划 §9.1 第 3、4 条）：关闭/点击退出/等待退出各打阶段标记；同步命令有界。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { ensureNotCanceled, spawnSyncBounded } from "../lib/native-scenario.mjs";

export async function nativeUiSystem({ inst, ck, ev, markStage, markStageDone, isCanceled }) {
  const page = () => inst.page;
  const root = () => page().locator("[data-summary-query]");
  const query = () => root().getAttribute("data-summary-query");
  // Tauri 的 invoke 属性不可改写；观察真实 IPC 请求，不替换函数或响应。
  // 每次启动独立计数，且预设切换必须增加一次，防止失效的计数器假通过。
  const queryCounts = new WeakMap();
  const queryBegins = () => {
    const current = page();
    if (!queryCounts.has(current)) {
      const counter = { begins: 0 };
      queryCounts.set(current, counter);
      current.on("request", (request) => {
        const url = new URL(request.url());
        if ((url.hostname === "ipc.localhost" || url.protocol === "ipc:") &&
          url.pathname === "/query_begin") counter.begins++;
      });
    }
    return queryCounts.get(current).begins;
  };
  const notifyGeneration = (delta = 0) => page().evaluate(async (extra) => {
    const raw = document.querySelector("[data-collection-generation]")?.dataset.collectionGeneration;
    const generation = Number(raw);
    if (!raw || !Number.isFinite(generation)) throw new Error("缺少真实采集代次");
    await window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
      event: "usage-updated",
      payload: { generation: generation + extra },
    });
  }, delta);
  const navigate = (label) =>
    page()
      .locator('[aria-label="页面切换"]')
      .getByRole("radio", { name: label, exact: true })
      .click();
  const waitQuery = (old) =>
    page().waitForFunction(
      (previous) => {
        const node = document.querySelector("[data-summary-query]");
        return (
          !!node?.dataset.summaryQuery &&
          node?.dataset.summaryQuery !== previous &&
          node?.dataset.summaryQuery === node?.dataset.eventsQuery
        );
      },
      old,
      { timeout: 10000 },
    );
  const range = async (label) => {
    const old = await query();
    const begins = await queryBegins();
    await page().locator(".range-trigger").click();
    await page().getByRole("button", { name: label, exact: true }).click();
    await waitQuery(old);
    const after = queryBegins();
    ck.add(
      `preset_${label}_starts_one_query`,
      after === begins + 1,
      `query_begin ${begins} -> ${after}`,
    );
  };
  const restore = async (checkHidden = false) => {
    const action = fileURLToPath(
      new URL("../../../scripts/native-window-action.ps1", import.meta.url),
    );
    const change = (state) => {
      const result = spawnSyncBounded(
        "pwsh",
        ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", state],
        { action: `window-${state}` },
      );
      if (result.status !== 0 || !/STATE_CHANGED true/i.test(result.stdout))
        throw new Error(`Native ${state} failed: ${result.stdout} ${result.stderr}`);
    };
    change("minimize");
    ev.minimizedVisibility = await page().evaluate(() => document.visibilityState);
    await new Promise((resolve) => setTimeout(resolve, 1200));
    if (checkHidden) {
      const hiddenQuery = await query();
      const begins = queryBegins();
      for (let n = 0; n < 3; n++) await notifyGeneration(1);
      await new Promise((resolve) => setTimeout(resolve, 1500));
      ck.add(
        "native_minimized_notifications_do_not_query",
        (await query()) === hiddenQuery && queryBegins() === begins,
        `${hiddenQuery} -> ${await query()}; query_begin ${begins} -> ${queryBegins()}; newer notifications via real event bus`,
      );
    }
    const started = Date.now();
    change("restore");
    await page().bringToFront();
    await page().waitForFunction(() => document.visibilityState === "visible");
    return started;
  };

  await inst.waitForData();
  await inst.setWindowSize(980, 620);
  ev.hostGeometry = await inst.assertHostFilled();
  ck.add("webview_fills_native_client", true, JSON.stringify(ev.hostGeometry));
  // PR2：状态胶囊为静态「已更新」（取消常驻"更新于 HH:MM:SS"）——真实
  // Rust 查询落到界面的证明改为胶囊文案 + 汇总数据容器。
  ck.add(
    "real_query_status_pill_rendered",
    (await page().locator(".ts-status-pill").first().innerText()).trim() === "已更新",
    "actual Rust query result rendered (PR2 static pill)",
  );
  await page().screenshot({ path: path.join(inst.opts.shots, "native-resized.png") });
  await navigate("设置");
  await page().locator('[aria-label="聚合/展示时区"]').click();
  await page().locator(".n-base-select-option").filter({ hasText: /^UTC$/ }).click();
  await page().waitForFunction(() => !document.querySelector('[data-section="general"] .tz-state'));
  const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
  const savedUtc = () => /^timezone\s*=\s*"UTC"\s*$/m.test(fs.readFileSync(settingsPath, "utf8"));
  ck.add("timezone_click_saved_to_toml", savedUtc(), "real settings dropdown -> isolated TOML UTC");
  await navigate("汇总");
  await inst.waitForData();
  await range("24h");
  const before = await query();
  const resumed = await restore(true);
  await waitQuery(before);
  ev.restoreLatencyMs = Date.now() - resumed;
  ck.add(
    "window_restore_refreshes_rolling_immediately",
    ev.restoreLatencyMs < 10000,
    `${before} -> ${await query()}; ${ev.restoreLatencyMs} ms`,
  );
  ck.add(
    "restored_results_share_query",
    (await query()) === (await root().getAttribute("data-events-query")),
    await query(),
  );

  await range("全部时间");
  // N14：落地后直接取基线，不再等待迟到批次稳定。通过真实事件总线重放
  // 当前事务已覆盖的代次，并在恢复/切页后同时核对显示身份和 begin 计数。
  const allQuery = await query();
  const allBegins = queryBegins();
  await notifyGeneration();
  await restore();
  await new Promise((resolve) => setTimeout(resolve, 1500));
  ck.add(
    "all_time_restore_does_not_query",
    (await query()) === allQuery,
    await query(),
  );
  ck.add(
    "covered_notification_and_restore_do_not_begin_query",
    queryBegins() === allBegins,
    `baseline=${allBegins}; current=${queryBegins()}`,
  );
  await page().mouse.move(500, 400);
  await page().mouse.wheel(0, 700);
  await page().waitForFunction(() => document.querySelector(".scroll-container").scrollTop > 0);
  const scrollBefore = await page()
    .locator(".scroll-container")
    .evaluate((el) => el.scrollTop);
  await navigate("设置");
  await navigate("汇总");
  await page().waitForFunction(
    (expected) => Math.abs(document.querySelector(".scroll-container").scrollTop - expected) <= 2,
    scrollBefore,
  );
  // 切页后不建查询，身份仍等于预设切换落地时的基线。
  ck.add(
    "page_switch_preserves_scroll_and_query",
    scrollBefore > 0 && (await query()) === allQuery &&
      queryBegins() === allBegins,
    `scroll=${scrollBefore}; query=${await query()}; baseline=${allQuery}`,
  );
  ev.viewport = await page().evaluate(() => ({
    width: innerWidth,
    height: innerHeight,
    dpr: devicePixelRatio,
  }));
  ev.restoredHostGeometry = await inst.assertHostFilled();
  ck.add(
    "no_horizontal_overflow",
    await page().evaluate(
      () =>
        document.documentElement.scrollWidth <= innerWidth &&
        document.querySelector(".scroll-container").scrollWidth <=
          document.querySelector(".scroll-container").clientWidth,
    ),
    JSON.stringify(ev.viewport),
  );
  await page().screenshot({ path: path.join(inst.opts.shots, "system-restored.png") });

  const action = fileURLToPath(
    new URL("../../../scripts/native-window-action.ps1", import.meta.url),
  );
  // S00：关闭 → 点击退出 → 等待退出各记一段；取消标志已置位时不得继续产生副作用
  ensureNotCanceled(isCanceled, "close-command");
  markStage("close-command");
  const close = spawnSyncBounded(
    "pwsh",
    ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", "close"],
    { action: "window-close", stage: "close-command" },
  );
  markStageDone("close-command");
  if (close.status !== 0 || !/CLOSED true/i.test(close.stdout)) throw new Error("WM_CLOSE failed");
  ensureNotCanceled(isCanceled, "click-quit");
  markStage("click-quit");
  await page()
    .locator(".close-dialog")
    .getByRole("button", { name: "直接退出", exact: true })
    .click();
  markStageDone("click-quit");
  markStage("wait-host-exit");
  const deadline = Date.now() + 15000;
  while (inst.proc.exitCode === null && Date.now() < deadline)
    await new Promise((resolve) => setTimeout(resolve, 100));
  markStageDone("wait-host-exit");
  if (inst.proc.exitCode !== 0)
    throw new Error(`expected clean shutdown, got ${inst.proc.exitCode}`);
  // 重启前确认旧实例真的退出、端口已释放（升级矩阵 §4.8 第 2 条）：走同一个 stop/launch
  // （stop/launch 内部各自记录「stop」「relaunch」阶段）
  const restartStop = await inst.stop({ terminate: false, reason: "ui-system-restart" });
  const restarted = await inst.launch();
  ev.restartStop = restartStop;
  ev.restartLaunch = { index: restarted.launchIndex, port: restarted.port, pid: restarted.pid };
  await inst.waitForData();
  await navigate("设置");
  ck.add(
    "timezone_click_survives_restart",
    savedUtc() &&
      (await page().locator('[aria-label="聚合/展示时区"]').innerText()).includes("UTC"),
    "TOML and restarted dropdown both UTC",
  );
  await page().screenshot({ path: path.join(inst.opts.shots, "timezone-restarted.png") });
  await navigate("汇总");
  await inst.waitForData();
  await range("24h");
  ev.beforeTrayHideQuery = await query();
  const trayClose = spawnSyncBounded(
    "pwsh",
    ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", "close"],
    { action: "window-close-tray" },
  );
  if (trayClose.status !== 0 || !/CLOSED true/i.test(trayClose.stdout))
    throw new Error("tray WM_CLOSE failed");
  await page().getByRole("button", { name: "最小化到托盘", exact: true }).click();
  await new Promise((resolve) => setTimeout(resolve, 1200));
  const trayVisible = await page().evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("plugin:window|is_visible", { label: "main" }),
  );
  ck.add("tray_window_is_hidden", trayVisible === false, String(trayVisible));
  // 隐藏前因焦点变化发出的在途查询允许完成；从确认隐藏后开始量测新请求。
  const trayQuery = await query();
  const trayBegins = queryBegins();
  await notifyGeneration(1);
  await new Promise((resolve) => setTimeout(resolve, 1500));
  ck.add(
    "tray_notifications_do_not_query",
    queryBegins() === trayBegins && (await query()) === trayQuery,
    `${trayQuery} -> ${await query()}; query_begin ${trayBegins} -> ${queryBegins()}`,
  );
  ev.scope =
    "real minimize/restore and focus; not OS sleep; explicit --scale is layout emulation only";
}

/**
 * UP09（升级矩阵 §4.8 第 7 条）：多路重启压力验证，只在本地跑，不进 CI（D5）。
 *
 * 首次启动后每轮：直接退出 → 确认退出（宿主与全部已记录后代退出、端口释放）→
 * 重启（新端口，端口不复用）→ 等待数据 → 核对设置仍在。
 * 轮数由 `--relaunch-cycles N` 指定（默认 20）；单次启动预算与退出确认都走同一套
 * `Instance.launch()` / `Instance.stop()`，不另写启动逻辑。
 */
export async function nativeUiRelaunch({ inst, ck, ev, markStage, markStageDone, isCanceled }) {
  const page = () => inst.page;
  const cycles = Number(inst.opts.relaunchCycles ?? 20);
  const action = fileURLToPath(
    new URL("../../../scripts/native-window-action.ps1", import.meta.url),
  );
  const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
  const savedUtc = () => /^timezone\s*=\s*"UTC"\s*$/m.test(fs.readFileSync(settingsPath, "utf8"));
  const navigate = (label) =>
    page()
      .locator('[aria-label="页面切换"]')
      .getByRole("radio", { name: label, exact: true })
      .click();
  const windowAction = (state) => {
    const result = spawnSyncBounded(
      "pwsh",
      ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", state],
      { action: `window-${state}` },
    );
    if (result.status !== 0 || !/(CLOSED|STATE_CHANGED) true/i.test(result.stdout))
      throw new Error(`Native ${state} failed: ${result.stdout} ${result.stderr}`);
    return (result.stdout || "").trim();
  };
  /** 走应用自己的「直接退出」，等宿主真实退出（退出码由父进程句柄给出）。 */
  const exitThroughDialog = async () => {
    ensureNotCanceled(isCanceled, "close-command");
    markStage("close-command");
    const close = windowAction("close");
    markStageDone("close-command");
    ensureNotCanceled(isCanceled, "click-quit");
    markStage("click-quit");
    await page()
      .locator(".close-dialog")
      .getByRole("button", { name: "直接退出", exact: true })
      .click();
    markStageDone("click-quit");
    markStage("wait-host-exit");
    const deadline = Date.now() + 20000;
    while (inst.proc.exitCode === null && Date.now() < deadline)
      await new Promise((resolve) => setTimeout(resolve, 100));
    markStageDone("wait-host-exit");
    return { close, exitCode: inst.proc.exitCode };
  };

  await inst.waitForData();
  // 播种一个可核对的持久化设置：真实下拉把隔离 TOML 写成 UTC
  await navigate("设置");
  await page().locator('[aria-label="聚合/展示时区"]').click();
  await page().locator(".n-base-select-option").filter({ hasText: /^UTC$/ }).click();
  await page().waitForFunction(() => !document.querySelector('[data-section="general"] .tz-state'));
  ck.add("relaunch_seeds_persisted_setting", savedUtc(), "真实下拉把隔离 TOML 写成 UTC");
  await navigate("汇总");
  await inst.waitForData();

  const first = inst.launches[0];
  ev.firstLaunch = { index: first?.index ?? null, port: first?.port ?? null, pid: first?.hostPid ?? null };
  ev.cycles = [];
  for (let cycle = 1; cycle <= cycles; cycle++) {
    const started = Date.now();
    const row = {
      cycle,
      close: null,
      exitCode: null,
      stop: null,
      port: inst.current?.port ?? null,
      relaunchPort: null,
      relaunchPid: null,
      dataReadyMs: null,
      settingStillUtc: null,
      error: null,
    };
    try {
      const exit = await exitThroughDialog();
      row.close = exit.close;
      row.exitCode = exit.exitCode;
      // 确认退出：宿主与全部已记录后代退出、端口不再监听（§4.8 第 2 条）
      row.stop = await inst.stop({ terminate: false, reason: `ui-relaunch-cycle-${cycle}` });
      const info = await inst.launch();
      row.relaunchPort = info.port;
      row.relaunchPid = info.pid;
      await inst.waitForData();
      row.dataReadyMs = Date.now() - started;
      await navigate("设置");
      const dropdown = await page().locator('[aria-label="聚合/展示时区"]').innerText();
      row.settingStillUtc = savedUtc() && dropdown.includes("UTC");
      await navigate("汇总");
      await inst.waitForData();
    } catch (error) {
      row.error = String(error?.message ?? error);
      ev.cycles.push(row);
      throw error;
    }
    ev.cycles.push(row);
    if (cycle % 5 === 0 || cycle === cycles)
      process.stderr.write(
        `[ui-relaunch] ${cycle}/${cycles} 轮完成：port=${row.relaunchPort} exit=${row.exitCode} data=${row.dataReadyMs} ms\n`,
      );
  }

  const ports = inst.launches.map((l) => l.port);
  ev.portSequence = ports;
  ev.lastLaunchPort = inst.current?.port ?? null;
  ck.add(
    "relaunch_cycles_completed",
    ev.cycles.length === cycles && ev.cycles.every((c) => !c.error),
    `${ev.cycles.length}/${cycles} 轮完成`,
  );
  ck.add(
    "relaunch_ports_never_reused",
    ports.length === cycles + 1 && new Set(ports).size === ports.length,
    `${ports.length} 次启动的端口：${ports.join(",")}`,
  );
  ck.add(
    "relaunch_exits_are_clean",
    ev.cycles.every((c) => c.exitCode === 0),
    ev.cycles.map((c) => `${c.cycle}:${c.exitCode}`).join(" "),
  );
  ck.add(
    "relaunch_settings_survive_every_cycle",
    ev.cycles.every((c) => c.settingStillUtc === true),
    `${ev.cycles.filter((c) => c.settingStillUtc).length}/${ev.cycles.length} 轮重启后仍是 UTC`,
  );
  await page().screenshot({ path: path.join(inst.opts.shots, "ui-relaunch-final.png") });
}
