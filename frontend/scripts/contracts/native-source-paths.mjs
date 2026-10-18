// PR3（计划 §9.6 第 2 条）：原生主线场景——从设置 UI 添加**自动候选之外**的手动
// 目录 → 列表追加且默认来源仍在 → 写入一条新的 Claude 原生请求 → 真实下一轮
// 采集后统计 +1 且汇总/明细同一查询 → 正常退出/重启 → 追加项仍在且同一请求
// 不再次计数。全程不点击刷新：自动接线由真实采集循环与 usage-updated 链路完成。
// S00 生命周期约束同样适用：关闭/退出/等待各打阶段标记，同步命令 30 秒上限。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { closeBrowserBounded, ensureNotCanceled, spawnSyncBounded } from "../lib/native-scenario.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export async function nativeSourcePaths({ inst, ck, ev, markStage, markStageDone, isCanceled }) {
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
      await sleep(100);
    }
  };
  const savedInterval = (secs) =>
    new RegExp(`^collect_interval_secs\\s*=\\s*${secs}\\s*$`, "m").test(
      fs.readFileSync(settingsPath, "utf8"),
    );
  const rowPaths = () =>
    page()
      .locator(".source-row .effective-path")
      .allInnerTexts()
      .then((list) => list.map((t) => t.trim().toLowerCase()));
  const hasRow = async (p) => (await rowPaths()).includes(p.toLowerCase());
  // 手动目录：隔离根下与 sources/ 平级的独立子树——不在任何默认根、迁移追加项
  // 或环境派生根之内（自动候选之外）。
  const manualDir = path.join(inst.root, "sources-extra", "claude-manual");
  const claudeDefault = path.join(inst.root, "sources", "claude", "projects");
  const codexDefault = path.join(inst.root, "sources", "codex", "sessions");
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
    while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(100);
    markStageDone("wait-host-exit");
    if (inst.proc.exitCode !== 0)
      throw new Error(`restart requires clean exit, got ${inst.proc.exitCode}`);
    await closeBrowserBounded(inst.browser);
  };

  await inst.waitForData();
  await inst.setWindowSize(980, 620);
  // 先把读取间隔降到 5 秒（设置保存路径，与 ui-refresh 相同；不算手动刷新），
  // 让后续"真实下一轮"的等待有确定上界。
  await navigate("设置");
  await section("数据源");
  await interval("5 秒");
  await waitUntil(
    async () => savedInterval(5) && (await page().locator(".interval-row .tz-state").count()) === 0,
  );

  // 迁移基线：v1 dir 随启动迁移为追加项，默认根行必须仍在列表里。
  await waitUntil(async () => (await hasRow(claudeDefault)) && (await hasRow(codexDefault)), 30000);
  ck.add(
    "default_rows_still_listed",
    true,
    `claude=${claudeDefault}; codex=${codexDefault}`,
  );

  // 1) 从 UI 追加手动目录：目录先真实创建（与用户先建目录一致），输入 + 添加
  //    是明确动作；列表重读后新行可见。
  fs.mkdirSync(manualDir, { recursive: true });
  await page().locator("#source-add-claude").fill(manualDir);
  await click(page().locator('[aria-label="Claude Code 添加目录"]'));
  await waitUntil(async () => hasRow(manualDir), 30000);
  ev.manual_source_path = manualDir;
  ck.add("manual_row_added_via_ui", true, manualDir);
  ck.add(
    "default_rows_unchanged_after_add",
    (await hasRow(claudeDefault)) && (await hasRow(codexDefault)),
    "追加后默认来源仍在列表",
  );

  // 2) 汇总基线 → 写入一条新的 Claude 原生请求到手动目录。
  await navigate("汇总");
  const before = Number(await root().getAttribute("data-requests"));
  ev.requests_before = before;
  const logDir = path.join(manualDir, "ui-source-paths");
  fs.mkdirSync(logDir, { recursive: true });
  const event = {
    type: "assistant",
    timestamp: new Date().toISOString(),
    sessionId: "ui-source-paths-e2e",
    cwd: "C:/acceptance/workspace/ui-source-paths",
    message: {
      id: "ui-source-paths-1",
      model: "claude-sonnet-4-5",
      usage: {
        input_tokens: 210,
        output_tokens: 13,
        cache_creation_input_tokens: 11,
        cache_read_input_tokens: 7,
      },
    },
  };
  const writtenAt = Date.now();
  fs.writeFileSync(path.join(logDir, "synthetic.jsonl"), JSON.stringify(event) + "\n", "utf8");

  // 3) 真实下一轮自动读取：统计 +1（不点击刷新），汇总/明细同一查询。
  await waitUntil(
    async () => Number(await root().getAttribute("data-requests")) === before + 1,
    90000,
  );
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
  await page().screenshot({ path: path.join(inst.opts.shots, "ui-source-paths-added.png") });

  // 4) 正常退出并重启（真实 WM_CLOSE → 点击「直接退出」→ 退出码 0）。
  await closeForRestart();
  await inst.launch();
  await inst.waitForData();

  // 5) 追加项仍在：重启后列表里手动行与默认行都在。
  await navigate("设置");
  await section("数据源");
  await waitUntil(
    async () => (await hasRow(manualDir)) && (await hasRow(claudeDefault)),
    30000,
  );
  ck.add("manual_row_survives_restart", true, manualDir);
  ck.add(
    "default_rows_unchanged_after_restart",
    (await hasRow(claudeDefault)) && (await hasRow(codexDefault)),
    "重启后默认来源仍在列表",
  );

  // 6) 同一请求不再次计数：等过至少两轮采集窗口（5 秒间隔）再断言计数不变。
  await navigate("汇总");
  await waitUntil(async () => Number(await root().getAttribute("data-requests")) === before + 1, 30000);
  const stabilityStartedAt = Date.now();
  while (Date.now() - stabilityStartedAt < 15000) {
    await sleep(1000);
    if (inst.proc.exitCode !== null) throw new Error("application exited during stability check");
    if (Math.floor((Date.now() - stabilityStartedAt) / 1000) % 5 === 0)
      process.stderr.write("[ui-source-paths] restart stability observation\n");
  }
  const after = Number(await root().getAttribute("data-requests"));
  ck.add(
    "history_not_double_counted_after_restart",
    after === before + 1,
    `requests=${after} expected=${before + 1}`,
  );
  ck.add(
    "same_query_visible_after_restart",
    (await root().getAttribute("data-summary-query")) ===
      (await root().getAttribute("data-events-query")),
    await root().getAttribute("data-summary-query"),
  );
  await page().screenshot({ path: path.join(inst.opts.shots, "ui-source-paths-restarted.png") });
}
