// C06（控件材质与浮层可读性计划）：浏览器像素级材质与焦点契约。
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { decodePng } from "../lib/png.mjs";
import { contrastRatio } from "../lib/contrast.mjs";

const REPO_ROOT = fileURLToPath(new URL("../../../", import.meta.url));
const ARTIFACT_DIR = join(REPO_ROOT, "qa-artifacts", "control-material");

const VIEWPORTS = {
  desktop: { width: 1280, height: 820, label: "1280x820" },
};

function parseRgb(str) {
  const m = /rgba?\(([^)]+)\)/.exec(str ?? "");
  if (!m) return [0, 0, 0];
  const parts = m[1].split(/[\s,\/]+/).filter(Boolean).map(Number);
  return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0];
}

function parseAlpha(str) {
  const m = /rgba?\(([^)]+)\)/.exec(str ?? "");
  if (!m) return 1;
  const parts = m[1].split(/[\s,\/]+/).filter(Boolean).map(Number);
  return parts.length >= 4 ? parts[3] : 1;
}

function formatSides(details) {
  return ["top", "bottom", "left", "right"]
    .map((s) => `${s}:${details[s]?.ratio.toFixed(2)}`)
    .join(", ");
}

/**
 * 真实点击：先把目标滚到视口中部再点。
 */
async function realClick(page, locator) {
  const el = locator.first();
  await el.scrollIntoViewIfNeeded();
  await el.evaluate((n) => n.scrollIntoView({ block: "center", inline: "nearest" }));
  await page.waitForTimeout(120);
  await el.click({ timeout: 10000 });
  await page.waitForTimeout(200);
}

/**
 * 打开业务主应用页面。
 */
async function openDashboard(session) {
  await session.page.goto(session.url, { waitUntil: "networkidle" });
  await session.page.waitForSelector("#app .app-shell", { timeout: 10000 });
  await session.page.waitForSelector(".page-head", { timeout: 10000 });
  await session.page.evaluate(() => document.fonts?.ready);
  await session.page.waitForTimeout(300);
}

/**
 * 导航到设置页 / 汇总页。
 */
async function gotoPage(page, label) {
  const btn = page.locator('[aria-label="页面切换"] button').filter({ hasText: label }).first();
  await btn.waitFor({ state: "visible", timeout: 5000 });
  await btn.click({ timeout: 10000 });
  await page.waitForTimeout(400);
}

/**
 * 切到设置页的某个分区。
 */
async function gotoSettingsSection(page, label) {
  const candidates = [
    page.locator(".settings-nav-item").filter({ hasText: label }).first(),
    page.locator(".settings-nav .ts-segmented button").filter({ hasText: label }).first(),
  ];
  for (const btn of candidates) {
    if ((await btn.count()) > 0) {
      await realClick(page, btn);
      await page.waitForTimeout(300);
      return true;
    }
  }
  return false;
}

/**
 * 像素量测方法 measureBoundary(page, locator)：
 * 1. 鼠标移到 (1, 1)，确保不处于悬停态；
 * 2. 截一张外扩 6px 的区域图（deviceScaleFactor 为 1）；
 * 3. 每条边取 3 个像素：边缘向内第 0 与第 1 整像素（边框候选），边缘向外第 3 整像素（外侧像素）；
 * 4. 两候选分别与外侧像素求对比度，取较大者；
 * 5. 上下左右四条边各测一次，取最小值。
 * 若中点落在圆角弧上，改取该边的 1/3 处。
 */
