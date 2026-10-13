// 真实桌面窗口恢复验收；不调用系统休眠、不改系统时钟或显示缩放。
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export async function nativeUiSystem({ inst, ck, ev }) {
  const page = () => inst.page;
  const root = () => page().locator("[data-summary-query]");
  const query = () => root().getAttribute("data-summary-query");
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
    await page().locator(".range-trigger").click();
    await page().getByRole("button", { name: label, exact: true }).click();
    await waitQuery(old);
  };
  const restore = async (checkHidden = false) => {
    const action = fileURLToPath(
      new URL("../../../scripts/native-window-action.ps1", import.meta.url),
    );
    const change = (state) => {
      const result = spawnSync(
        "pwsh",
        ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", state],
        { encoding: "utf8", windowsHide: true },
      );
      if (result.status !== 0 || !/STATE_CHANGED true/i.test(result.stdout))
        throw new Error(`Native ${state} failed: ${result.stdout} ${result.stderr}`);
    };
    change("minimize");
    ev.minimizedVisibility = await page().evaluate(() => document.visibilityState);
    await new Promise((resolve) => setTimeout(resolve, 1200));
    if (checkHidden) {
      const hiddenQuery = await query();
      await page().evaluate(async () => {
        for (let n = 0; n < 3; n++) {
          await window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
            event: "usage-updated",
            payload: { generation: 0 },
          });
        }
      });
      await new Promise((resolve) => setTimeout(resolve, 1500));
      ck.add(
        "native_minimized_notifications_do_not_query",
        (await query()) === hiddenQuery,
        `${hiddenQuery} -> ${await query()}; synthetic notifications via real event bus`,
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
  ck.add(
    "real_query_timestamp_displayed",
    await page()
      .getByText(/^更新于 \d{2}:\d{2}:\d{2}$/)
      .isVisible(),
    "actual Rust timestamp rendered",
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
  const allQuery = await query();
  await restore();
  await new Promise((resolve) => setTimeout(resolve, 1500));
  ck.add("all_time_restore_does_not_query", (await query()) === allQuery, await query());
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
  ck.add(
    "page_switch_preserves_scroll_and_query",
    scrollBefore > 0 && (await query()) === allQuery,
    `scroll=${scrollBefore}; query=${await query()}`,
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
  const close = spawnSync(
    "pwsh",
    ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", "close"],
    { encoding: "utf8", windowsHide: true },
  );
  if (close.status !== 0 || !/CLOSED true/i.test(close.stdout)) throw new Error("WM_CLOSE failed");
  await page()
    .locator(".close-dialog")
    .getByRole("button", { name: "直接退出", exact: true })
    .click();
  const deadline = Date.now() + 15000;
  while (inst.proc.exitCode === null && Date.now() < deadline)
    await new Promise((resolve) => setTimeout(resolve, 100));
  if (inst.proc.exitCode !== 0)
    throw new Error(`expected clean shutdown, got ${inst.proc.exitCode}`);
  try {
    await inst.browser.close();
  } catch {}
  await inst.launch();
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
  const trayClose = spawnSync(
    "pwsh",
    ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", "close"],
    { encoding: "utf8", windowsHide: true },
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
  await page().evaluate(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__trayQueryBegins = 0;
    window.__TAURI_INTERNALS__.invoke = function (cmd, ...args) {
      if (cmd === "query_begin") window.__trayQueryBegins++;
      return original.call(this, cmd, ...args);
    };
  });
  await page().evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
      event: "usage-updated",
      payload: { generation: 0 },
    }),
  );
  await new Promise((resolve) => setTimeout(resolve, 1500));
  ck.add(
    "tray_notifications_do_not_query",
    (await page().evaluate(() => window.__trayQueryBegins)) === 0,
    `${trayQuery} -> ${await query()}; new queries=${await page().evaluate(() => window.__trayQueryBegins)}`,
  );
  ev.scope =
    "real minimize/restore and focus; not OS sleep; explicit --scale is layout emulation only";
}
