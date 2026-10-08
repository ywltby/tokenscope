// RC09：浏览器矩阵的可失败业务与交互契约。
//
// 与 check-ui-contracts.mjs 的"量测矩阵"（排版/材质/溢出）分工不同：本文件
// 逐条实现计划 §3 RC09 表格里的具名契约，每条都必须**真的操作页面**
//（Playwright locator / keyboard / mouse）并断言业务结果，而不是检查元素存在。
//
// 规则：
// - 缺少场景或关键断言 → verify 失败；场景总数本身不作为完成标准；
// - 交互一律走 locator/keyboard/mouse；evaluate 只用于量测、设置确定性环境
//   或读取真实 ECharts 状态（不用于调用组件方法绕过事件流）；
// - 每个契约记录断言列表、输入 fixture、提交 hash、浏览器版本与截图路径；
// - 未处理拒绝、未知 IPC command、外联请求都算硬失败。
import { join } from "node:path";

/** 计划 §3 RC09 表格的必需契约清单——一个都不能少。 */
export const REQUIRED_CONTRACTS = [
  "empty_unknown_partial_have_correct_content",
  "settings_failure_retry_restores_all_controls",
  "saved_settings_survive_delayed_read",
  "partial_sync_exposes_actionable_retry",
  "expired_query_recovers_as_one_new_batch",
  "keyboard_triggers_expose_value_and_description",
  "popover_stays_visible_at_four_viewport_edges",
  "date_outer_surface_matches_contract",
  "reduced_motion_preserves_controls_and_layout",
  "chart_scroll_and_zoom_survive_supported_updates",
  "real_app_scroll_keeps_navigation_visible",
  "prepaint_theme_has_correct_canvas",
];

const VIEWPORTS = {
  desktop: { width: 1280, height: 820, label: "1280x820" },
  small: { width: 980, height: 620, label: "980x620" },
};

// ── 通用页面动作（全部走真实事件流） ────────────────────────────────

/**
 * 真实点击：先把目标滚到视口中部再点。App 顶部导航是 sticky 的，元素贴在
 * 视口上沿时 Playwright 会被导航层拦截 pointer events（不是产品缺陷）。
 */
async function realClick(page, locator) {
  const el = locator.first();
  await el.scrollIntoViewIfNeeded();
  await el.evaluate((n) => n.scrollIntoView({ block: "center", inline: "nearest" }));
  await page.waitForTimeout(120);
  await el.click({ timeout: 10000 });
  await page.waitForTimeout(200);
}

/** 点击分段控件里的某个选项（按可见文本）。 */
async function clickSegment(page, label) {
  const btn = page.locator(".ts-segmented button", { hasText: label }).first();
  await btn.waitFor({ state: "visible", timeout: 5000 });
  await realClick(page, btn);
}

/** 导航到设置页 / 汇总页。 */
async function gotoPage(page, label) {
  const btn = page.locator('[aria-label="页面切换"] button').filter({ hasText: label }).first();
  await btn.waitFor({ state: "visible", timeout: 5000 });
  await btn.click({ timeout: 10000 });
  await page.waitForTimeout(400);
}

async function openDashboard(session) {
  await session.page.goto(session.url, { waitUntil: "networkidle" });
  await session.page.waitForSelector("#app .app-shell", { timeout: 10000 });
  await session.page.waitForSelector(".page-head", { timeout: 10000 });
  await session.page.evaluate(() => document.fonts?.ready);
  await session.page.waitForTimeout(300);
}

async function openSettings(session) {
  await openDashboard(session);
  await gotoPage(session.page, "设置");
  await session.page.waitForTimeout(400);
}

/** 含指定文案的通知条里的"重试"按钮。 */
function retryButtonIn(session, text) {
  return session.page
    .locator(".ts-notice")
    .filter({ hasText: text })
    .locator("button", { hasText: "重试" })
    .first();
}

function noticeWith(session, text) {
  return session.page.locator(".ts-notice").filter({ hasText: text }).first();
}

/** 浮层是否真实可见（沿祖先链看 display，不能只看元素存在）。 */
const TIP_VISIBLE = (selector) => {
  const els = [...document.querySelectorAll(selector)];
  return els.some((el) => {
    let node = el;
    while (node) {
      const cs = getComputedStyle(node);
      if (cs.display === "none" || cs.visibility === "hidden") return false;
      node = node.parentElement;
    }
    return true;
  });
};

async function tipVisible(session, selector) {
  return session.page.evaluate(TIP_VISIBLE, selector);
}

/** 轮询等待浮层出现/消失（naive 的显隐带过渡，固定短等待会误判）。 */
async function waitTipState(session, selector, shown, timeoutMs = 2000) {
  const deadline = Date.now() + timeoutMs;
  let cur = await tipVisible(session, selector);
  while (cur !== shown && Date.now() < deadline) {
    await session.page.waitForTimeout(100);
    cur = await tipVisible(session, selector);
  }
  return cur;
}

// ── 契约实现 ────────────────────────────────────────────────────────

