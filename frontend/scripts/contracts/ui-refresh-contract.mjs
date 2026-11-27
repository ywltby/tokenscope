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
    // PR3：来源开关即改即存（不再有「保存」按钮）——切换总开关触发保存并
    // 唤醒采集，通知链与旧「保存」路径同源。
    const beforeSave = calls();
    await click(page.locator('[aria-label="Claude Code 启用"]'));
    // 开关即改即存不弹通知（避免噪音）——等 source_config_set 真实发出。
    await page.waitForFunction(
      () => window.__TS_IPC_CALLS.some((c) => c.cmd === "source_config_set"),
      undefined,
      { timeout: 10000 },
    );
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

  await verifyScrollPreservationOnRefresh(ctx, name);
}

/**
 * 真实浏览器回归：同筛选后台自动刷新时，聚合表与请求明细都不回顶部，
 * 且上方新增行时尽量将正在看的那一行留在原视口位置；仅主动切换维度、
 * 时间范围或下钻筛选时，才让受影响的相应列表回到顶部。
 */
async function verifyScrollPreservationOnRefresh(ctx, name) {
  const s = await ctx.open({
    fixture: "model-rows-40",
    theme: "light",
    viewport: { width: 1280, height: 820, label: "1280x820" },
  });
  const { page } = s;
  const original = s.fixture.ipc;
  let extraEvents = 0;
  let extraGroups = 0;
  let extraRollups = 0;
  let baseEvents = null;
  let baseRollups = null;
  let lastCursorsByRowId = new Map();

  function makeSynthEvent(idx, prefix = "req", eventId = idx) {
    const min = String(idx % 60).padStart(2, "0");
    const hour = String(10 + Math.floor(idx / 60)).padStart(2, "0");
    const ts = `2026-10-07 ${hour}:${min}:00`;
    const rid = `${prefix}-${String(idx).padStart(3, "0")}`;
    return {
      ts,
      record_id: rid,
      row_id: `event:${eventId}`,
      agent: "claude-code",
      model: `model-${String(idx % 40).padStart(2, "0")}`,
      session_id: `sess-${idx}`,
      project: "/workspace/tokenscope",
      input: 1200 + idx * 10,
      output: 400 + idx * 5,
      cache_write: 100,
      cache_read: 50,
      cost_usd: 0.012,
      cost_breakdown: null,
    };
  }

  function makeSynthRollup(dayNum, modelKey = "claudeopus47") {
    const day = `2026-09-${String(dayNum).padStart(2, "0")}`;
    const sourceTz = "Asia/Shanghai";
    const agent = "claude-code";
    return {
      ts: day,
      record_id: "",
      row_id: `rollup:${JSON.stringify([day, modelKey, sourceTz, agent])}`,
      agent,
      model: modelKey,
      session_id: "",
      project: "",
      input: 50000 + dayNum * 100,
      output: 12000 + dayNum * 50,
      cache_write: 0,
      cache_read: 0,
      cost_usd: 1.5,
      cost_breakdown: null,
      rollup: {
        day,
        source_tz: sourceTz,
        source: "ccs",
        requests: 10 + dayNum,
        label: "Claude Opus 4.7",
        models: ["claude-opus-4-7"],
        cost_usd: 1.5,
        total_cost_usd: 1.5,
        source_cost_usd: null,
        source_cost_state: "absent",
        cost_detail: "rollup_only",
        unknown_pricing: false,
        unknown_reason: null,
        unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
        cost_partially_unknown: false,
        has_estimated_cost: true,
        cache_write_split_estimated: false,
        cache_price_zero_estimated: false,
      },
    };
  }

  s.fixture.ipc = async (cmd, args) => {
    const result = await original(cmd, args);
    if (cmd === "query_summary" && result && Array.isArray(result.groups)) {
      const cloned = result.groups.map((g) => ({ ...g, tokens: { ...g.tokens } }));
      if (extraGroups > 0) {
        const prefix =
          result.by === "model"
            ? "NewTopModel"
            : result.by === "project"
              ? "NewTopProject"
              : "2026-10-9";
        const prepended = Array.from({ length: extraGroups }, (_, i) => ({
          ...cloned[0],
          key: `prepended-${result.by}-${i}`,
          label: `${prefix} ${i}`,
          requests: 99 - i,
        }));
        return { ...result, groups: [...prepended, ...cloned] };
      }
      return { ...result, groups: cloned };
    }
    if (cmd === "query_events" && result && Array.isArray(result.rows)) {
      if (!baseEvents) {
        baseEvents = Array.from({ length: 45 }, (_, i) => makeSynthEvent(45 - i, "base", 1000 + i));
      }
      if (!baseRollups) {
        baseRollups = Array.from({ length: 15 }, (_, i) => makeSynthRollup(20 - i));
      }
      const prependedEvents = Array.from({ length: extraEvents }, (_, i) =>
        makeSynthEvent(100 + extraEvents - i, "fresh", 2000 + extraEvents - i),
      );
      const prependedRollups = Array.from({ length: extraRollups }, (_, i) =>
        makeSynthRollup(20 + extraRollups - i),
      );
      const qid = result.query_id;
      const mfp = `mfp-${qid}`;
      const dfp = JSON.stringify([args?.model ?? null, args?.project ?? null, args?.day ?? null]);
      const rawAll = [...prependedEvents, ...baseEvents, ...prependedRollups, ...baseRollups];
      const filtered = args?.model
        ? rawAll.filter((_, i) => i % 2 === 0)
        : args?.day
          ? rawAll.filter((_, i) => i % 2 === 1)
          : rawAll;
      let ridx = 0;
      const cursors = new Map();
      const rowsWithSessionCursor = filtered.map((r, i) => {
        // 真实后端 PageCursor v2 格式：每次刷新随 query_id（及日汇总 ridx）变化。
        const cursor = r.rollup
          ? JSON.stringify({
              v: 2,
              qid,
              mfp,
              dfp,
              ts: r.ts,
              rid: "",
              seq: 0,
              kind: "rollup",
              ridx: ridx++,
            })
          : JSON.stringify({
              v: 2,
              qid,
              mfp,
              dfp,
              ts: `${r.ts.replace(" ", "T")}.000Z`,
              rid: r.record_id,
              seq: i,
            });
        cursors.set(r.row_id, cursor);
        return { ...r, cursor };
      });
      lastCursorsByRowId = cursors;
      return {
        ...result,
        rows: rowsWithSessionCursor,
        total: rowsWithSessionCursor.length,
      };
    }
    return result;
  };

  const row = page.locator("[data-summary-query]");
  const usageSelector = ".usage-card .scroll-list";
  const eventsSelector = ".events-card .scroll-list";
  const waitNewQuery = async (prevQuery) => {
    await page.waitForFunction(
      (old) => document.querySelector("[data-summary-query]")?.dataset.summaryQuery !== old,
      prevQuery,
    );
    await page.waitForTimeout(80);
  };
  const triggerUsageUpdated = async (generation) =>
    page.evaluate((gen) => {
      const registration = window.__TS_IPC_CALLS.find(
        (c) => c.cmd === "plugin:event|listen" && c.args.event === "usage-updated",
      );
      if (!registration) throw new Error("usage-updated listener missing");
      window.__TS_CB[registration.args.handler]({ payload: { generation: gen } });
    }, generation);
  const measureAnchor = async (containerSelector, targetText = null) =>
    page.evaluate(
      ({ sel, target }) => {
        const el = document.querySelector(sel);
        if (!el) return null;
        const vp = el.getBoundingClientRect();
        const trs = [...el.querySelectorAll(".n-data-table-tbody > tr.n-data-table-tr")];
        const info = trs.map((tr) => {
          const r = tr.getBoundingClientRect();
          const firstCell = tr.querySelector("td")?.textContent?.trim() ?? "";
          return { text: firstCell, topOffset: r.top - vp.top, bottomOffset: r.bottom - vp.top };
        });
        const picked =
          target != null
            ? info.find((x) => x.text === target)
            : info.find((x) => x.bottomOffset > 1);
        return {
          scrollTop: el.scrollTop,
          scrollHeight: el.scrollHeight,
          clientHeight: el.clientHeight,
          anchorText: picked?.text ?? null,
          anchorTop: picked?.topOffset ?? null,
        };
      },
      { sel: containerSelector, target: targetText },
    );
  const setScroll = async (containerSelector, top) =>
    page.evaluate(
      ({ sel, value }) => {
        const el = document.querySelector(sel);
        if (!el) throw new Error(`missing ${sel}`);
        el.scrollTop = value;
        el.dispatchEvent(new Event("scroll"));
      },
      { sel: containerSelector, value: top },
    );

  try {
    await page.goto(s.url, { waitUntil: "networkidle" });
    await row.waitFor();
    await page.getByRole("radio", { name: "按模型", exact: true }).click();
    await page.waitForFunction(
      () =>
        document.querySelectorAll(".usage-card .n-data-table-tbody > tr.n-data-table-tr").length >=
        40,
    );
    await page.waitForTimeout(80);

    await setScroll(usageSelector, 450);
    await setScroll(eventsSelector, 515);
    const usageBefore = await measureAnchor(usageSelector);
    const eventsBefore = await measureAnchor(eventsSelector);
    const nativeAnchorRowId = baseEvents.find((e) => e.ts === eventsBefore.anchorText)?.row_id;
    const nativeCursorBefore = lastCursorsByRowId.get(nativeAnchorRowId);

    // 1) 同筛选后台自动刷新（内容相同，替换行对象且游标随 query_id 切换）：不归零，原行留在原位
    const q1 = await row.getAttribute("data-summary-query");
    await triggerUsageUpdated(50);
    await waitNewQuery(q1);
    const usageSame = await measureAnchor(usageSelector, usageBefore.anchorText);
    const eventsSame = await measureAnchor(eventsSelector, eventsBefore.anchorText);
    const nativeCursorAfterSame = lastCursorsByRowId.get(nativeAnchorRowId);
    ctx.record(
      name,
      "同筛选后台刷新（同内容新对象 + 会话游标轮换）保留双表滚动与锚点行",
      nativeCursorBefore != null &&
        nativeCursorAfterSame != null &&
        nativeCursorBefore !== nativeCursorAfterSame &&
        Math.abs(usageSame.scrollTop - 450) <= 2 &&
        Math.abs(eventsSame.scrollTop - 515) <= 2 &&
        Math.abs(usageSame.anchorTop - usageBefore.anchorTop) <= 1.5 &&
        Math.abs(eventsSame.anchorTop - eventsBefore.anchorTop) <= 1.5,
      JSON.stringify({
        usageBefore,
        usageSame,
        eventsBefore,
        eventsSame,
        cursorRotated: nativeCursorBefore !== nativeCursorAfterSame,
      }),
    );

    // 2) 同筛选后台自动刷新且上方新增行（聚合 +2 行、请求明细 +5 条 + 游标随新会话变化）：原行留在视野原位
    extraGroups = 2;
    extraEvents = 5;
    const q2 = await row.getAttribute("data-summary-query");
    await triggerUsageUpdated(51);
    await waitNewQuery(q2);
    const usagePrepended = await measureAnchor(usageSelector, usageBefore.anchorText);
    const eventsPrepended = await measureAnchor(eventsSelector, eventsBefore.anchorText);
    const nativeCursorAfterPrepend = lastCursorsByRowId.get(nativeAnchorRowId);
    ctx.record(
      name,
      "同筛选后台刷新（上方新增请求/分组 + 会话游标轮换）保持正在看的那一行原位置",
      nativeCursorAfterPrepend != null &&
        nativeCursorAfterPrepend !== nativeCursorAfterSame &&
        usagePrepended.scrollTop > usageBefore.scrollTop + 40 &&
        eventsPrepended.scrollTop > eventsBefore.scrollTop + 120 &&
        Math.abs(usagePrepended.anchorTop - usageBefore.anchorTop) <= 1.5 &&
        Math.abs(eventsPrepended.anchorTop - eventsBefore.anchorTop) <= 1.5,
      JSON.stringify({
        usageBefore,
        usagePrepended,
        eventsBefore,
        eventsPrepended,
        cursorRotated: nativeCursorAfterPrepend !== nativeCursorAfterSame,
      }),
    );

    // 2.5) 滚到 CCS 日汇总行区，再触发后台刷新（上方新增 3 条原生请求 + 1 条更晚日期 CCS 汇总，
    //      使游标中的 qid 与 ridx 同时变化）：正在看的 CCS 日汇总行同样留在原位置
    await setScroll(eventsSelector, 2120);
    const rollupBefore = await measureAnchor(eventsSelector);
    const rollupAnchorRow = baseRollups.find((r) => rollupBefore.anchorText?.startsWith(r.ts));
    const rollupCursorBefore = rollupAnchorRow
      ? lastCursorsByRowId.get(rollupAnchorRow.row_id)
      : null;
    extraEvents = 8;
    extraRollups = 1;
    const q2b = await row.getAttribute("data-summary-query");
    await triggerUsageUpdated(52);
    await waitNewQuery(q2b);
    const rollupAfter = await measureAnchor(eventsSelector, rollupBefore.anchorText);
    const rollupCursorAfter = rollupAnchorRow
      ? lastCursorsByRowId.get(rollupAnchorRow.row_id)
      : null;
    ctx.record(
      name,
      "同筛选后台刷新（上方新增请求与日汇总 + qid/ridx 游标轮换）保持 CCS 日汇总行原位置",
      rollupAnchorRow != null &&
        rollupCursorBefore != null &&
        rollupCursorAfter != null &&
        rollupCursorBefore !== rollupCursorAfter &&
        rollupAfter.scrollTop > rollupBefore.scrollTop + 100 &&
        Math.abs(rollupAfter.anchorTop - rollupBefore.anchorTop) <= 1.5,
      JSON.stringify({
        rollupBefore,
        rollupAfter,
        rollupCursorBefore,
        rollupCursorAfter,
      }),
    );

    // 3) 点击聚合行触发下钻：仅请求明细回顶部，聚合表保持原位
    const usageBeforeDrill = await measureAnchor(usageSelector);
    const firstVisibleUsageRow = page
      .locator(".usage-card .n-data-table-tbody > tr.n-data-table-tr")
      .nth(14);
    await firstVisibleUsageRow.click();
    await page.locator(".events-card .filter-chip").waitFor();
    await page.waitForTimeout(100);
    const usageAfterDrill = await measureAnchor(usageSelector);
    const eventsAfterDrill = await measureAnchor(eventsSelector);
    ctx.record(
      name,
      "点击下钻仅重置请求明细滚动位置，聚合表保持原位",
      eventsAfterDrill.scrollTop === 0 &&
        Math.abs(usageAfterDrill.scrollTop - usageBeforeDrill.scrollTop) <= 2,
      JSON.stringify({ usageBeforeDrill, usageAfterDrill, eventsAfterDrill }),
    );

    // 4) 清除下钻：再次仅重置请求明细，聚合表不动
    await setScroll(eventsSelector, 240);
    await page.locator(".events-card .filter-chip").click();
    await page.waitForFunction(() => !document.querySelector(".events-card .filter-chip"));
    await page.waitForTimeout(100);
    const usageAfterClear = await measureAnchor(usageSelector);
    const eventsAfterClear = await measureAnchor(eventsSelector);
    ctx.record(
      name,
      "清除下钻仅重置请求明细滚动位置，聚合表保持原位",
      eventsAfterClear.scrollTop === 0 &&
        Math.abs(usageAfterClear.scrollTop - usageBeforeDrill.scrollTop) <= 2,
      JSON.stringify({ usageAfterClear, eventsAfterClear }),
    );

    // 5) 切换维度（按模型 → 按项目）：聚合表回顶部，请求明细保持原位
    await setScroll(eventsSelector, 300);
    const q3 = await row.getAttribute("data-summary-query");
    await page.getByRole("radio", { name: "按项目", exact: true }).click();
    await waitNewQuery(q3);
    const usageAfterDim = await measureAnchor(usageSelector);
    const eventsAfterDim = await measureAnchor(eventsSelector);
    ctx.record(
      name,
      "切换维度时聚合表回顶部，请求明细保持原位",
      usageAfterDim.scrollTop === 0 && Math.abs(eventsAfterDim.scrollTop - 300) <= 2,
      JSON.stringify({ usageAfterDim, eventsAfterDim }),
    );

    // 6) 切换时间范围（全部时间 → 7d）：双表同时回顶部
    await setScroll(usageSelector, 450);
    await setScroll(eventsSelector, 300);
    const q4 = await row.getAttribute("data-summary-query");
    await page.locator(".range-trigger").click();
    await page.getByRole("button", { name: "7d", exact: true }).click();
    await waitNewQuery(q4);
    const usageAfterRange = await measureAnchor(usageSelector);
    const eventsAfterRange = await measureAnchor(eventsSelector);
    ctx.record(
      name,
      "切换时间范围时聚合表与请求明细均回顶部",
      usageAfterRange.scrollTop === 0 && eventsAfterRange.scrollTop === 0,
      JSON.stringify({ usageAfterRange, eventsAfterRange }),
    );

    ctx.finishSession(s);
  } finally {
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
