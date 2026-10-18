// 真 WebView2 + 真 Rust + 隔离根。只追加合成日志，不读写真实来源。
// S00（PR3 计划 §9.1 第 3、4 条）：重启段的关闭/点击退出/等待退出各打阶段标记；
// 同步命令 30 秒上限、CDP 关闭 10 秒上限。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { closeBrowserBounded, ensureNotCanceled, spawnSyncBounded } from "../lib/native-scenario.mjs";

export async function nativeUiRefresh({ inst, ck, ev, markStage, markStageDone, isCanceled }) {
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
  const waitUntil = async (predicate, timeout = 20000) => {
    const deadline = Date.now() + timeout;
    while (!(await predicate())) {
      if (Date.now() >= deadline) throw new Error("native UI condition timed out");
      if (inst.proc.exitCode !== null) throw new Error("native application exited");
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  };
  const savedInterval = (secs) =>
    new RegExp(`^collect_interval_secs\\s*=\\s*${secs}\\s*$`, "m").test(
      fs.readFileSync(settingsPath, "utf8"),
    );
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
  await inst.waitForData();
  await inst.setWindowSize(980, 620);
  ev.initialViewport = await page().evaluate(() => ({
    width: innerWidth,
    height: innerHeight,
    dpr: devicePixelRatio,
  }));
  const before = Number(await root().getAttribute("data-requests"));
  await navigate("设置");
  await section("数据源");
  await interval("5 秒");
  await waitUntil(
    async () => savedInterval(5) && (await page().locator(".interval-row .tz-state").count()) === 0,
  );
  await navigate("汇总");
  const logDir = path.join(inst.root, "sources", "claude", "projects", "ui-e2e");
  fs.mkdirSync(logDir, { recursive: true });
  const event = {
    type: "assistant",
    timestamp: new Date().toISOString(),
    sessionId: "ui-refresh-e2e",
    cwd: "C:/acceptance/workspace/ui-e2e",
    message: {
      id: "ui-refresh-1",
      model: "claude-sonnet-4-5",
      usage: {
        input_tokens: 310,
        output_tokens: 17,
        cache_creation_input_tokens: 19,
        cache_read_input_tokens: 23,
      },
    },
  };
  const writtenAt = Date.now();
  fs.writeFileSync(path.join(logDir, "synthetic.jsonl"), JSON.stringify(event) + "\n", "utf8");
  await waitUntil(async () => Number(await root().getAttribute("data-requests")) === before + 1);
  ev.autoVisibleLatencyMs = Date.now() - writtenAt;
  ck.add(
    "synthetic_usage_visible_without_manual_refresh",
    true,
    `requests ${before} -> ${before + 1}; ${ev.autoVisibleLatencyMs} ms`,
  );
  ck.add(
    "same_query_visible",
    (await root().getAttribute("data-summary-query")) ===
      (await root().getAttribute("data-events-query")),
    await root().getAttribute("data-summary-query"),
  );

  // 空闲采集验证保留真实 30 秒（自动读取最短间隔 5 秒，约 6 轮）；期间不发查询、
  // 不模拟时间、不绕过原生日志器。
  const logs = () => {
    const dir = path.join(inst.root, "tokenscope", "logs");
    return fs
      .readdirSync(dir)
      .sort()
      .flatMap((f) =>
        fs
          .readFileSync(path.join(dir, f), "utf8")
          .split(/\r?\n/)
          .filter((line) => /\bINFO\b/.test(line)),
      );
  };
  const baseline = logs();
  const started = Date.now();
  while (Date.now() - started < 30000) {
    await new Promise((resolve) => setTimeout(resolve, 1000));
    if (inst.proc.exitCode !== null) throw new Error("application exited during idle check");
    if (Math.floor((Date.now() - started) / 1000) % 10 === 0)
      process.stderr.write("[ui-refresh] idle log observation\n");
  }
  const after = logs();
  ck.add(
    "idle_30s_no_info_lines",
    JSON.stringify(after) === JSON.stringify(baseline),
    `${baseline.length} -> ${after.length}`,
  );
  ck.add(
    "repeated_collection_does_not_duplicate",
    Number(await root().getAttribute("data-requests")) === before + 1,
    await root().getAttribute("data-requests"),
  );

  await navigate("设置");
  await section("数据源");
  await interval("10 秒");
  await interval("30 秒");
  await interval("5 秒");
  await waitUntil(
    async () => savedInterval(5) && (await page().locator(".interval-row .tz-state").count()) === 0,
  );
  // 构造旧版偏好仅写隔离 WebView profile；停进程后移除隔离 TOML 的新字段。
  await page().evaluate(() => localStorage.setItem("tokenscope-tz", "UTC"));
  await closeForRestart();
  fs.writeFileSync(
    settingsPath,
    fs.readFileSync(settingsPath, "utf8").replace(/^timezone\s*=.*\r?\n/gm, ""),
    "utf8",
  );
  await inst.launch();
  await inst.waitForData();
  ck.add(
    "interval_survives_restart",
    savedInterval(5) &&
      (await page().locator(".ts-status-pill").getAttribute("title"))?.includes("5 秒"),
    fs.readFileSync(settingsPath, "utf8"),
  );
  ck.add(
    "history_survives_restart",
    Number(await root().getAttribute("data-requests")) === before + 1,
    await root().getAttribute("data-requests"),
  );
  const savedTimezone = () =>
    /^timezone\s*=\s*"UTC"\s*$/m.test(fs.readFileSync(settingsPath, "utf8"));
  ck.add(
    "legacy_timezone_migrated",
    savedTimezone() &&
      (await page().evaluate(() => localStorage.getItem("tokenscope-tz"))) === null,
    "isolated legacy UTC -> TOML; old key removed",
  );
  // 再放一个冲突旧键，新 TOML 必须胜出，验证迁移只发生一次。
  await page().evaluate(() => localStorage.setItem("tokenscope-tz", "Asia/Shanghai"));
  await closeForRestart();
  await inst.launch();
  await inst.waitForData();
  ck.add(
    "stored_timezone_wins_on_second_restart",
    savedTimezone() &&
      (await page().evaluate(() => localStorage.getItem("tokenscope-tz"))) === null,
    "TOML UTC retained despite legacy Asia/Shanghai",
  );
  await page().screenshot({ path: path.join(inst.opts.shots, "ui-refresh-restarted.png") });
}
