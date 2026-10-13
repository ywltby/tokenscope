// U02/U07：真实键盘输入与完整视觉矩阵；IPC 使用合成事实，不访问用户目录。
export async function uiDateKeyboardMatrix(ctx) {
  const name = "ui_date_keyboard_matrix";
  for (const theme of ["light", "dark"]) {
    for (const [width, height] of [
      [1280, 820],
      [980, 620],
    ]) {
      const tag = `${theme}-${width}x${height}`;
      const s = await ctx.open({
        fixture: "normal",
        theme,
        viewport: { width, height, label: `${width}x${height}` },
      });
      const { page } = s;
      const original = s.fixture.ipc;
      let hold = false;
      let release;
      s.fixture.ipc = async (cmd, args) => {
        const result = await original(cmd, args);
        if (cmd === "query_summary" && result.range?.granularity === "instant") {
          result.rollup_coverage = {
            detail_buckets: 0,
            rollup_buckets: 0,
            unresolved_buckets: 0,
            unresolved_covered_tokens: {
              input: 0,
              output: 0,
              cache_write: 0,
              cache_read: 0,
            },
            rollups_excluded_for_instant_range: true,
          };
        }
        if (cmd === "query_events" && hold) {
          hold = false;
          return new Promise((resolve) => {
            release = () => resolve(result);
          });
        }
        return result;
      };
      const record = (label, pass, detail) => ctx.record(name, `${tag}: ${label}`, pass, detail);
      const shot = async (label) => {
        // Tab 穿过页面会改变滚动位置；截图前仅恢复拍摄位置，不改变焦点/业务值。
        await page.locator(".scroll-container").evaluate((el) => {
          el.scrollTop = 0;
        });
        await page.waitForTimeout(150);
        if (label.endsWith("-panel")) {
          const bounds = await page.locator(".range-panel").evaluate((el) => {
            const outer = el.closest(".n-popover").getBoundingClientRect();
            const buttons = [...el.querySelectorAll(".actions-row button")].map((b) =>
              b.getBoundingClientRect(),
            );
            return {
              top: outer.top,
              bottom: outer.bottom,
              height: innerHeight,
              visible:
                outer.top >= 0 &&
                outer.bottom <= innerHeight &&
                buttons.every((b) => b.top >= 0 && b.bottom <= innerHeight),
            };
          });
          record(`${label} 弹层及操作按钮在视口内`, bounds.visible, JSON.stringify(bounds));
        }
        await ctx.shot(name, s, `${tag}-${label}`);
      };
      // 只靠 Tab 寻找焦点，禁止 locator.focus/fill 或脚本修改组件状态。
      const tabTo = async (locator) => {
        for (let n = 0; n < 160; n++) {
          if (await locator.evaluate((el) => el === document.activeElement)) return;
          await page.keyboard.press("Tab");
        }
        throw new Error(`${tag}: Tab 无法到达 ${await locator.evaluate((el) => el.outerHTML)}`);
      };
      const activate = async (locator, key = "Enter") => {
        await tabTo(locator);
        await page.keyboard.press(key);
      };
      const type = async (locator, value) => {
        // NTimePicker 的被动输入态先将焦点给 .n-input 外壳，Enter 才进入编辑。
        if ((await locator.getAttribute("tabindex")) === "-1") {
          await activate(
            locator.locator(
              "xpath=ancestor::*[contains(concat(' ', normalize-space(@class), ' '), ' n-input ')][1]",
            ),
          );
        }
        await tabTo(locator);
        await page.keyboard.press("ControlOrMeta+A");
        await page.keyboard.type(value);
        await page.keyboard.press("Tab");
      };
      const trigger = page.locator(".range-trigger");
      const row = page.locator("[data-summary-query]");
      const lastArgs = () => s.ipcCalls.filter((c) => c.cmd === "query_begin").at(-1)?.args;
      const changed = async (old) =>
        page.waitForFunction((id) => {
          const el = document.querySelector("[data-summary-query]");
          return (
            el?.dataset.summaryQuery !== id && el?.dataset.summaryQuery === el?.dataset.eventsQuery
          );
        }, old);
      const open = async () => {
        await activate(trigger);
        await page.locator(".range-panel").waitFor();
      };
      try {
        // 与合成后端冻结基准一致；避免键盘长流程碰上真实分钟 tick。
        await page.clock.install({ time: new Date("2026-10-07T14:00:00Z") });
        await page.goto(s.url);
        await row.waitFor();
        record("全部时间为图标", (await trigger.locator("svg").count()) === 1, "初始态");
        await shot("all-time");
        for (const label of ["当日", "24h", "7d"]) {
          const old = await row.getAttribute("data-summary-query");
          await open();
          await activate(page.getByRole("button", { name: label, exact: true }));
          await changed(old);
          const args = lastArgs();
          record(
            `${label} 键盘预设`,
            (await trigger.innerText()).trim() === label &&
              (label === "24h" ? args.rollingHours === 24 : args.days === (label === "7d" ? 7 : 1)),
            JSON.stringify(args),
          );
          if (label === "24h") {
            record(
              "时刻模式披露日汇总排除",
              (await page.locator(".ts-notice").count()) === 1 &&
                /不含只有日粒度的历史/.test(await page.locator(".ts-notice").innerText()),
              "合成后端排除标志",
            );
          }
          await shot(`preset-${label}`);
        }
        const year = 2026;
        const cases = [
          {
            id: "whole-day",
            start: `${year}-01-02`,
            end: `${year}-01-03`,
            a: "00:00",
            b: "23:59",
            label: "1/2–1/3",
          },
          {
            id: "minute",
            start: `${year}-01-02`,
            end: `${year}-01-02`,
            a: "08:15",
            b: "09:45",
            label: "1/2 08:15–09:45",
          },
          {
            id: "cross-year",
            start: `${year - 1}-12-31`,
            end: `${year}-01-02`,
            a: "00:00",
            b: "23:59",
            label: `${year - 1}/12/31–${year}/1/2`,
          },
          {
            id: "until-now",
            start: `${year}-01-02`,
            end: `${year}-01-03`,
            a: "08:15",
            b: "09:45",
            now: true,
            label: "1/2 08:15 起",
          },
        ];
        for (const c of cases) {
          const old = await row.getAttribute("data-summary-query");
          await open();
          const dates = page.locator(".n-date-panel-date-input input");
          const times = page.locator(".n-date-panel .n-time-picker input");
          // 先结束后开始，避免更新开始日期时 Naive 将逆序区间自动换位。
          await type(dates.nth(0), `${year - 2}-01-01`);
          await type(dates.nth(1), c.end);
          await type(dates.nth(0), c.start);
          await type(times.nth(0), c.a);
          await type(times.nth(1), c.b);
          if (c.now) await activate(page.getByRole("checkbox", { name: "结束于现在" }), "Space");
          await shot(`${c.id}-panel`);
          await activate(page.getByRole("button", { name: "确定", exact: true }));
          await changed(old);
          const args = lastArgs();
          record(
            `${c.id} 键盘提交分钟及边界`,
            args.startAt === `${c.start}T${c.a}` &&
              (c.now ? args.untilNow === true && !args.endAt : args.endAt === `${c.end}T${c.b}`),
            JSON.stringify(args),
          );
          record(
            `${c.id} 标签`,
            (await trigger.innerText()).trim() === c.label,
            await trigger.innerText(),
          );
          await shot(c.id);
        }
        // 离开会按分钟更新的「至今」，避免慢速 CI 的时钟刷新干扰取消断言。
        const moving = await row.getAttribute("data-summary-query");
        await open();
        await activate(page.getByRole("button", { name: "7d", exact: true }));
        await changed(moving);
        const old = await row.getAttribute("data-summary-query");
        await open();
        await type(page.locator(".n-date-panel-date-input input").first(), `${year}-01-01`);
        await tabTo(page.locator(".n-date-panel-date-input input").first());
        await page.keyboard.press("Escape");
        await page.locator(".range-panel").waitFor({ state: "hidden" });
        record("Escape 丢弃草稿", (await row.getAttribute("data-summary-query")) === old, old);
        await open();
        record(
          "重开恢复已提交值",
          (await page.locator(".n-date-panel-date-input input").first().inputValue()) ===
            `${year}-10-01`,
          "未保留 Escape 草稿",
        );
        await activate(page.getByRole("button", { name: "取消", exact: true }));
        record("取消不查询", (await row.getAttribute("data-summary-query")) === old, old);
        hold = true;
        await page.getByRole("button", { name: "刷新", exact: true }).click();
        for (let n = 0; !release && n < 100; n++) await page.waitForTimeout(20);
        if (!release) throw new Error("刷新没有进入挂起状态");
        record(
          "刷新中禁止连点",
          await page.locator(".page-head button.n-button").isDisabled(),
          "挂起本次明细响应",
        );
        await shot("refreshing");
        release();
        await changed(old);
        await shot("updated");
        const list = page.getByRole("region", { name: "请求明细列表" });
        await list.focus();
        const count = s.ipcCalls.filter((c) => c.cmd === "query_events").length;
        await list.press("End");
        await page.waitForFunction(
          (n) => window.__TS_IPC_CALLS.filter((c) => c.cmd === "query_events").length > n,
          count,
        );
        await page.waitForFunction(
          () => !document.querySelector(".scroll-list-status")?.textContent.includes("正在加载"),
        );
        await page.evaluate(() => {
          const c = window.__TS_IPC_CALLS.find(
            (c) => c.cmd === "plugin:event|listen" && c.args.event === "usage-updated",
          );
          window.__TS_CB[c.args.handler]({ payload: { generation: 99 } });
        });
        await page.getByRole("button", { name: "有新数据 · 刷新" }).waitFor();
        record(
          "续页后仅提示新数据",
          await page.getByRole("button", { name: "有新数据 · 刷新" }).isVisible(),
          "后台通知",
        );
        await shot("paged-update");
        await page
          .locator('[aria-label="页面切换"]')
          .getByRole("radio", { name: "设置", exact: true })
          .click();
        for (const section of ["通用", "数据源", "用量数据", "价格", "外观", "高级"]) {
          await page.locator(".settings-nav button").filter({ hasText: section }).click();
          record(`设置分区 ${section}`, await page.locator(".settings-nav").isVisible(), section);
          await shot(`settings-${section}`);
        }
        await ctx.finishSession(s);
      } finally {
        release?.();
        await s.close();
      }
    }
  }
}