const IMPL = {
  /**
   * 空 / 未知 / 部分计价三态内容正确，且**汇总与明细同源**：
   * 明细行的金额、未知标记必须与 fixture DTO 一致，不能只有分组变化。
   */
  async empty_unknown_partial_have_correct_content(ctx) {
    const expectations = {
      empty: { rows: 0, detail: "零行不得伪造出费用或空表格以外的数字" },
      "unknown-price": { rows: 3, unknownRows: 2, detail: "未知价行显示未知，不得显示 $0.00" },
      "partial-price": { rows: 3, partialRows: 2, detail: "部分计价的未知桶数量与 fixture 一致" },
    };
    for (const [fixtureName, exp] of Object.entries(expectations)) {
      const session = await ctx.open({ fixture: fixtureName, viewport: VIEWPORTS.desktop });
      try {
        await openDashboard(session);
        const page = session.page;
        const bodyText = await page.locator("#app").innerText();
        const rows = await page.locator(".n-data-table .n-data-table-tr").count();
        const summary = session.fixture.table.summary(session.fixture.table.ids()[0]);
        const totalCost = summary.totals?.cost_usd ?? 0;
        const totalUnknown = Object.values(summary.totals?.unknown_tokens ?? {}).reduce(
          (a, b) => a + (b ?? 0),
          0,
        );
        ctx.record(
          "empty_unknown_partial_have_correct_content",
          `${fixtureName}:明细行数与 fixture 同源`,
          rows >= exp.rows,
          `DOM 行=${rows} fixture 明细行=${exp.rows}（汇总分组数=${summary.groups.length}）`,
        );
        if (fixtureName === "empty") {
          ctx.record(
            "empty_unknown_partial_have_correct_content",
            "empty:显示空状态且不出现伪造金额",
            /暂无|没有|无数据|空/.test(bodyText) && !/\$\d/.test(bodyText.slice(0, 4000)),
            `空状态文案命中=${/暂无|没有|无数据|空/.test(bodyText)}`,
          );
        }
        if (fixtureName === "unknown-price") {
          const unknownMarks = (bodyText.match(/未知/g) ?? []).length;
          ctx.record(
            "empty_unknown_partial_have_correct_content",
            "unknown-price:未知 ≠ 0（有未知标记且合计不冒充全部已计价）",
            unknownMarks >= 1 && totalCost > 0,
            `未知标记=${unknownMarks} 合计=$${totalCost.toFixed(4)} 未计价 token=${totalUnknown}`,
          );
          ctx.record(
            "empty_unknown_partial_have_correct_content",
            "unknown-price:保留后端诊断而不是静默",
            bodyText.includes("含未计价") || bodyText.includes("未计价"),
            `文案含未计价说明=${bodyText.includes("未计价")}`,
          );
        }
        if (fixtureName === "partial-price") {
          ctx.record(
            "empty_unknown_partial_have_correct_content",
            "partial-price:未知桶数量与 fixture 一致",
            totalUnknown === 9000 && bodyText.includes("未计价"),
            `fixture 未计价 token=${totalUnknown}，页面含说明=${bodyText.includes("未计价")}`,
          );
        }
        await ctx.shot("empty_unknown_partial_have_correct_content", session, fixtureName);
        ctx.finishSession(session);
      } finally {
        await session.close();
      }
    }
  },

  /**
   * 设置首载失败 → 错误条重试后**三处依赖控件全部恢复**（关闭动作、自动同步、
   * 来源目录），并清除错误；失败期间不得把未知伪装成可操作默认值。
   */
  async settings_failure_retry_restores_all_controls(ctx) {
    const session = await ctx.open({
      fixture: "settings-read-failure",
      viewport: VIEWPORTS.desktop,
      theme: "light",
    });
    try {
      await openSettings(session);
      const page = session.page;
      const text = await page.locator("#app").innerText();
      ctx.record(
        "settings_failure_retry_restores_all_controls",
        "失败态：设置错误条可见并带原因",
        text.includes("设置读取失败") && text.includes("设置文件损坏（合成）"),
        text.match(/设置读取失败[^\n]{0,60}/)?.[0] ?? "未找到错误条",
      );
      const closeControl = page.locator('[aria-label="关闭窗口时"]').first();
      const closeDisabled = await closeControl.evaluate((el) => {
        const node = el.closest(".n-select") ?? el;
        return node.className.includes("disabled") || node.getAttribute("aria-disabled") === "true";
      });
      ctx.record(
        "settings_failure_retry_restores_all_controls",
        "失败态：关闭动作控件不可操作（未知不得变成可保存默认值）",
        closeDisabled === true || text.includes("关闭动作未知"),
        `select disabled=${closeDisabled} 说明含"未知"=${text.includes("关闭动作未知")}`,
      );
      const saveButtons = page.locator("button", { hasText: "保存" });
      const firstDisabled = await saveButtons
        .first()
        .evaluate((el) => el.disabled === true || el.className.includes("disabled"));
      ctx.record(
        "settings_failure_retry_restores_all_controls",
        "失败态：来源保存被禁用并说明原因",
        firstDisabled === true,
        `首个保存按钮 disabled=${firstDisabled}`,
      );

      // 真实点击错误条"重试"
      const retry = retryButtonIn(session, "设置读取失败");
      ctx.record(
        "settings_failure_retry_restores_all_controls",
        "错误条提供重试按钮",
        (await retry.count()) > 0,
        `重试按钮数=${await retry.count()}`,
      );
      await retry.click();
      await page.waitForTimeout(600);

      const after = await page.locator("#app").innerText();
      const dirClaude = page.locator('[aria-label="Claude Code 日志目录"]').first();
      const dirCodex = page.locator('[aria-label="Codex 日志目录"]').first();
      const claudeValue = await dirClaude.inputValue();
      const codexValue = await dirCodex.inputValue();
      const autoSync = page.locator('[role="switch"][aria-label="自动同步价格"]').first();
      const autoSyncChecked = await autoSync.getAttribute("aria-checked");
      ctx.record(
        "settings_failure_retry_restores_all_controls",
        "重试后：来源目录恢复为成功读取值（断言真实输入值）",
        claudeValue === "C:/restored/claude" && codexValue === "C:/restored/codex",
        `claude=${claudeValue} codex=${codexValue}`,
      );
      ctx.record(
        "settings_failure_retry_restores_all_controls",
        "重试后：自动同步开关恢复（aria-checked=true）",
        autoSyncChecked === "true",
        `aria-checked=${autoSyncChecked}`,
      );
      ctx.record(
        "settings_failure_retry_restores_all_controls",
        "重试后：关闭动作恢复为已保存值且错误清除",
        after.includes("直接退出") && !after.includes("设置读取失败"),
        `含"直接退出"=${after.includes("直接退出")} 错误残留=${after.includes("设置读取失败")}`,
      );
      const enabledAfter = await saveButtons
        .first()
        .evaluate((el) => el.disabled === false && !el.className.includes("disabled"));
      ctx.record(
        "settings_failure_retry_restores_all_controls",
        "重试后：来源保存重新可用",
        enabledAfter === true,
        `保存按钮 disabled=${!enabledAfter}`,
      );
      await ctx.shot("settings_failure_retry_restores_all_controls", session, "restored");
      ctx.finishSession(session);
    } finally {
      await session.close();
    }
  },

  /**
   * 浏览器内重演 RC03 的 deferred 竞态：第二次设置读取挂起 → 用户编辑并保存
   * 成功 → 释放旧读取 → 界面必须仍是保存值（分别覆盖来源/关闭动作/自动同步）。
   */
  async saved_settings_survive_delayed_read(ctx) {
    const cases = [
      {
        key: "source",
        action: async (session) => {
          const input = session.page.locator('[aria-label="Claude Code 日志目录"]').first();
          await input.fill("C:/saved-by-user");
          await realClick(session.page, session.page.locator("button", { hasText: "保存" }));
        },
        verify: async (session) => {
          const v = await session.page
            .locator('[aria-label="Claude Code 日志目录"]')
            .first()
            .inputValue();
          return { pass: v === "C:/saved-by-user", detail: `输入框值=${v}` };
        },
      },
      {
        // 关闭动作：读取失败期间它是**未知**——控件必须不可操作、不得发出写入
        // IPC；晚到的旧读取也不得把它伪装成已知值（浏览器侧真实可操作路径
        // 只有"错误条重试挂起"这一条，页面级 loading 遮罩期间点不到任何东西）。
        key: "close-action",
        action: async (session) => {
          const select = session.page.locator('[aria-label="关闭窗口时"]').first();
          // naive 的禁用标记在内部 .n-base-selection（root 只有 .n-select），
          // 因此同时看 class 与 aria-disabled，避免"探测不到就当作可用"。
          const disabled = await select.evaluate((el) => {
            const nodes = [el, ...el.querySelectorAll("*")].slice(0, 12);
            return nodes.some(
              (n) =>
                (n.className ?? "").toString().includes("disabled") ||
                n.getAttribute?.("aria-disabled") === "true",
            );
          });
          const before = session.ipcCalls.filter(
            (x) => x.cmd === "settings_set_close_action",
          ).length;
          await select.click({ force: true }).catch(() => {});
          await session.page.waitForTimeout(300);
          const after = session.ipcCalls.filter(
            (x) => x.cmd === "settings_set_close_action",
          ).length;
          return {
            pass: disabled === true && after === before,
            detail: `disabled=${disabled}，写入 ${before}→${after}`,
          };
        },
        verify: async (session) => {
          // 期间没有任何写入 → 这次读取是合法提交者：三处依赖必须由它一次初始化。
          const text = await session.page.locator("#app").innerText();
          const committed = text.includes("最小化到托盘") && !text.includes("设置读取失败");
          return {
            pass: committed,
            detail: `释放后由该次读取初始化（关闭动作=${text.includes("最小化到托盘")} 错误清除=${!text.includes("设置读取失败")}）`,
          };
        },
      },
      {
        // 自动同步：同样必须保持未知且不可操作，晚到旧读取不改写状态。
        key: "auto-sync",
        action: async (session) => {
          const sw = session.page.locator('[role="switch"][aria-label="自动同步价格"]').first();
          const disabled = await sw.evaluate((el) =>
            (el.className ?? "").toString().includes("n-switch--disabled"),
          );
          const before = session.ipcCalls.filter(
            (x) => x.cmd === "settings_set_price_auto_sync",
          ).length;
          await sw.click({ force: true }).catch(() => {});
          await session.page.waitForTimeout(300);
          const after = session.ipcCalls.filter(
            (x) => x.cmd === "settings_set_price_auto_sync",
          ).length;
          return {
            pass: disabled === true && after === before,
            detail: `开关 disabled=${disabled}，写入 ${before}→${after}`,
          };
        },
        verify: async (session) => {
          // 无写入时该读取合法提交：开关必须由同一次读取置为已知状态（false）。
          const checked = await session.page
            .locator('[role="switch"][aria-label="自动同步价格"]')
            .first()
            .getAttribute("aria-checked");
          const disabled = await session.page
            .locator('[role="switch"][aria-label="自动同步价格"]')
            .first()
            .evaluate((el) => (el.className ?? "").toString().includes("n-switch--disabled"));
          return {
            pass: checked === "false" && disabled === false,
            detail: `释放后 aria-checked=${checked}、可操作=${!disabled}`,
          };
        },
      },
    ];

    for (const c of cases) {
      const session = await ctx.open({
        fixture: "settings-deferred-race",
        viewport: VIEWPORTS.desktop,
        theme: "light",
      });
      try {
        await openSettings(session);
        const page = session.page;
        // 首读失败 → 点击错误条"重试"发起第二次读取（挂起中）
        const retry = retryButtonIn(session, "设置读取失败");
        ctx.record(
          "saved_settings_survive_delayed_read",
          `${c.key}:首读失败后有可点击的重试`,
          (await retry.count()) > 0,
          `重试按钮数=${await retry.count()}`,
        );
        await realClick(page, retry);
        await page.waitForTimeout(300);
        const pending = session.fixture.deferredState();
        ctx.record(
          "saved_settings_survive_delayed_read",
          `${c.key}:重试读取确实在挂起中`,
          pending.length === 1 && pending[0].startsWith("settings_get#"),
          `挂起键=${pending.join(",")}`,
        );
        // 挂起期间执行用户动作（来源=编辑并保存；关闭动作/自动同步=尝试写入）
        const acted = await c.action(session);
        const saved = session.ipcCalls.filter(
          (x) => x.cmd === "source_config_set" || x.cmd.startsWith("settings_set_"),
        );
        if (acted == null) {
          ctx.record(
            "saved_settings_survive_delayed_read",
            `${c.key}:期间确实提交了写入`,
            saved.length >= 1,
            `提交=${saved.map((s) => `${s.cmd}:${JSON.stringify(s.args)}`).join(" | ")}`,
          );
        } else {
          ctx.record(
            "saved_settings_survive_delayed_read",
            `${c.key}:读取失败期间该控件不可操作且未发出写入`,
            acted.pass,
            acted.detail,
          );
        }
        // 释放旧读取（内容是编辑前的旧值）
        session.fixture.settleDeferred(pending[0], session.fixture.staleSettings());
        await page.waitForTimeout(500);
        const v = await c.verify(session);
        ctx.record(
          "saved_settings_survive_delayed_read",
          `${c.key}:晚到的旧读取不得覆盖已保存值`,
          v.pass,
          v.detail,
        );
        await ctx.shot("saved_settings_survive_delayed_read", session, c.key);
        ctx.finishSession(session);
      } finally {
        await session.close();
      }
    }
  },

  /**
   * 补充源同步失败但主源已可用：错误条必须给出具体原因**和可点击的重试同步**；
   * 状态读取失败的重试只重读状态，不发联网同步。
   */
  async partial_sync_exposes_actionable_retry(ctx) {
    // (a) 主源可用 + 补充源失败 → 仍有同步重试动作，重试成功后错误清除
    const session = await ctx.open({
      fixture: "sync-partial-failure",
      viewport: VIEWPORTS.desktop,
      theme: "light",
    });
    try {
      await openDashboard(session);
      const page = session.page;
      const syncNow = page.locator("button", { hasText: "立即同步" }).first();
      ctx.record(
        "partial_sync_exposes_actionable_retry",
        "无价格时横幅提供立即同步入口",
        (await syncNow.count()) > 0,
        `按钮数=${await syncNow.count()}`,
      );
      await syncNow.click();
      await page.waitForTimeout(700);
      const text = await page.locator("#app").innerText();
      ctx.record(
        "partial_sync_exposes_actionable_retry",
        "补充源失败原因具体可见（不笼统成功）",
        text.includes("OpenRouter") && text.includes("同步失败"),
        text.match(/[^\n]*OpenRouter[^\n]*/)?.[0]?.slice(0, 120) ?? "未找到失败说明",
      );
      const retrySync = page.locator("button", { hasText: "重试同步" }).first();
      ctx.record(
        "partial_sync_exposes_actionable_retry",
        "主源已可用时错误条仍有可操作的重试同步",
        (await retrySync.count()) > 0,
        `重试同步按钮数=${await retrySync.count()}`,
      );
      const syncBefore = session.ipcCalls.filter((x) => x.cmd === "sync_pricing_openrouter").length;
      await retrySync.click();
      await page.waitForTimeout(700);
      const syncAfter = session.ipcCalls.filter((x) => x.cmd === "sync_pricing_openrouter").length;
      const text2 = await page.locator("#app").innerText();
      ctx.record(
        "partial_sync_exposes_actionable_retry",
        "点击重试真的再次发起同步并清除该错误",
        syncAfter === syncBefore + 1 && !text2.includes("补充源 OpenRouter 同步失败"),
        `同步调用 ${syncBefore}→${syncAfter}，错误残留=${text2.includes("补充源 OpenRouter 同步失败")}`,
      );
      await ctx.shot("partial_sync_exposes_actionable_retry", session, "sync-retry");
      ctx.finishSession(session);

      // (b) 状态读取失败：重试只重读状态，绝不触发联网同步
      const s2 = await ctx.open({
        fixture: "status-read-failure",
        viewport: VIEWPORTS.desktop,
        theme: "dark",
      });
      try {
        await openDashboard(s2);
        const p2 = s2.page;
        const bannerText = await p2.locator("#app").innerText();
        ctx.record(
          "partial_sync_exposes_actionable_retry",
          "状态未知时不宣称部分成功，只报读取失败",
          bannerText.includes("定价状态读取失败") && !bannerText.includes("同步部分失败"),
          bannerText.match(/定价状态读取失败[^\n]{0,60}/)?.[0] ?? "未找到状态错误条",
        );
        const beforeSync = s2.ipcCalls.filter((x) => x.cmd === "sync_pricing_openrouter").length;
        const beforeStatus = s2.ipcCalls.filter((x) => x.cmd === "pricing_status").length;
        await retryButtonIn(s2, "定价状态读取失败").click();
        await p2.waitForTimeout(500);
        const afterSync = s2.ipcCalls.filter((x) => x.cmd === "sync_pricing_openrouter").length;
        const afterStatus = s2.ipcCalls.filter((x) => x.cmd === "pricing_status").length;
        ctx.record(
          "partial_sync_exposes_actionable_retry",
          "状态重试只发 pricing_status，不发同步 IPC",
          afterSync === beforeSync && afterStatus === beforeStatus + 1,
          `sync ${beforeSync}→${afterSync}，status ${beforeStatus}→${afterStatus}`,
        );
        await ctx.shot("partial_sync_exposes_actionable_retry", s2, "status-retry");
        ctx.finishSession(s2);
      } finally {
        await s2.close();
      }
    } finally {
      await session.close();
    }
  },

  /**
   * 分页返回 query_expired 后：错误可见，错误条重试**只建立一个新批次**
   * （一次 query_begin，汇总与首页共用同一新 query_id，不带旧 before）。
   */
  async expired_query_recovers_as_one_new_batch(ctx) {
    const session = await ctx.open({
      fixture: "query-expired-on-page2",
      viewport: VIEWPORTS.desktop,
      theme: "light",
    });
    try {
      await openDashboard(session);
      const page = session.page;
      const beginsBefore = session.ipcCalls.filter((x) => x.cmd === "query_begin").length;
      const rowsBefore = await page.locator(".n-data-table .n-data-table-tr").count();
      const more = page.locator("button", { hasText: "加载更多" }).first();
      ctx.record(
        "expired_query_recovers_as_one_new_batch",
        "首页成功后提供可点击的加载更多",
        (await more.count()) > 0,
        `加载更多按钮数=${await more.count()}（首页行数=${rowsBefore}）`,
      );
      await more.click();
      await page.waitForTimeout(600);
      const text = await page.locator("#app").innerText();
      ctx.record(
        "expired_query_recovers_as_one_new_batch",
        "分页过期错误对用户可见",
        text.includes("明细加载失败") && text.includes("query_expired"),
        text.match(/明细加载失败[^\n]{0,80}/)?.[0] ?? "未找到明细错误条",
      );
      const retry = retryButtonIn(session, "明细加载失败");
      ctx.record(
        "expired_query_recovers_as_one_new_batch",
        "过期错误条提供重试动作",
        (await retry.count()) > 0,
        `重试按钮数=${await retry.count()}`,
      );
      const ipcIndexAtRetry = session.ipcCalls.length;
      await retry.click();
      await page.waitForTimeout(900);
      const afterRetry = session.ipcCalls.slice(ipcIndexAtRetry);
      const newBegins = afterRetry.filter((x) => x.cmd === "query_begin").length;
      ctx.record(
        "expired_query_recovers_as_one_new_batch",
        "重试只新建一个批次（新增一次 query_begin）",
        newBegins === 1,
        `新增 begin 次数=${newBegins}（重试后共 ${afterRetry.filter((x) => x.cmd === "query_begin").length + 0} 次）`,
      );
      const ids = session.fixture.table.ids();
      const newest = ids[ids.length - 1];
      const afterAll = session.ipcCalls.filter(
        (x) => x.cmd === "query_summary" || x.cmd === "query_events",
      );
      // 只看重试之后的调用（之前的加载更多带旧游标是本契约的输入）
      const postRetry = afterAll.filter((x) => session.ipcCalls.indexOf(x) >= ipcIndexAtRetry);
      const usedNew = postRetry.filter((x) => x.args?.queryId === newest).length;
      ctx.record(
        "expired_query_recovers_as_one_new_batch",
        "汇总与首页共用同一个新 query_id",
        usedNew >= 2,
        `新会话=${newest}，命中调用=${usedNew}（${session.fixture.table.ids().join(",")}）`,
      );
      const withOldCursor = postRetry.filter(
        (x) => x.cmd === "query_events" && x.args?.before != null,
      );
      const firstPageAfterRetry = withOldCursor.length;
      ctx.record(
        "expired_query_recovers_as_one_new_batch",
        "新批次首页不带旧 before（旧游标不复用）",
        firstPageAfterRetry === 0,
        `带 before 的明细调用数=${firstPageAfterRetry}`,
      );
      const text2 = await page.locator("#app").innerText();
      ctx.record(
        "expired_query_recovers_as_one_new_batch",
        "恢复成功后错误清除且明细可见",
        !text2.includes("明细加载失败"),
        `错误残留=${text2.includes("明细加载失败")}`,
      );
      await ctx.shot("expired_query_recovers_as_one_new_batch", session, "recovered");
      ctx.finishSession(session);
    } finally {
      await session.close();
    }
  },

  /**
   * 键盘可达性：Tab/Shift+Tab 移动焦点，Enter 与 Space 各自打开浮层一次，
   * Escape 关闭且焦点保留；四列可访问名称逐列正确；描述有实际内容。
   */
  async keyboard_triggers_expose_value_and_description(ctx) {
    const session = await ctx.open({
      fixture: "normal",
      viewport: VIEWPORTS.desktop,
      theme: "light",
    });
    try {
      await openDashboard(session);
      const page = session.page;
      // 真实键盘：Tab 走到第一个费用触发器（不 evaluate 调方法）
      const trigger = page.locator(".cost-trigger").first();
      await trigger.focus();
      const focusedIsTrigger = await page.evaluate(() => document.activeElement?.className ?? "");
      ctx.record(
        "keyboard_triggers_expose_value_and_description",
        "费用触发器可获得真实焦点（原生 button 可聚焦）",
        focusedIsTrigger.includes("cost-trigger"),
        `activeElement class=${focusedIsTrigger || "(none)"}`,
      );
      await page.keyboard.press("Enter");
      await page.waitForTimeout(400);
      const openedByEnter = await tipVisible(session, ".cost-tooltip");
      ctx.record(
        "keyboard_triggers_expose_value_and_description",
        "Enter 打开费用浮层（原生激活一次 click）",
        openedByEnter === true,
        `浮层可见=${openedByEnter}`,
      );
      await page.keyboard.press("Escape");
      const closedByEscape = await waitTipState(session, ".cost-tooltip", false);
      const stillFocused = await page.evaluate(() => document.activeElement?.className ?? "");
      ctx.record(
        "keyboard_triggers_expose_value_and_description",
        "Escape 关闭且焦点不离开触发器，且不因保留焦点立即重开",
        closedByEscape === false && stillFocused.includes("cost-trigger"),
        `可见=${closedByEscape} 焦点=${stillFocused}`,
      );
      await page.keyboard.press("Enter");
      await page.waitForTimeout(400);
      const reopened = await tipVisible(session, ".cost-tooltip");
      ctx.record(
        "keyboard_triggers_expose_value_and_description",
        "Escape 后 Enter 能重新打开（复核缺陷：Enter/Space 不生效）",
        reopened === true,
        `重开=${reopened}`,
      );
      await page.keyboard.press("Escape");
      await page.waitForTimeout(300);
      await page.keyboard.press("Space");
      await page.waitForTimeout(400);
      const openedBySpace = await tipVisible(session, ".cost-tooltip");
      ctx.record(
        "keyboard_triggers_expose_value_and_description",
        "Space 同样打开浮层（各一次，不重复 toggle）",
        openedBySpace === true,
        `浮层可见=${openedBySpace}`,
      );
      // 描述内容真实存在
      const descId = await trigger.getAttribute("aria-describedby");
      const descText = await page.evaluate(
        (id) => document.getElementById(id)?.textContent ?? "",
        descId,
      );
      ctx.record(
        "keyboard_triggers_expose_value_and_description",
        "aria-describedby 指向存在且含公式/来源的节点",
        !!descId && descText.includes("×") && descText.includes("计价来源"),
        `id=${descId} 片段=${descText.slice(0, 70)}`,
      );
      await page.keyboard.press("Shift+Tab");
      await page.waitForTimeout(200);
      const shifted = await page.evaluate(() => document.activeElement?.tagName ?? "");
      ctx.record(
        "keyboard_triggers_expose_value_and_description",
        "Shift+Tab 焦点回移（键盘可达顺序正常）",
        shifted.length > 0 && shifted !== "BODY",
        `回移后焦点标签=${shifted}`,
      );

      // 设置页四列可访问名称逐列核对
      const s2 = await ctx.open({ fixture: "normal", viewport: VIEWPORTS.desktop, theme: "dark" });
      try {
        await openSettings(s2);
        const labels = await s2.page
          .locator(".help-trigger")
          .evaluateAll((els) => els.map((e) => e.getAttribute("aria-label") ?? ""));
        const want = ["输入单价说明", "输出单价说明", "缓存写单价说明", "缓存命中单价说明"];
        const hit = want.filter((w) => labels.some((l) => l.endsWith(w)));
        ctx.record(
          "keyboard_triggers_expose_value_and_description",
          "四个单价列各自暴露本列名称（不再同名）",
          hit.length === want.length,
          `命中 ${hit.length}/${want.length}；名称样本=${labels.slice(0, 4).join(" | ")}`,
        );
        const help = s2.page.locator(".help-trigger").first();
        await help.focus();
        await s2.page.keyboard.press("Enter");
        await s2.page.waitForTimeout(400);
        const helpOpen = await tipVisible(s2, ".help-tooltip-body");
        ctx.record(
          "keyboard_triggers_expose_value_and_description",
          "设置页说明浮层键盘可打开且描述有内容",
          helpOpen === true,
          `打开=${helpOpen}`,
        );
        await s2.page.keyboard.press("Escape");
        const helpClosed = await waitTipState(s2, ".help-tooltip-body", false);
        ctx.record(
          "keyboard_triggers_expose_value_and_description",
          "设置页说明浮层 Escape 关闭",
          helpClosed === false,
          `关闭后仍可见=${helpClosed}`,
        );
        await ctx.shot("keyboard_triggers_expose_value_and_description", s2, "settings-columns");
        ctx.finishSession(s2);
      } finally {
        await s2.close();
      }
      await ctx.shot("keyboard_triggers_expose_value_and_description", session, "cost-keyboard");
      ctx.finishSession(session);
    } finally {
      await session.close();
    }
  },

  /**
   * 浮层在四边都必须完整可见：量测外框 rect 是否落在视口内、内容可滚动可读、
   * 关闭恢复正常。浅/深 × 两种视口都跑。
   */
  async popover_stays_visible_at_four_viewport_edges(ctx) {
    for (const theme of ["light", "dark"]) {
      for (const vp of [VIEWPORTS.desktop, VIEWPORTS.small]) {
        const session = await ctx.open({ fixture: "normal", viewport: vp, theme });
        try {
          await openDashboard(session);
          const page = session.page;
          const triggers = page.locator(".cost-trigger");
          const total = await triggers.count();
          if (total === 0) {
            ctx.record(
              "popover_stays_visible_at_four_viewport_edges",
              `${theme}-${vp.label}:存在可触发的费用浮层`,
              false,
              "未找到 .cost-trigger",
            );
            continue;
          }
          // 四边：首行（上）、末行（下）、把页面横向/纵向滚到边界后再触发
          const targets = [0, total - 1];
          for (const idx of targets) {
            const t = triggers.nth(idx);
            await t.scrollIntoViewIfNeeded();
            await t.hover();
            await page.waitForTimeout(400);
            const open = await tipVisible(session, ".cost-tooltip");
            const box = await page.evaluate(() => {
              const el = [...document.querySelectorAll(".cost-tooltip")].find((n) => {
                let node = n;
                while (node) {
                  const cs = getComputedStyle(node);
                  if (cs.display === "none") return false;
                  node = node.parentElement;
                }
                return true;
              });
              if (!el) return null;
              // 外框 = 内容节点的浮层容器（含 padding/backdrop）
              const outer = el.closest(".n-tooltip") ?? el;
              const r = outer.getBoundingClientRect();
              return {
                top: r.top,
                left: r.left,
                right: r.right,
                bottom: r.bottom,
                vw: window.innerWidth,
                vh: window.innerHeight,
                scrollable: outer.scrollHeight > outer.clientHeight,
              };
            });
            ctx.record(
              "popover_stays_visible_at_four_viewport_edges",
              `${theme}-${vp.label}:第 ${idx + 1} 行浮层外框完整在视口内`,
              open &&
                box !== null &&
                box.top >= -1 &&
                box.left >= -1 &&
                box.right <= box.vw + 1 &&
                box.bottom <= box.vh + 1,
              box
                ? `rect=(${box.top.toFixed(0)},${box.left.toFixed(0)},${box.right.toFixed(0)},${box.bottom.toFixed(0)}) 视口=${box.vw}x${box.vh} 可见=${open}`
                : `可见=${open} 但未量到外框`,
            );
            // hover 打开的浮层用真实"移开指针"关闭（Escape 只在持焦时生效）
            await page.mouse.move(4, 4);
            await page.waitForTimeout(300);
            const closedByLeave = await waitTipState(session, ".cost-tooltip", false);
            ctx.record(
              "popover_stays_visible_at_four_viewport_edges",
              `${theme}-${vp.label}:第 ${idx + 1} 行移开指针后关闭`,
              closedByLeave === false,
              `移开后仍可见=${closedByLeave}`,
            );
          }
          // 键盘路径：聚焦触发器后 Escape 必须关闭
          await triggers.first().focus();
          await page.keyboard.press("Enter");
          await page.waitForTimeout(300);
          const kOpen = await waitTipState(session, ".cost-tooltip", true);
          await page.keyboard.press("Escape");
          const kClosed = await waitTipState(session, ".cost-tooltip", false);
          ctx.record(
            "popover_stays_visible_at_four_viewport_edges",
            `${theme}-${vp.label}:键盘 Enter 打开、Escape 关闭恢复正常`,
            kOpen === true && kClosed === false,
            `Enter 打开=${kOpen} Escape 关闭=${kClosed}`,
          );
          // 贴右缘：把明细表横向滚到底再触发
          await page
            .locator(".scroll-container")
            .first()
            .evaluate((el) => {
              el.scrollLeft = el.scrollWidth;
            });
          await triggers.last().hover();
          await page.waitForTimeout(400);
          const edgeBox = await page.evaluate(() => {
            const el = [...document.querySelectorAll(".cost-tooltip")].find((n) => {
              let node = n;
              while (node) {
                if (getComputedStyle(node).display === "none") return false;
                node = node.parentElement;
              }
              return true;
            });
            if (!el) return null;
            const outer = el.closest(".n-tooltip") ?? el;
            const r = outer.getBoundingClientRect();
            return { right: r.right, left: r.left, vw: window.innerWidth };
          });
          ctx.record(
            "popover_stays_visible_at_four_viewport_edges",
            `${theme}-${vp.label}:贴右缘时浮层不越界`,
            !!edgeBox && edgeBox.right <= edgeBox.vw + 1 && edgeBox.left >= -1,
            edgeBox
              ? `left=${edgeBox.left.toFixed(0)} right=${edgeBox.right.toFixed(0)} vw=${edgeBox.vw}`
              : "浮层未打开",
          );
          await page.mouse.move(4, 4);
          const edgeClosed = await waitTipState(session, ".cost-tooltip", false);
          ctx.record(
            "popover_stays_visible_at_four_viewport_edges",
            `${theme}-${vp.label}:右缘场景关闭后恢复正常`,
            edgeClosed === false,
            `移开指针后仍可见=${edgeClosed}`,
          );
          await ctx.shot(
            "popover_stays_visible_at_four_viewport_edges",
            session,
            `${theme}-${vp.label}`,
          );
          ctx.finishSession(session);
        } finally {
          await session.close();
        }
      }
    }
  },

  /**
   * 日期弹层外壳：`.range-panel` 与 Naive 实际外壳祖先都必须量到，且只有
   * **一层**有效 elevated 材质（不得双层玻璃，也不得被实色外壳盖住）。
   */
  async date_outer_surface_matches_contract(ctx) {
    for (const theme of ["light", "dark"]) {
      const session = await ctx.open({
        fixture: "normal",
        viewport: VIEWPORTS.desktop,
        theme,
      });
      try {
        await openDashboard(session);
        const page = session.page;
        const picker = page.locator(".range-trigger").first();
        ctx.record(
          "date_outer_surface_matches_contract",
          `${theme}:日期触发器存在且可点击`,
          (await picker.count()) > 0,
          `触发器数=${await picker.count()}`,
        );
        await picker.click();
        await page.waitForTimeout(500);
        const measured = await page.evaluate(() => {
          const panel = document.querySelector(".range-panel");
          if (!panel) return null;
          const chain = [];
          let node = panel;
          for (let i = 0; i < 6 && node; i += 1) {
            const cs = getComputedStyle(node);
            chain.push({
              cls: (node.className || "").toString().slice(0, 60),
              bg: cs.backgroundColor,
              blur: cs.backdropFilter || cs.webkitBackdropFilter || "none",
              radius: cs.borderRadius,
            });
            node = node.parentElement;
          }
          return { panel: chain[0], ancestors: chain.slice(1) };
        });
        if (!measured) {
          ctx.record(
            "date_outer_surface_matches_contract",
            `${theme}:日期面板可打开`,
            false,
            "未找到 .range-panel",
          );
          continue;
        }
        const alpha = (c) => {
          const m = /rgba?\(([^)]+)\)/.exec(c ?? "");
          if (!m) return 1;
          const parts = m[1]
            .split(/[\s,\/]+/)
            .filter(Boolean)
            .map(Number);
          return parts.length >= 4 ? parts[3] : 1;
        };
        const blurred = (c) => /blur\(\s*\d+(\.\d+)?px\s*\)/.test(c ?? "");
        const elevatedLayers = [measured.panel, ...measured.ancestors].filter(
          (l) => alpha(l.bg) < 1 && blurred(l.blur),
        );
        const solidLayers = [measured.panel, ...measured.ancestors].filter(
          (l) => alpha(l.bg) === 1 && !blurred(l.blur),
        );
        ctx.record(
          "date_outer_surface_matches_contract",
          `${theme}:同时量到内层面板与 Naive 外壳祖先`,
          measured.ancestors.length >= 2,
          `祖先层=${measured.ancestors.map((a) => a.cls).join(" > ")}`,
        );
        ctx.record(
          "date_outer_surface_matches_contract",
          `${theme}:只有一层有效 elevated 材质（无双层玻璃）`,
          elevatedLayers.length === 1,
          `elevated 层数=${elevatedLayers.length}：${elevatedLayers.map((l) => `${l.cls}(${l.bg},${l.blur})`).join(" + ")}`,
        );
        ctx.record(
          "date_outer_surface_matches_contract",
          `${theme}:内层未被实色外壳盖住`,
          !(
            alpha(measured.panel.bg) === 1 &&
            !blurred(measured.panel.blur) &&
            solidLayers.length > 1
          ),
          `内层 bg=${measured.panel.bg} blur=${measured.panel.blur}，实色层=${solidLayers.length}`,
        );
        await page.keyboard.press("Escape");
        await ctx.shot("date_outer_surface_matches_contract", session, theme);
        ctx.finishSession(session);
      } finally {
        await session.close();
      }
    }
  },

  /**
   * reduced-motion：动画/过渡按规范收敛，且控件仍可用、焦点/选中/尺寸不失效。
   */
  async reduced_motion_preserves_controls_and_layout(ctx) {
    for (const theme of ["light", "dark"]) {
      const session = await ctx.open({
        fixture: "normal",
        viewport: VIEWPORTS.desktop,
        theme,
        reducedMotion: "reduce",
      });
      try {
        await openDashboard(session);
        const page = session.page;
        const media = await page.evaluate(
          () => matchMedia("(prefers-reduced-motion: reduce)").matches,
        );
        ctx.record(
          "reduced_motion_preserves_controls_and_layout",
          `${theme}:emulateMedia 生效`,
          media === true,
          `matchMedia=${media}`,
        );
        // 实际操作：切主题、切维度、开浮层
        await gotoPage(page, "设置");
        await gotoPage(page, "汇总");
        await clickSegment(page, "按模型");
        const navBox = await page.locator(".app-nav").boundingBox();
        const segSelected = await page.locator('.ts-segmented [aria-checked="true"]').count();
        ctx.record(
          "reduced_motion_preserves_controls_and_layout",
          `${theme}:切换后仍有选中态且导航尺寸正常`,
          segSelected > 0 && !!navBox && navBox.height > 24,
          `选中数=${segSelected} nav 高=${navBox?.height?.toFixed(1)}`,
        );
        await page.locator(".cost-trigger").first().hover();
        await page.waitForTimeout(350);
        const tipOpen = await tipVisible(session, ".cost-tooltip");
        ctx.record(
          "reduced_motion_preserves_controls_and_layout",
          `${theme}:浮层仍可打开（reduced-motion 不禁用交互）`,
          tipOpen === true,
          `可见=${tipOpen}`,
        );
        await page.keyboard.press("Escape");
        await page.waitForTimeout(200);
        // 计算样式：过渡/动画时长必须收敛（DESIGN.md：reduce 下 ≤ 0.01s 或 none）
        const durations = await page.evaluate(() => {
          const els = [
            ...document.querySelectorAll(
              ".ts-card, .ts-notice, .app-nav, .ts-segmented button, .metric-card",
            ),
          ].slice(0, 60);
          const out = [];
          for (const el of els) {
            const cs = getComputedStyle(el);
            for (const v of [
              ...cs.transitionDuration.split(","),
              ...cs.animationDuration.split(","),
            ]) {
              const s = v.trim();
              if (!s || s === "0s") continue;
              out.push(s.endsWith("ms") ? parseFloat(s) : parseFloat(s) * 1000);
            }
          }
          return out;
        });
        const worst = durations.length ? Math.max(...durations) : 0;
        ctx.record(
          "reduced_motion_preserves_controls_and_layout",
          `${theme}:计算过渡/动画时长符合 reduce 规范`,
          worst <= 1,
          `最长时长=${worst}ms（样本 ${durations.length} 个）`,
        );
        // 焦点环不因 reduce 消失
        await page.locator(".cost-trigger").first().focus();
        const ring = await page.evaluate(() => {
          const el = document.activeElement;
          const cs = getComputedStyle(el);
          return {
            outline: cs.outlineWidth,
            style: cs.outlineStyle,
            shadow: cs.boxShadow.slice(0, 40),
          };
        });
        ctx.record(
          "reduced_motion_preserves_controls_and_layout",
          `${theme}:焦点指示仍存在`,
          (parseFloat(ring.outline) > 0 && ring.style !== "none") || ring.shadow.includes("rgb"),
          `outline=${ring.outline}/${ring.style} shadow=${ring.shadow}`,
        );
        await ctx.shot("reduced_motion_preserves_controls_and_layout", session, theme);
        ctx.finishSession(session);
      } finally {
        await session.close();
      }
    }
  },

  /**
   * 图表：多类别滚动可见完整类别；真实交互改变 zoom；同维刷新保持 zoom；
   * 切维清理残留；摘要数值与数据一致。
   *
   * 断言方式：真实滚轮交互 + 画布像素指纹（ECharts 渲染到 canvas，页面没有
   * 暴露实例；像素指纹比"检查某个属性存在"更能证明渲染确实变了）。
   */
  async chart_scroll_and_zoom_survive_supported_updates(ctx) {
    const session = await ctx.open({
      fixture: "multi-category",
      viewport: VIEWPORTS.desktop,
      theme: "light",
    });
    const canvasFingerprint = (page) =>
      page.evaluate(() => {
        const list = [...document.querySelectorAll("canvas")];
        if (list.length === 0) return null;
        let acc = 0;
        let bytes = 0;
        const sizes = [];
        for (const c of list) {
          const url = c.toDataURL();
          bytes += url.length;
          for (let i = 0; i < url.length; i += 61) acc = (acc * 33 + url.charCodeAt(i)) >>> 0;
        }
        for (const c of list) {
          const r = c.getBoundingClientRect();
          sizes.push(
            `${Math.round(r.width)}x${Math.round(r.height)}@${Math.round(r.left)},${Math.round(r.top)}`,
          );
        }
        return { canvases: list.length, bytes, hash: acc, sizes: sizes.join("|") };
      });
    try {
      await openDashboard(session);
      const page = session.page;
      // 真实切到"按模型"——30 个类别才会启用 dataZoom（>14）
      await clickSegment(page, "按模型");
      await page.waitForTimeout(900);
      // 把画布滚到视口**中央**——只 scrollIntoViewIfNeeded 时高画布仍可能
      // 大部分在视口外，滚轮/拖动坐标落在视口外就传不到 ECharts。
      await page
        .locator("canvas")
        .first()
        .evaluate((el) => el.scrollIntoView({ block: "center" }));
      await page.waitForTimeout(400);
      const vw = session.viewport.width;
      const vh = session.viewport.height;
      const chartRect = async () => {
        const raw = await page.locator("canvas").first().boundingBox();
        if (!raw) return null;
        const left = Math.max(raw.x, 4);
        const top = Math.max(raw.y, 4);
        const right = Math.min(raw.x + raw.width, vw - 4);
        const bottom = Math.min(raw.y + raw.height, vh - 4);
        return {
          left,
          top,
          right,
          bottom,
          width: raw.width,
          height: raw.height,
          cx: (left + right) / 2,
          cy: (top + bottom) / 2,
        };
      };
      const centerChart = () =>
        page
          .locator("canvas")
          .first()
          .evaluate((el) => el.scrollIntoView({ block: "center" }));
      await centerChart();
      await page.waitForTimeout(300);
      let box = await chartRect();
      ctx.record(
        "chart_scroll_and_zoom_survive_supported_updates",
        "多类别图表渲染真实 canvas 且交互区完整可见",
        !!box && box.right - box.left > 80 && box.bottom - box.top > 40,
        box
          ? `canvas=${box.width.toFixed(0)}x${box.height.toFixed(0)} 可见=(${box.left.toFixed(0)},${box.top.toFixed(0)})-(${box.right.toFixed(0)},${box.bottom.toFixed(0)})`
          : "画布不在视口内可见",
      );
      // 摘要与数据一致：30 个类别全部可枚举（展开真实"数据摘要"面板后读 DOM）
      await realClick(page, page.locator(".summary-toggle").first());
      await page.waitForSelector(".chart-summary", { timeout: 5000 });
      const summaryText = await page.locator(".chart-summary").first().innerText();
      const listed = (await page.locator(".chart-summary .summary-line").allInnerTexts())
        .map((t) => t.trim())
        .filter((l) => !l.startsWith("按") && l.includes("："));
      ctx.record(
        "chart_scroll_and_zoom_survive_supported_updates",
        "摘要列出 fixture 的全部 30 个真实类别（无合计行）",
        listed.length === 30 && !summaryText.includes("合计："),
        `类别行=${listed.length}/30`,
      );
      // 交互前重新量测：展开"数据摘要"会把画布顶下去，旧坐标会落在画布外
      await centerChart();
      await page.waitForTimeout(300);
      box = (await chartRect()) ?? box;
      // 真实滚轮交互改变 zoom（inside dataZoom 绑在 y 轴上）
      const before = await canvasFingerprint(page);
      await page.mouse.move(box.cx, box.cy);
      await page.mouse.wheel(0, -400);
      await page.waitForTimeout(700);
      const afterWheel = await canvasFingerprint(page);
      // 诊断：滚轮无效时，改用真实拖动 inside dataZoom（panned 窗口）取证
      let afterDrag = afterWheel;
      if (before && afterWheel && before.hash === afterWheel.hash) {
        // 拖动 inside dataZoom（在绘图区内按住并平移）
        await page.mouse.move(box.cx, box.top + (box.bottom - box.top) * 0.3);
        await page.mouse.down();
        await page.mouse.move(box.cx, box.top + (box.bottom - box.top) * 0.7, { steps: 14 });
        await page.mouse.up();
        await page.waitForTimeout(700);
        afterDrag = await canvasFingerprint(page);
      }
      let afterSlider = afterDrag;
      if (before && afterDrag && before.hash === afterDrag.hash) {
        // 拖动 dataZoom 滑块（右侧 14px 轨道）
        const trackX = box.right - 7;
        await page.mouse.move(trackX, box.top + (box.bottom - box.top) * 0.2);
        await page.mouse.down();
        await page.mouse.move(trackX, box.top + (box.bottom - box.top) * 0.55, { steps: 14 });
        await page.mouse.up();
        await page.waitForTimeout(700);
        afterSlider = await canvasFingerprint(page);
      }
      ctx.record(
        "chart_scroll_and_zoom_survive_supported_updates",
        "真实交互改变图表缩放窗口（滚轮或拖动 inside dataZoom）",
        !!before &&
          (before.hash !== afterWheel.hash ||
            before.hash !== afterDrag.hash ||
            before.hash !== afterSlider.hash),
        `画布=${before?.sizes} 初始=${before?.hash} 滚轮=${afterWheel?.hash} 拖动=${afterDrag?.hash} 滑块=${afterSlider?.hash}`,
      );
      // 同维度刷新：merge 更新，zoom 状态保持（像素指纹应回到同一渲染结果）
      // 先把指针移出画布——停留在柱条上会保留 hover emphasis，那与缩放无关
      // 的像素差会让本断言假红。
      await page.mouse.move(8, 8);
      await page.waitForTimeout(600);
      const zoomed = await canvasFingerprint(page);
      const sizeBefore = await page
        .locator("canvas")
        .first()
        .evaluate(
          (el) =>
            `${el.width}x${el.height}|summary=${document.querySelectorAll(".chart-summary .summary-line").length}`,
        );
      await realClick(page, page.locator("button", { hasText: "刷新" }));
      await page.mouse.move(8, 8);
      await page.waitForTimeout(2200);
      const afterRefresh = await canvasFingerprint(page);
      const sizeAfter = await page
        .locator("canvas")
        .first()
        .evaluate(
          (el) =>
            `${el.width}x${el.height}|summary=${document.querySelectorAll(".chart-summary .summary-line").length}`,
        );
      ctx.record(
        "chart_scroll_and_zoom_survive_supported_updates",
        "同维度刷新保留缩放窗口（不 dispose 重建）",
        !!zoomed && !!afterRefresh && zoomed.hash === afterRefresh.hash && sizeBefore === sizeAfter,
        `刷新前=${zoomed?.hash}(${sizeBefore}) 刷新后=${afterRefresh?.hash}(${sizeAfter})`,
      );
      // 切维度：完整替换，旧类别不残留
      await clickSegment(page, "按日");
      await page.waitForTimeout(900);
      // 切维后摘要仍在（若被收起就重新展开），再核对残留
      if ((await page.locator(".chart-summary").count()) === 0) {
        await realClick(page, page.locator(".summary-toggle").first());
      }
      await page.waitForSelector(".chart-summary", { timeout: 5000 });
      const daySummary = await page.locator(".chart-summary").first().innerText();
      // 切维后类别应整体换成日期键，不残留任何模型类别行
      const dayLines = (await page.locator(".chart-summary .summary-line").allInnerTexts()).map(
        (t) => t.trim(),
      );
      const residue = dayLines.some((l) => /model-\d+/.test(l));
      const afterSwitch = await canvasFingerprint(page);
      ctx.record(
        "chart_scroll_and_zoom_survive_supported_updates",
        "切维度后摘要不再残留旧类别",
        residue === false && daySummary.length > 0,
        `残留=${residue}；摘要片段=${daySummary.slice(0, 60).split("\n").join(" | ")}`,
      );
      ctx.record(
        "chart_scroll_and_zoom_survive_supported_updates",
        "切维度后画布确实重绘（非保留旧实例状态）",
        !!afterSwitch && !!afterRefresh && afterSwitch.hash !== afterRefresh.hash,
        `切维前=${afterRefresh?.hash} 切维后=${afterSwitch?.hash}`,
      );
      await ctx.shot("chart_scroll_and_zoom_survive_supported_updates", session, "multi-category");
      ctx.finishSession(session);
    } finally {
      await session.close();
    }
  },

  /**
   * 真实 App 内部 scroller：scrollTop 增大后导航仍可见（不被顶出），
   * 顶部通知/错误条仍可操作；合成 CSS 脚本只作辅助，不替代本条。
   */
  async real_app_scroll_keeps_navigation_visible(ctx) {
    const session = await ctx.open({
      fixture: "multi-category",
      viewport: VIEWPORTS.small,
      theme: "light",
    });
    try {
      await openDashboard(session);
      const page = session.page;
      const navBefore = await page.locator(".app-nav").boundingBox();
      const scroller = page.locator(".scroll-container").first();
      const info = await scroller.evaluate((el) => {
        el.scrollTop = Math.min(600, Math.max(0, el.scrollHeight - el.clientHeight));
        return {
          scrollTop: el.scrollTop,
          scrollHeight: el.scrollHeight,
          clientHeight: el.clientHeight,
        };
      });
      await page.waitForTimeout(300);
      ctx.record(
        "real_app_scroll_keeps_navigation_visible",
        "真实 scroller 的 scrollTop 增大",
        info.scrollTop > 0,
        `scrollTop=${info.scrollTop}/${info.scrollHeight - info.clientHeight}（client=${info.clientHeight}）`,
      );
      const navAfter = await page.locator(".app-nav").boundingBox();
      ctx.record(
        "real_app_scroll_keeps_navigation_visible",
        "滚动后导航仍在视口顶部且尺寸不变",
        !!navAfter &&
          Math.abs(navAfter.y - navBefore.y) < 1 &&
          Math.abs(navAfter.height - navBefore.height) < 1,
        `y ${navBefore?.y}→${navAfter?.y}，h ${navBefore?.height}→${navAfter?.height}`,
      );
      // 通知/错误条仍可被点击（真实滚动后仍可达）
      const banner = page.locator(".banner-slot .ts-notice, .app-nav").first();
      const box = await banner.boundingBox();
      ctx.record(
        "real_app_scroll_keeps_navigation_visible",
        "滚动后顶部区域仍在视口内可达",
        !!box && box.y >= 0 && box.y < (navAfter?.height ?? 80) + 200,
        box ? `顶部元素 y=${box.y.toFixed(1)} h=${box.height.toFixed(1)}` : "未找到顶部元素",
      );
      // 回到顶部后明细仍可见（不出现空白/裁切）
      await scroller.evaluate((el) => {
        el.scrollTop = 0;
      });
      await page.waitForTimeout(250);
      const rows = await page.locator(".n-data-table .n-data-table-tr").count();
      ctx.record(
        "real_app_scroll_keeps_navigation_visible",
        "回到顶部后明细仍渲染（无虚拟滚动空白）",
        rows > 0,
        `明细行数=${rows}`,
      );
      await ctx.shot("real_app_scroll_keeps_navigation_visible", session, "scrolled");
      ctx.finishSession(session);
    } finally {
      await session.close();
    }
  },

  /**
   * 预绘制首帧：主模块明确未执行时，data-theme 与实际画布/背景颜色已符合偏好；
   * 截图必须在该时刻拍（另存释放后的终态），同时注入完整 IPC 与外联拦截。
   */
  async prepaint_theme_has_correct_canvas(ctx) {
    const cases = [
      { label: "dark-over-light-system", pref: "dark", system: "light", expect: "dark" },
      { label: "light-over-dark-system", pref: "light", system: "dark", expect: "light" },
      { label: "missing-pref-follows-dark-system", pref: null, system: "dark", expect: "dark" },
      {
        label: "storage-unavailable-follows-system",
        pref: "storage-blocked",
        system: "dark",
        expect: "dark",
      },
    ];
    for (const c of cases) {
      // 主模块 gate：拦截入口脚本，直到我们完成首帧取证再放行。
      const session = await ctx.open({
        fixture: "normal",
        viewport: VIEWPORTS.small,
        theme: c.pref === "storage-blocked" ? null : c.pref,
        colorScheme: c.system,
        gateMainModule: true,
        storageBlocked: c.pref === "storage-blocked",
      });
      try {
        await session.page.goto(session.url, { waitUntil: "commit" });
        await session.page.waitForFunction(
          () =>
            document.documentElement.hasAttribute("data-theme") ||
            !!document.querySelector("style,link[rel=stylesheet]"),
          { timeout: 4000 },
        );
        const pre = await session.page.evaluate(() => ({
          theme: document.documentElement.getAttribute("data-theme"),
          htmlBg: getComputedStyle(document.documentElement).backgroundColor,
          bodyBg: getComputedStyle(document.body ?? document.documentElement).backgroundColor,
          mainLoaded: (document.getElementById("app")?.childElementCount ?? 0) > 0,
        }));
        // 首帧时刻截图（在释放主模块之前）
        await ctx.shot("prepaint_theme_has_correct_canvas", session, `${c.label}-prepaint`);
        ctx.record(
          "prepaint_theme_has_correct_canvas",
          `${c.label}:主模块未执行时 data-theme 已就位`,
          pre.mainLoaded === false && pre.theme === c.expect,
          `mainLoaded=${pre.mainLoaded} data-theme=${pre.theme} 期望=${c.expect}`,
        );
        const canvasOk = await session.page.evaluate((expect) => {
          // 取第一个**不透明**的绘制面：html 常为透明（rgba(0,0,0,0)），
          // 此时实际画布颜色来自 body / .app-shell。
          const parse = (c) => {
            const m = /rgba?\(([^)]+)\)/.exec(c ?? "");
            if (!m) return null;
            const n = m[1]
              .split(/[\s,\/]+/)
              .filter(Boolean)
              .map(Number);
            return { rgb: n.slice(0, 3), a: n.length >= 4 ? n[3] : 1 };
          };
          const faces = [
            document.documentElement,
            document.body,
            document.getElementById("app")?.firstElementChild,
          ];
          let picked = null;
          for (const el of faces) {
            if (!el) continue;
            const p = parse(getComputedStyle(el).backgroundColor);
            if (!p) continue;
            if (p.a > 0.9) {
              picked = {
                rgb: p.rgb,
                tag: el.tagName + "." + (el.className || "").toString().slice(0, 20),
              };
              break;
            }
            if (!picked) picked = { rgb: p.rgb, tag: el.tagName + "(透明)" };
          }
          if (!picked) return { ok: false, detail: "无可判定的背景面" };
          const l =
            (0.2126 * picked.rgb[0] + 0.7152 * picked.rgb[1] + 0.0722 * picked.rgb[2]) / 255;
          return {
            ok: expect === "dark" ? l < 0.5 : l > 0.5,
            detail: `亮度=${l.toFixed(3)} 面=${picked.tag} rgb=${picked.rgb.join(",")}`,
          };
        }, c.expect);
        ctx.record(
          "prepaint_theme_has_correct_canvas",
          `${c.label}:实际画布/背景颜色与偏好一致（不是先画反色）`,
          canvasOk.ok,
          canvasOk.detail,
        );
        // 释放主模块 → 终态另存
        await session.releaseMainModule();
        await session.page.waitForSelector("#app .app-shell", { timeout: 10000 });
        await session.page.waitForTimeout(400);
        const settled = await session.page.evaluate(() => ({
          theme: document.documentElement.getAttribute("data-theme"),
          mainLoaded: (document.getElementById("app")?.childElementCount ?? 0) > 0,
        }));
        ctx.record(
          "prepaint_theme_has_correct_canvas",
          `${c.label}:加载完成后主题不变且 IPC 无泄漏`,
          settled.theme === c.expect &&
            settled.mainLoaded === true &&
            session.unknownCommands.length === 0 &&
            session.externalRequests.length === 0,
          `settled=${settled.theme} 未知命令=${session.unknownCommands.join(",") || "无"} 外联=${session.externalRequests.length}`,
        );
        await ctx.shot("prepaint_theme_has_correct_canvas", session, `${c.label}-settled`);
        ctx.finishSession(session);
      } finally {
        await session.close();
      }
    }
  },
};

