import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { observeCollectionCallbacks } from "./collection-publish-window.mjs";
import { closeBrowserBounded, ensureNotCanceled, spawnSyncBounded } from "./native-scenario.mjs";

// AC03–AC05：Node 侧监听真实 IPC 网络事件，不写 Tauri 只读属性、不改原 Promise。
// started_ms / settled_ms 是 Node 时钟，不能与页面 callback 的 performance.now 相减。
export function observeNativeInvokes(page, now = Date.now) {
  if (typeof page?.on !== "function") throw new Error("native page events unavailable");
  const records = [];
  const pending = new Map();
  const copy = (value) => (value === undefined ? null : JSON.parse(JSON.stringify(value)));
  const onRequest = (request) => {
    const url = new URL(request.url());
    if (url.hostname !== "ipc.localhost" && url.protocol !== "ipc:") return;
    const command = decodeURIComponent(url.pathname.replace(/^\//, ""));
    const record = {
      command,
      args: null,
      status: "pending",
      started_ms: now(),
      settled_ms: null,
      value: null,
      error: null,
    };
    records.push(record);
    pending.set(request, record);
    try {
      record.args = copy(request.postDataJSON());
    } catch (error) {
      record.status = "probe-error";
      record.error = `IPC 请求体不可解析：${String(error?.message ?? error)}`;
    }
  };
  const onResponse = async (response) => {
    const record = pending.get(response.request());
    if (!record || record.status === "probe-error") return;
    try {
      record.value = copy(await response.json());
      record.http_status = response.status();
      const rejected = response.headers()["tauri-response"] === "error" || !response.ok();
      record.status = rejected ? "rejected" : "fulfilled";
      if (rejected)
        record.error =
          typeof record.value === "string" ? record.value : JSON.stringify(record.value);
    } catch (error) {
      record.status = "probe-error";
      record.error = `IPC 响应不可读取：${String(error?.message ?? error)}`;
    } finally {
      record.settled_ms = now();
      pending.delete(response.request());
    }
  };
  const onFailed = (request) => {
    const record = pending.get(request);
    if (!record) return;
    record.status = "rejected";
    record.error = request.failure()?.errorText ?? "IPC request failed";
    record.settled_ms = now();
    pending.delete(request);
  };
  page.on("request", onRequest);
  page.on("response", onResponse);
  page.on("requestfailed", onFailed);
  return {
    read: () => copy(records),
    dispose() {
      page.off("request", onRequest);
      page.off("response", onResponse);
      page.off("requestfailed", onFailed);
    },
  };
}

export function correlateNativeQuery({ summaryQuery, eventsQuery, records = [] }) {
  const latest = (command, id) =>
    records.findLast(
      (record) =>
        record.command === command &&
        record.status === "fulfilled" &&
        record.args?.queryId === id &&
        record.value?.query_id === id,
    );
  const summaryCall = latest("query_summary", summaryQuery);
  const eventCall = latest("query_events", eventsQuery);
  const beginCall = records.findLast(record => record.command === "query_begin"
    && record.status === "fulfilled" && record.value?.queryId === summaryQuery) ?? null;
  return {
    summaryQuery,
    eventsQuery,
    summary: summaryCall?.value ?? null,
    events: eventCall?.value ?? null,
    eventArgs: eventCall?.args ?? null,
    summaryCall: summaryCall ?? null,
    eventCall: eventCall ?? null,
    beginCall,
    sameQuery: Boolean(summaryQuery && summaryQuery === eventsQuery && summaryCall && eventCall),
  };
}

export function nextQueryDimension(selectedLabel) {
  if (!["按时间", "按模型", "按项目", "按应用"].includes(selectedLabel)) {
    throw new Error(`无法识别页面当前聚合维度：${selectedLabel}`);
  }
  return selectedLabel === "按时间" ? "project" : "day";
}

export function matchEventDom(events, domEvents) {
  if (
    !Array.isArray(events?.rows) ||
    !Array.isArray(domEvents?.rows) ||
    events.rows.length !== domEvents.rows.length ||
    events.total !== domEvents.total ||
    events.rows.length !== domEvents.loaded
  )
    return false;
  const label = { "claude-code": "Claude Code", codex: "Codex" };
  const fmt = (value) => Number(value).toLocaleString("en-US");
  return events.rows.every((row, index) => {
    const expected = [
      row.ts,
      label[row.agent] ?? row.agent,
      row.model,
      row.project.split(/[\\/]/).filter(Boolean).at(-1) ?? row.project,
      fmt(row.input + row.cache_write + row.cache_read),
      fmt(row.output),
      fmt(row.cache_write),
      fmt(row.cache_read),
    ];
    return expected.every((text, column) => domEvents.rows[index][column] === text);
  });
}

export function createUnifiedContext({
  inst,
  ev,
  markStage = () => {},
  markStageDone = () => {},
  isCanceled,
}) {
  let invokeProbe = null;
  const page = () => inst.page;
  const root = () => page().locator("[data-summary-query]");
  const click = async (locator) => {
    ensureNotCanceled(isCanceled, "unified-ui-click");
    await locator.scrollIntoViewIfNeeded();
    await locator.evaluate((element) => element.scrollIntoView({ block: "center" }));
    await locator.click();
  };
  const nav = (label) =>
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
  const select = async (locator, label) => {
    await click(typeof locator === "string" ? page().locator(locator) : locator);
    await click(page().locator(".n-base-select-option").getByText(label, { exact: true }));
  };
  const waitUntil = async (predicate, timeout = 20000, reason = "原生界面条件") => {
    const deadline = Date.now() + timeout;
    while (!(await predicate())) {
      ensureNotCanceled(isCanceled, reason);
      if (Date.now() >= deadline) throw new Error(`${reason}：${timeout} ms 内未满足`);
      if (inst.proc.exitCode !== null) throw new Error(`应用退出：${inst.proc.exitCode}`);
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  };
  const installProbe = async () => {
    invokeProbe?.dispose();
    invokeProbe = observeNativeInvokes(page());
    await page().evaluate(`(() => {
    window.__unifiedCallbacks?.dispose();
    window.__unifiedCallbacks = (${observeCollectionCallbacks.toString()})(window.__TAURI_INTERNALS__, () => performance.now());
  })()`);
  };
  const readProbe = async () => ({
    invokes: invokeProbe?.read() ?? [],
    callbacks: await page().evaluate(() => window.__unifiedCallbacks?.read() ?? []),
  });
  const last = async (command) =>
    (await readProbe()).invokes.findLast((record) => record.command === command) ?? null;
  const querySnapshot = async () => {
    const [dom, trace] = await Promise.all([
      page().evaluate(() => {
        const summary = document.querySelector("[data-summary-query]");
        const card = document.querySelector(".events-card");
        const text = card?.querySelector(".table-hint")?.textContent ?? "";
        const counts = text.match(/共\s*([\d,]+)\s*条\s*·\s*已加载\s*([\d,]+)\s*条/);
        const domEvents = {
          total: counts ? Number(counts[1].replaceAll(",", "")) : null,
          loaded: counts ? Number(counts[2].replaceAll(",", "")) : null,
          rows: [...(card?.querySelectorAll("tbody tr") ?? [])]
            .map((row) =>
              [...row.querySelectorAll("td")].slice(0, 8).map((cell) => cell.textContent.trim()),
            )
            .filter((cells) => cells.length >= 8),
        };
        return {
          summaryQuery: summary?.getAttribute("data-summary-query"),
          eventsQuery: summary?.getAttribute("data-events-query"),
          domEvents,
        };
      }),
      readProbe(),
    ]);
    const snapshot = correlateNativeQuery({ ...dom, records: trace.invokes });
    return {
      ...snapshot,
      domEvents: dom.domEvents,
      domMatches: matchEventDom(snapshot.events, dom.domEvents),
    };
  };
  const saveProbe = async (label = "final") => {
    const trace = await readProbe();
    ev.unified_probe ??= {};
    ev.unified_probe[label] = trace;
    return trace;
  };
  const cleanup = async () => {
    invokeProbe?.dispose();
    invokeProbe = null;
    await page().evaluate(() => {
      window.__unifiedCallbacks?.dispose();
      delete window.__unifiedCallbacks;
    });
  };
  const closeForRestart = async () => {
    const action = fileURLToPath(
      new URL("../../../scripts/native-window-action.ps1", import.meta.url),
    );
    ensureNotCanceled(isCanceled, "unified-close-command");
    markStage("unified-close-command");
    const result = spawnSyncBounded(
      "pwsh",
      ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", "close"],
      { action: "window-close", stage: "unified-close-command" },
    );
    markStageDone("unified-close-command");
    if (result.status !== 0 || !/CLOSED true/i.test(result.stdout)) {
      throw new Error(`WM_CLOSE failed: ${result.stderr} ${result.stdout}`);
    }
    markStage("unified-click-quit");
    await click(
      page().locator(".close-dialog").getByRole("button", { name: "直接退出", exact: true }),
    );
    markStageDone("unified-click-quit");
    markStage("unified-wait-host-exit");
    const deadline = Date.now() + 15000;
    while (inst.proc.exitCode === null && Date.now() < deadline) {
      ensureNotCanceled(isCanceled, "unified-wait-host-exit");
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    markStageDone("unified-wait-host-exit");
    if (inst.proc.exitCode !== 0)
      throw new Error(`restart requires clean exit, got ${inst.proc.exitCode}`);
    await closeBrowserBounded(inst.browser);
  };
  const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
  const savedSetting = (key, value) =>
    fs
      .readFileSync(settingsPath, "utf8")
      .split(/\r?\n/)
      .some((line) => line.trim() === `${key} = ${value}`);
  return {
    page,
    root,
    click,
    nav,
    section,
    select,
    waitUntil,
    installProbe,
    readProbe,
    last,
    querySnapshot,
    saveProbe,
    cleanup,
    closeForRestart,
    settingsPath,
    savedSetting,
  };
}