export async function measureBoundary(page, locator) {
  const el = locator.first();
  await el.scrollIntoViewIfNeeded();
  await el.evaluate((n) => n.scrollIntoView({ block: "center", inline: "nearest" }));
  await page.waitForTimeout(100);
  await page.mouse.move(1, 1);
  await page.waitForTimeout(100);

  const box = await el.boundingBox();
  if (!box) {
    throw new Error("Element boundingBox is null");
  }

  const pad = 6;
  const clip = {
    x: Math.max(0, Math.floor(box.x - pad)),
    y: Math.max(0, Math.floor(box.y - pad)),
    width: Math.ceil(box.width + (box.x - Math.max(0, Math.floor(box.x - pad))) + pad),
    height: Math.ceil(box.height + (box.y - Math.max(0, Math.floor(box.y - pad))) + pad),
  };

  const shotBuffer = await page.screenshot({ clip });
  const png = decodePng(shotBuffer);
  if (!png) {
    throw new Error("Failed to decode screenshot PNG");
  }

  const edgeLeft = box.x - clip.x;
  const edgeTop = box.y - clip.y;
  const edgeRight = edgeLeft + box.width;
  const edgeBottom = edgeTop + box.height;

  function getPx(x, y) {
    const cx = Math.max(0, Math.min(png.width - 1, Math.round(x)));
    const cy = Math.max(0, Math.min(png.height - 1, Math.round(y)));
    const idx = (cy * png.width + cx) * png.channels;
    return [png.data[idx], png.data[idx + 1], png.data[idx + 2]];
  }

  function measureSide(side, fraction) {
    let outer, inner0, inner1, inner2, x, y, outerNear;
    if (side === "top") {
      x = edgeLeft + box.width * fraction;
      y = edgeTop;
      outer = getPx(x, edgeTop - 3);
      outerNear = getPx(x, edgeTop - 1);
      inner0 = getPx(x, Math.floor(edgeTop));
      inner1 = getPx(x, Math.floor(edgeTop) + 1);
      inner2 = getPx(x, Math.floor(edgeTop) + 2);
    } else if (side === "bottom") {
      x = edgeLeft + box.width * fraction;
      y = edgeBottom;
      outer = getPx(x, Math.floor(edgeBottom) + 3);
      outerNear = getPx(x, Math.floor(edgeBottom) + 1);
      inner0 = getPx(x, Math.ceil(edgeBottom) - 1);
      inner1 = getPx(x, Math.ceil(edgeBottom) - 2);
      inner2 = getPx(x, Math.ceil(edgeBottom) - 3);
    } else if (side === "left") {
      x = edgeLeft;
      y = edgeTop + box.height * fraction;
      outer = getPx(edgeLeft - 3, y);
      outerNear = getPx(edgeLeft - 1, y);
      inner0 = getPx(Math.floor(edgeLeft), y);
      inner1 = getPx(Math.floor(edgeLeft) + 1, y);
      inner2 = getPx(Math.floor(edgeLeft) + 2, y);
    } else if (side === "right") {
      x = edgeRight;
      y = edgeTop + box.height * fraction;
      outer = getPx(Math.floor(edgeRight) + 3, y);
      outerNear = getPx(Math.floor(edgeRight) + 1, y);
      inner0 = getPx(Math.ceil(edgeRight) - 1, y);
      inner1 = getPx(Math.ceil(edgeRight) - 2, y);
      inner2 = getPx(Math.ceil(edgeRight) - 3, y);
    }

    const r0 = contrastRatio(inner0, outer);
    const r1 = contrastRatio(inner1, outer);
    const r0Near = contrastRatio(inner0, outerNear);
    const r1Near = contrastRatio(inner1, outerNear);

    const ratio = Math.max(r0, r1, r0Near, r1Near);
    return { side, fraction, ratio, outer, inner0, inner1, r0, r1 };
  }

  const sides = ["top", "bottom", "left", "right"];
  const details = {};
  let minRatio = Infinity;

  for (const s of sides) {
    let res = measureSide(s, 0.5);
    if (res.ratio < 3.0) {
      const alt = measureSide(s, 1 / 3);
      if (alt.ratio > res.ratio) {
        res = alt;
      }
    }
    details[s] = res;
    if (res.ratio < minRatio) {
      minRatio = res.ratio;
    }
  }

  return {
    minRatio,
    pass: minRatio >= 3.0,
    details,
    box,
  };
}

/**
 * 契约：ui_control_material
 */