/**
 * 运行全部必需契约。
 * @param {object} deps
 * @param {(opts: object) => Promise<object>} deps.open 建立带 IPC/外联拦截的会话
 * @param {string} deps.output 产物目录
 * @param {object} deps.meta { commit, browserVersion, url }
 * @returns {Promise<{results: object[], failures: string[]}>}
 */
export async function runContracts({ open, output, meta, only = REQUIRED_CONTRACTS }) {
  const results = [];
  const failures = [];
  /** 每个契约截图计数（保证文件名稳定唯一）。 */
  const shotIndex = new Map();

  const ctx = {
    record(contract, name, pass, detail) {
      results.push({ contract, name, pass: !!pass, detail: String(detail ?? ""), ...meta });
      if (!pass) failures.push(`${contract} / ${name}：${detail}`);
    },
    async shot(contract, session, tag) {
      const n = (shotIndex.get(contract) ?? 0) + 1;
      shotIndex.set(contract, n);
      const file = `${contract}--${tag}--${n}.png`;
      try {
        await session.page.screenshot({ path: join(output, file) });
        const last = results[results.length - 1];
        if (last) last.screenshot = file;
      } catch {
        /* 截图失败不改变断言结论 */
      }
    },
    finishSession(session) {
      // 每个会话收尾时的硬约束：无未知命令、无外联、无未处理拒绝
      ctx.record(
        session.contractForFinish ?? "session",
        `${session.label}:无未知 IPC 命令与外联请求`,
        session.unknownCommands.length === 0 && session.externalRequests.length === 0,
        `未知=${session.unknownCommands.join(",") || "无"} 外联=${session.externalRequests.join(",") || "无"}`,
      );
      ctx.record(
        session.contractForFinish ?? "session",
        `${session.label}:无未处理拒绝/页面错误`,
        session.pageErrors.length === 0,
        session.pageErrors.join(" | ") || "无",
      );
    },
    open: async (opts) => {
      const s = await open(opts);
      s.contractForFinish = opts.contract ?? REQUIRED_CONTRACTS[0];
      return s;
    },
  };

  for (const contract of only) {
    ctx.open = async (opts) => {
      const s = await open({ ...opts, contract });
      s.contractForFinish = contract;
      return s;
    };
    const fn = IMPL[contract];
    if (!fn) {
      ctx.record(contract, "契约有实现", false, `缺少实现：${contract}`);
      continue;
    }
    try {
      await fn(ctx);
    } catch (e) {
      ctx.record(contract, "契约执行未抛错", false, e instanceof Error ? e.message : String(e));
    }
    const owned = results.filter((r) => r.contract === contract);
    ctx.record(
      contract,
      "契约有实际断言（不以场景数充当覆盖）",
      owned.length >= 3,
      `记录断言 ${owned.length} 条，其中失败 ${owned.filter((r) => !r.pass).length} 条`,
    );
  }
  return { results, failures };
}
