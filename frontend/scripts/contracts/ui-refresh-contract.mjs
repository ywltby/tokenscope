// 浏览器端到端：真实 Vue 页面、受控 IPC 时序；不替代原生后端验收。
export async function uiRefreshLifecycle(ctx) {
  const name = "ui_refresh_lifecycle";
  const s = await ctx.open({
    fixture: "normal",
    theme: "light",
    viewport: { width: 1280, height: 820, label: "1280x820" },
  });
  const { page } = s;
  const calls = () => s.ipcCalls.filter((c) => c.cmd === "query_begin").length;
  const row = page.locator("[data-summary-query]");
  const original = s.fixture.ipc;
  let hold = false;
  let release;
  s.fixture.ipc = async (cmd, args) => {
    const result = await original(cmd, args);
    if (hold && cmd === "query_events") {
      hold = false;
      return new Promise((resolve) => {
        release = () => resolve(result);
      });
    }
    return result;
  };
  const click = async (locator) => {
    await locator.scrollIntoViewIfNeeded();
    await locator.evaluate((el) => el.scrollIntoView({ block: "center" }));
    await locator.click();
  };
  const navigate = (label) =>
    click(page.locator('[aria-label="页面切换"]').getByRole("radio", { name: label, exact: true }));
  const notify = async () =>
    page.evaluate(() => {
      const registration = window.__TS_IPC_CALLS.find(
        (c) => c.cmd === "plugin:event|listen" && c.args.event === "usage-updated",
      );
      if (!registration) throw new Error("usage-updated listener missing");
      window.__TS_CB[registration.args.handler]({ payload: { generation: 99 } });
    });
  try {
    await page.goto(s.url);
    await row.waitFor();
    const initial = await row.getAttribute("data-summary-query");
    hold = true;
    await click(page.getByRole("button", { name: "刷新", exact: true }));
    for (let n = 0; !release && n < 100; n++) await page.waitForTimeout(20);
    if (!release) throw new Error("new first page never requested");
    ctx.record(
      name,
      "新汇总先返回时继续显示完整旧批次",
      (await row.getAttribute("data-summary-query")) === initial &&
        (await row.getAttribute("data-events-query")) === initial,
      initial,
    );
    release();
    await page.waitForFunction(
      (old) => document.querySelector("[data-summary-query]")?.dataset.summaryQuery !== old,
      initial,
    );
    ctx.record(
      name,
      "两侧齐备后整体提交",
      (await row.getAttribute("data-summary-query")) ===
        (await row.getAttribute("data-events-query")),
      await row.getAttribute("data-summary-query"),
    );
    const list = page.getByRole("region", { name: "请求明细列表" });
    const eventCalls = s.ipcCalls.filter((c) => c.cmd === "query_events").length;
    await list.focus();
    await list.press("End");
    await page.waitForFunction(
      (n) => window.__TS_IPC_CALLS.filter((c) => c.cmd === "query_events").length > n,
      eventCalls,
    );
    await page.waitForFunction(
      () => !document.querySelector(".scroll-list-status")?.textContent.includes("正在加载"),
    );
    const paged = await row.getAttribute("data-summary-query");
    const before = calls();
    await navigate("设置");
    await notify();
    await navigate("汇总");
    await page.getByRole("button", { name: "有新数据 · 刷新" }).waitFor();
    ctx.record(
      name,
      "隐藏后返回仍保护续页",
      calls() === before && (await row.getAttribute("data-summary-query")) === paged,
      `begin=${calls()}/${before}`,
    );
    await click(page.getByRole("button", { name: "有新数据 · 刷新" }));
    await page.waitForFunction(
      (old) => document.querySelector("[data-summary-query]")?.dataset.summaryQuery !== old,
      paged,
    );
    ctx.record(name, "确认刷新重建一批", calls() === before + 1, calls());
    await navigate("设置");
    await click(page.getByRole("button", { name: "数据源", exact: true }));
    const beforeSave = calls();
    await click(
      page
        .locator('[data-section="sources"]')
        .getByRole("button", { name: "保存", exact: true })
        .first(),
    );
    await page
      .locator(".n-message")
      .filter({ hasText: /已保存.*来源配置/ })
      .first()
      .waitFor();
    await navigate("汇总");
    await page.waitForFunction(
      (n) => window.__TS_IPC_CALLS.filter((c) => c.cmd === "query_begin").length > n,
      beforeSave,
    );
    ctx.record(name, "来源保存通知保留的汇总页", calls() === beforeSave + 1, calls());
    await ctx.shot(name, s, "refreshed");
    ctx.finishSession(s);
  } finally {
    if (release) release();
    await s.close();
  }
}

export async function uiRefreshClock(ctx) {
  const name = "ui_refresh_clock";
  const s = await ctx.open({
    fixture: "normal",
    theme: "dark",
    viewport: { width: 980, height: 620, label: "980x620" },
  });
  const { page } = s;
  const calls = () => s.ipcCalls.filter((c) => c.cmd === "query_begin").length;
  const click = async (locator) => {
    await locator.scrollIntoViewIfNeeded();
    await locator.click();
  };
  const navigation = (label) =>
    page.locator('[aria-label="页面切换"] button').filter({ hasText: label });
  const selectRange = async (label) => {
    await click(page.locator(".range-trigger"));
    await click(page.getByRole("button", { name: label, exact: true }));
    // PR2：刷新完成的信号是静态「已更新」（取消常驻"更新于 HH:MM:SS"）。
    await page.waitForFunction(() =>
      document.querySelector(".ts-status-pill")?.textContent.includes("已更新"),
    );
  };
  try {
    await page.clock.install({ time: new Date("2026-10-07T15:59:30Z") });
    await page.goto(s.url);
    await page.locator("[data-summary-query]").waitFor();
    await selectRange("当日");
    const before = calls();
    await click(navigation("设置"));
    await page.clock.setSystemTime(new Date("2026-10-07T16:01:00Z"));
    await click(navigation("汇总"));
    await page.waitForFunction(
      (n) => window.__TS_IPC_CALLS.filter((c) => c.cmd === "query_begin").length > n,
      before,
    );
    ctx.record(name, "隐藏跨午夜激活立即刷新", calls() === before + 1, calls());
    await selectRange("24h");
    const rolling = calls();
    await page.clock.runFor(60000);
    await page.waitForFunction(
      (n) => window.__TS_IPC_CALLS.filter((c) => c.cmd === "query_begin").length > n,
      rolling,
    );
    ctx.record(name, "无新数据时滚动窗口仍刷新", calls() === rolling + 1, calls());
    ctx.record(
      name,
      "时钟刷新不重复采集",
      s.ipcCalls.filter((c) => c.cmd === "query_begin").at(-1).args.collect === false,
      s.ipcCalls.filter((c) => c.cmd === "query_begin").at(-1).args.trigger,
    );
    await selectRange("全部时间");
    const all = calls();
    await page.clock.runFor(60000);
    ctx.record(name, "全部时间不因时钟刷新", calls() === all, calls());
    await ctx.shot(name, s, "clock");
    ctx.finishSession(s);
  } finally {
    await s.close();
  }
}