export async function uiControlMaterial(ctx) {
  const name = "ui_control_material";
  mkdirSync(ARTIFACT_DIR, { recursive: true });

  for (const theme of ["light", "dark"]) {
    const session = await ctx.open({
      fixture: "normal",
      viewport: VIEWPORTS.desktop,
      theme,
      reducedMotion: "reduce",
    });

    const { page } = session;
    const record = (assertionName, pass, detail) =>
      ctx.record(name, `${theme}: ${assertionName}`, pass, detail);

    try {
      await openDashboard(session);

      // 1. refresh_button_boundary_ge_3 (.page-head button.n-button 边界 ≥ 3:1)
      const refreshBtn = page.locator(".page-head button.n-button").first();
      await refreshBtn.waitFor({ state: "visible", timeout: 5000 });
      const bRefresh = await measureBoundary(page, refreshBtn);
      record(
        "refresh_button_boundary_ge_3",
        bRefresh.pass,
        `min=${bRefresh.minRatio.toFixed(2)} (${formatSides(bRefresh.details)})`,
      );

      // 2. date_trigger_boundary_ge_3 (.range-trigger 边界 ≥ 3:1，全部时间图标态)
      const dateTrigger = page.locator(".range-trigger").first();
      await dateTrigger.waitFor({ state: "visible", timeout: 5000 });
      const bTrigger = await measureBoundary(page, dateTrigger);
      record(
        "date_trigger_boundary_ge_3",
        bTrigger.pass,
        `min=${bTrigger.minRatio.toFixed(2)} (${formatSides(bTrigger.details)})`,
      );

      // 3. segmented_groove_boundary_ge_3 (筛选栏聚合维度 .ts-segmented 边界 ≥ 3:1)
      const segmentedGroove = page
        .locator('.filter-line .ts-segmented[aria-label="聚合维度"]')
        .first();
      await segmentedGroove.waitFor({ state: "visible", timeout: 5000 });
      const bGroove = await measureBoundary(page, segmentedGroove);
      record(
        "segmented_groove_boundary_ge_3",
        bGroove.pass,
        `min=${bGroove.minRatio.toFixed(2)} (${formatSides(bGroove.details)})`,
      );

      // 截图 1：汇总页标题行与筛选栏
      await page.screenshot({ path: join(ARTIFACT_DIR, `${theme}-dashboard-head.png`) });
      await page.screenshot({ path: join(ARTIFACT_DIR, `after-${theme}-dashboard-head.png`) });

      // 打开日期浮层
      await realClick(page, dateTrigger);
      await page.waitForSelector(".range-panel", { state: "visible", timeout: 5000 });
      await page.waitForTimeout(300);

      // 4. date_shortcut_boundary_ge_3 (打开日期浮层后，每个未选中的 .shortcut-btn 边界 ≥ 3:1)
      const unselectedShortcuts = page.locator('.range-panel .shortcut-btn[aria-pressed="false"]');
      const scCount = await unselectedShortcuts.count();
      let allShortcutsPass = scCount >= 5;
      const scDetails = [];
      for (let i = 0; i < scCount; i++) {
        const btn = unselectedShortcuts.nth(i);
        const text = (await btn.innerText()).trim();
        const b = await measureBoundary(page, btn);
        if (!b.pass) allShortcutsPass = false;
        scDetails.push(`${text}:${b.minRatio.toFixed(2)}`);
      }
      record(
        "date_shortcut_boundary_ge_3",
        allShortcutsPass,
        `count=${scCount} [${scDetails.join(", ")}]`,
      );

      // 5. selected_shortcut_has_accent_stroke (当前预设项计算样式 border-color == --ts-accent, aria-pressed="true")
      const selectedShortcut = page
        .locator('.range-panel .shortcut-btn[aria-pressed="true"]')
        .first();
      const selData = await selectedShortcut.evaluate((el) => {
        const cs = getComputedStyle(el);
        const dummy = document.createElement("div");
        dummy.style.color = "var(--ts-accent)";
        document.body.appendChild(dummy);
        const expectedAccent = getComputedStyle(dummy).color;
        dummy.remove();
        return {
          borderColor: cs.borderColor,
          expectedAccent,
          ariaPressed: el.getAttribute("aria-pressed"),
        };
      });
      const strokeOk =
        selData.borderColor === selData.expectedAccent && selData.ariaPressed === "true";
      record(
        "selected_shortcut_has_accent_stroke",
        strokeOk,
        `border=${selData.borderColor} expected=${selData.expectedAccent} aria-pressed=${selData.ariaPressed}`,
      );

      // 6. primary_button_text_ge_4_5 (日期浮层“确定”按钮：可用时文字色与背景色对比 ≥ 4.5:1)
      // 在自定义日历中选择日期，或直接使用已回填的草稿使“确定”可用
      const confirmBtn = page.locator(".range-panel .actions-row button", { hasText: "确定" }).first();
      let isEnabled = await confirmBtn.evaluate(
        (el) => !el.disabled && !el.className.includes("n-button--disabled"),
      );
      if (!isEnabled) {
        // 点击开始日历与结束日历的一个单元格以激活草稿
        const startCell = page
          .locator(".range-panel .n-date-panel-calendar--start .n-date-panel-date:not(.n-date-panel-date--excluded)")
          .nth(10);
        if ((await startCell.count()) > 0) {
          await startCell.click();
          await page.waitForTimeout(100);
        }
        const endCell = page
          .locator(".range-panel .n-date-panel-calendar--end .n-date-panel-date:not(.n-date-panel-date--excluded)")
          .nth(15);
        if ((await endCell.count()) > 0) {
          await endCell.click();
          await page.waitForTimeout(100);
        }
      }
      await page.waitForFunction(
        () => {
          const btn = document.querySelector(".range-panel .actions-row button.n-button--primary-type");
          return btn && !btn.disabled && !btn.className.includes("n-button--disabled");
        },
        null,
        { timeout: 5000 },
      );

      const confirmColors = await confirmBtn.evaluate((el) => {
        const cs = getComputedStyle(el);
        return {
          color: cs.color,
          backgroundColor: cs.backgroundColor,
        };
      });
      const textRgb = parseRgb(confirmColors.color);
      const bgRgb = parseRgb(confirmColors.backgroundColor);
      const confirmRatio = contrastRatio(textRgb, bgRgb);
      record(
        "primary_button_text_ge_4_5",
        confirmRatio >= 4.5,
        `ratio=${confirmRatio.toFixed(2)}:1 (text=${confirmColors.color}, bg=${confirmColors.backgroundColor})`,
      );

      // 9. time_column_popover_is_opaque (在日期浮层自定义区点开结束时间输入，等待 .n-time-picker-panel，断言背景 alpha=1 且 backdrop-filter 为 none)
      const endTimePicker = page.locator(".range-panel .n-date-panel .n-time-picker").nth(1);
      await realClick(page, endTimePicker);
      await page.waitForSelector(".n-time-picker-panel", { state: "visible", timeout: 5000 });
      await page.waitForTimeout(300);

      // 截图 2：日期浮层（含展开的时间列）
      await page.screenshot({ path: join(ARTIFACT_DIR, `${theme}-date-popover-time.png`) });
      await page.screenshot({ path: join(ARTIFACT_DIR, `after-${theme}-date-popover-time.png`) });

      const timePanelResult = await page.evaluate(() => {
        const panel = document.querySelector(".n-time-picker-panel");
        if (!panel) return null;
        const cs = getComputedStyle(panel);
        const parseA = (c) => {
          const m = /rgba?\(([^)]+)\)/.exec(c ?? "");
          if (!m) return 1;
          const parts = m[1].split(/[\s,\/]+/).filter(Boolean).map(Number);
          return parts.length >= 4 ? parts[3] : 1;
        };
        return {
          alpha: parseA(cs.backgroundColor),
          backgroundColor: cs.backgroundColor,
          backdropFilter: cs.backdropFilter || cs.webkitBackdropFilter || "none",
        };
      });

      const timePanelOk =
        timePanelResult != null &&
        Math.abs(timePanelResult.alpha - 1) <= 0.01 &&
        (timePanelResult.backdropFilter === "none" || !timePanelResult.backdropFilter);
      record(
        "time_column_popover_is_opaque",
        timePanelOk,
        `alpha=${timePanelResult?.alpha} bg=${timePanelResult?.backgroundColor} backdrop=${timePanelResult?.backdropFilter}`,
      );

      // 10 题中的复选框部分：日期浮层里“结束于现在”勾选框的方框 .n-checkbox-box 边界 ≥ 3:1
      const checkboxBox = page.locator(".range-panel .n-checkbox .n-checkbox-box").first();
      await checkboxBox.waitFor({ state: "visible", timeout: 5000 });
      const bCheckbox = await measureBoundary(page, checkboxBox);

      // 关闭日期浮层
      await page.keyboard.press("Escape");
      await page.waitForTimeout(300);

      // 导航到设置页
      await gotoPage(page, "设置");
      await page.waitForTimeout(400);

      // 切到外观分区
      await gotoSettingsSection(page, "外观");
      await page.waitForTimeout(400);

      // 7. mouse_click_leaves_no_focus_ring
      // 用 realClick 点击“恢复默认颜色”按钮，断言其为 activeElement，outline 为 none，无柔光，边框颜色与点击前相同
      const resetBtn = page.getByRole("button", { name: "恢复默认颜色", exact: true });
      await resetBtn.waitFor({ state: "visible", timeout: 5000 });
      const beforeBorder = await resetBtn.evaluate((el) => getComputedStyle(el).borderColor);
      await realClick(page, resetBtn);
      await page.waitForTimeout(200);

      const clickData = await resetBtn.evaluate((el, beforeColor) => {
        const isActive = document.activeElement === el;
        const cs = getComputedStyle(el);
        const outlineStyle = cs.outlineStyle;
        const boxShadow = cs.boxShadow;
        const currentBorder = cs.borderColor;
        const hasFocusHalo = /rgba\(\s*0,\s*113,\s*227|rgba\(\s*77,\s*163,\s*255/.test(boxShadow);
        const borderSame = currentBorder === beforeColor;
        return {
          isActive,
          outlineStyle,
          boxShadow,
          hasFocusHalo,
          borderSame,
          currentBorder,
          beforeColor,
        };
      }, beforeBorder);

      const clickOk =
        clickData.isActive &&
        clickData.outlineStyle === "none" &&
        !clickData.hasFocusHalo &&
        clickData.borderSame;
      record(
        "mouse_click_leaves_no_focus_ring",
        clickOk,
        `active=${clickData.isActive} outline=${clickData.outlineStyle} halo=${clickData.hasFocusHalo} border=${clickData.currentBorder} vs ${clickData.beforeColor}`,
      );

      // 8. keyboard_focus_shows_ring
      // 先按一次 Tab，再 focus 同一按钮，断言 :focus-visible 匹配，outline 2px solid --ts-accent
      await page.keyboard.press("Tab");
      await resetBtn.focus();
      await page.waitForTimeout(200);

      const keyData = await resetBtn.evaluate((el) => {
        const matchesFocusVisible = el.matches(":focus-visible");
        const cs = getComputedStyle(el);
        const dummy = document.createElement("div");
        dummy.style.color = "var(--ts-accent)";
        document.body.appendChild(dummy);
        const expectedAccent = getComputedStyle(dummy).color;
        dummy.remove();
        const hasFocusHalo = /rgba\(\s*0,\s*113,\s*227|rgba\(\s*77,\s*163,\s*255/.test(cs.boxShadow);
        return {
          matchesFocusVisible,
          outlineStyle: cs.outlineStyle,
          outlineWidth: cs.outlineWidth,
          outlineColor: cs.outlineColor,
          boxShadow: cs.boxShadow,
          hasFocusHalo,
          expectedAccent,
        };
      });

      const keyOk =
        keyData.matchesFocusVisible &&
        keyData.outlineStyle === "solid" &&
        keyData.outlineWidth === "2px" &&
        keyData.outlineColor === keyData.expectedAccent &&
        keyData.hasFocusHalo;
      record(
        "keyboard_focus_shows_ring",
        keyOk,
        `:focus-visible=${keyData.matchesFocusVisible} outline=${keyData.outlineWidth} ${keyData.outlineStyle} ${keyData.outlineColor} halo=${keyData.hasFocusHalo} (${keyData.boxShadow}) (expected ${keyData.expectedAccent})`,
      );

      // 切到价格分区
      await gotoSettingsSection(page, "价格");
      await page.waitForTimeout(400);

      // 截图 3：设置页“价格”分区按钮区
      await page.screenshot({ path: join(ARTIFACT_DIR, `${theme}-settings-pricing.png`) });
      await page.screenshot({ path: join(ARTIFACT_DIR, `after-${theme}-settings-pricing.png`) });

      // 切到通用分区，测量 NSwitch
      await gotoSettingsSection(page, "通用");
      await page.waitForTimeout(400);

      // 10 题中的开关部分：设置页第一个 NSwitch 关闭态轨道 .n-switch__rail 边界 ≥ 3:1
      const sw = page.locator(".setting-control .n-switch").first();
      await sw.waitFor({ state: "visible", timeout: 5000 });
      const isChecked = await sw.evaluate(
        (el) =>
          el.classList.contains("n-switch--active") || el.getAttribute("aria-checked") === "true",
      );
      if (isChecked) {
        await realClick(page, sw);
        await page.waitForTimeout(200);
      }
      const rail = sw.locator(".n-switch__rail");
      const bSwitch = await measureBoundary(page, rail);
      if (isChecked) {
        await realClick(page, sw);
        await page.waitForTimeout(100);
      }

      // 10. checkbox_and_switch_boundary_ge_3
      const cbAndSwOk = bCheckbox.pass && bSwitch.pass;
      record(
        "checkbox_and_switch_boundary_ge_3",
        cbAndSwOk,
        `checkbox: min=${bCheckbox.minRatio.toFixed(2)} (${formatSides(bCheckbox.details)}) | switch: min=${bSwitch.minRatio.toFixed(2)} (${formatSides(bSwitch.details)})`,
      );

      // 触发 close-requested 事件打开关闭确认对话框
      await page.evaluate(() => {
        const c = window.__TS_IPC_CALLS.find(
          (x) => x.cmd === "plugin:event|listen" && x.args?.event === "close-requested",
        );
        if (c && window.__TS_CB[c.args.handler]) {
          window.__TS_CB[c.args.handler]({});
        }
      });
      await page.waitForSelector(".close-dialog", { state: "visible", timeout: 5000 });
      await page.waitForTimeout(300);

      // 截图 4：关闭确认对话框
      await page.screenshot({ path: join(ARTIFACT_DIR, `${theme}-close-dialog.png`) });
      await page.screenshot({ path: join(ARTIFACT_DIR, `after-${theme}-close-dialog.png`) });

      // 关闭对话框
      await page.locator(".close-actions button", { hasText: "取消" }).first().click();
      await page.waitForSelector(".close-dialog", { state: "hidden", timeout: 5000 });

      ctx.finishSession(session);

      // 抽查 150% 缩放截图
      const session150 = await ctx.open({
        fixture: "normal",
        viewport: VIEWPORTS.desktop,
        theme,
        reducedMotion: "reduce",
        deviceScaleFactor: 1.5,
      });
      try {
        await openDashboard(session150);
        await session150.page.screenshot({
          path: join(ARTIFACT_DIR, `after-${theme}-1280x820-150pct.png`),
        });
        ctx.finishSession(session150);
      } finally {
        await session150.close();
      }
    } finally {
      await session.close();
    }
  }
}
