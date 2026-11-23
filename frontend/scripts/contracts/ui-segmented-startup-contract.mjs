// 冷启动首帧按真实按钮边界量测；预存 all/model，首个查询挂起，不以数据到达
// 或 setTimeout 后的终态代替首次可见帧。只用真实点击/键盘操作产品。
const CONTRACT = "segmented_cold_start_highlight";

function startFrameProbe() {
  localStorage.setItem(
    "tokenscope-view-filters",
    JSON.stringify({
      agent: "all",
      by: "model",
      preset: null,
      rollupsMode: "auto",
      drill: null,
    }),
  );
  window.__segmentFrames = [];
  window.__segmentProbe = true;
  window.__segmentIds = new WeakMap();
  let nextId = 0;
  const rect = (r) => ({ x: r.left, y: r.top, width: r.width, height: r.height });
  const collect = () => {
    const groups = [...document.querySelectorAll(".ts-segmented")].flatMap((group) => {
      const box = group.getBoundingClientRect();
      const button = group.querySelector('.ts-segmented-item[aria-checked="true"]');
      if (!button || box.width <= 0 || box.height <= 0) return [];
      if (!window.__segmentIds.has(group)) window.__segmentIds.set(group, ++nextId);
      const buttonRect = button.getBoundingClientRect();
      const thumb = group.querySelector(".ts-segmented-thumb");
      const thumbCss = thumb ? getComputedStyle(thumb) : null;
      const thumbRect = thumb ? thumb.getBoundingClientRect() : null;
      const fallback = getComputedStyle(button, "::before");
      const fallbackVisible =
        !["none", "normal"].includes(fallback.content) &&
        fallback.display !== "none" &&
        fallback.visibility !== "hidden" &&
        Number(fallback.opacity) > 0 &&
        parseFloat(fallback.borderLeftWidth) > 0;
      const thumbVisible =
        thumbCss &&
        thumbCss.display !== "none" &&
        thumbCss.visibility !== "hidden" &&
        Number(thumbCss.opacity) > 0 &&
        thumbRect.width > 0 &&
        parseFloat(thumbCss.borderLeftWidth) > 0;
      const layers = [];
      if (fallbackVisible)
        layers.push({
          kind: "button",
          x: buttonRect.left + button.clientLeft + parseFloat(fallback.left),
          y: buttonRect.top + button.clientTop + parseFloat(fallback.top),
          width: parseFloat(fallback.width),
          height: parseFloat(fallback.height),
        });
      if (thumbVisible) layers.push({ kind: "thumb", ...rect(thumbRect) });
      return [
        {
          id: window.__segmentIds.get(group),
          label: group.getAttribute("aria-label"),
          selected: button.getAttribute("aria-label"),
          button: rect(buttonRect),
          layers,
          duration: thumbCss?.transitionDuration,
          focus: document.activeElement === button,
        },
      ];
    });
    return groups;
  };
  /** 在等待结束时同步读一次真实终态：rAF 最后一帧可能早于合成器落定不足 1px。 */
  window.__segmentSampleNow = () => {
    const groups = collect();
    if (groups.length) window.__segmentFrames.push({ at: performance.now(), groups });
    return groups.length;
  };
  /**
   * 等待指定分段的滑动层与选中按钮真实对齐（上限 timeout ms）。
   * CI 负载下 CSS 过渡可能比 200ms 更晚才开始/结束；只等待、不改阈值——
   * 超时仍返回 false，由原有断言失败。中间帧已由持续采样保留。
   */
  window.__segmentWaitAligned = (id, timeout) =>
    new Promise((resolve) => {
      const started = performance.now();
      const tick = () => {
        const g = collect().find((x) => x.id === id);
        const layer = g?.layers.length === 1 ? g.layers[0] : null;
        if (
          layer &&
          ["x", "y", "width", "height"].every((key) => Math.abs(layer[key] - g.button[key]) <= 0.25)
        ) {
          resolve(true);
          return;
        }
        if (performance.now() - started > timeout) {
          resolve(false);
          return;
        }
        requestAnimationFrame(tick);
      };
      tick();
    });
  const sample = () => {
    if (!window.__segmentProbe) return;
    window.__segmentSampleNow();
    requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
}

async function frames(page, reset = false) {
  return page.evaluate((clear) => {
    const result = window.__segmentFrames ?? [];
    if (clear) window.__segmentFrames = [];
    return result;
  }, reset);
}

const samples = (data, id) => data.flatMap((frame) => frame.groups.filter((g) => g.id === id));
const aligned = (sample) =>
  sample.layers.length === 1 &&
  ["x", "y", "width", "height"].every(
    (key) =>
      Number.isFinite(sample.layers[0][key]) &&
      Math.abs(sample.layers[0][key] - sample.button[key]) <= 0.25,
  );

function recordAligned(ctx, name, data, ids) {
  const all = data.flatMap((f) => f.groups.filter((g) => ids.includes(g.id)));
  ctx.record(
    CONTRACT,
    name,
    ids.every((id) => Number.isInteger(id) && all.some((g) => g.id === id)) && all.every(aligned),
    JSON.stringify({
      samples: all.length,
      first: all[0],
      failures: all.filter((g) => !aligned(g)).slice(0, 4),
    }),
  );
}

async function switchSample(ctx, session, locator, id, key, reduced, label) {
  const page = session.page;
  await frames(page, true);
  await page.waitForTimeout(50);
  if (key) {
    await locator.focus();
    await page.keyboard.press(key);
  } else await locator.click();
  // 0.2s 动画已结束：等待滑动层与按钮真实对齐（上限 2s），再同步采样终态。
  // CI 负载下过渡可能晚开始；只等待不改 0.25px，超时仍由下方断言失败。
  // 等待前后的持续 rAF 采样仍保留中间帧证据。
  await page.evaluate((segment) => window.__segmentWaitAligned?.(segment, 2000), id);
  await page.evaluate(() => window.__segmentSampleNow?.());
  const data = samples(await frames(page), id);
  const first = data[0];
  const last = data.at(-1);
  const middle = data.filter(
    (g) =>
      g.layers.length === 1 &&
      first?.layers.length === 1 &&
      Math.abs(g.layers[0].x - first.layers[0].x) > 0.25 &&
      Math.abs(g.layers[0].x - last.button.x) > 0.25,
  );
  ctx.record(
    CONTRACT,
    label,
    !!last &&
      aligned(last) &&
      data.every((g) => g.layers.length === 1) &&
      (reduced ? data.every(aligned) : middle.length >= 2),
    JSON.stringify({ samples: data.length, intermediate: middle.length, first, last }),
  );
  return data;
}

export async function uiSegmentedStartup(ctx) {
  const evidence = [];
  try {
    for (const theme of ["light", "dark"])
      for (const viewport of [
        { width: 1280, height: 820, label: "1280x820" },
        { width: 980, height: 620, label: "980x620" },
      ])
        for (const reduced of [false, true]) {
          const session = await ctx.open({
            fixture: "normal",
            theme,
            viewport,
            reducedMotion: reduced ? "reduce" : "no-preference",
          });
          let release;
          const blocked = new Promise((resolve) => {
            release = resolve;
          });
          const original = session.fixture.ipc.bind(session.fixture);
          let firstQuery = false;
          session.fixture.ipc = (cmd, args) => {
            if (cmd === "query_begin" && !firstQuery) {
              firstQuery = true;
              return blocked.then(() => original(cmd, args));
            }
            return original(cmd, args);
          };
          try {
            const page = session.page;
            await session.context.addInitScript(startFrameProbe);
            await page.goto(session.url, { waitUntil: "networkidle" });
            const source = page.getByRole("radiogroup", { name: "数据来源", exact: true });
            const dimension = page.getByRole("radiogroup", { name: "聚合维度", exact: true });
            await source.waitFor({ state: "visible" });
            await dimension.waitFor({ state: "visible" });
            await page.waitForTimeout(350);
            const cold = await frames(page, true);
            const sourceId = await source.evaluate((el) => window.__segmentIds.get(el));
            const dimId = await dimension.evaluate((el) => window.__segmentIds.get(el));
            const nav = page.getByRole("radiogroup", { name: "页面切换", exact: true });
            const themeGroup = page.getByRole("radiogroup", {
              name: "主题偏好（浅色/深色/跟随系统）",
              exact: true,
            });
            const navId = await nav.evaluate((el) => window.__segmentIds.get(el));
            const themeId = await themeGroup.evaluate((el) => window.__segmentIds.get(el));
            const tag = `${theme}/${viewport.label}/${reduced ? "reduce" : "motion"}`;
            const entry = { tag, cold };
            evidence.push(entry);
            const pending = session.ipcCalls.find((c) => c.cmd === "query_begin");
            ctx.record(
              CONTRACT,
              `${tag} 首次查询确实挂起且恢复 all/model`,
              firstQuery &&
                !pending?.outcome &&
                pending?.args.by === "model" &&
                (await source
                  .getByRole("radio", { name: "全部", exact: true })
                  .getAttribute("aria-checked")) === "true" &&
                (await dimension
                  .getByRole("radio", { name: "按模型", exact: true })
                  .getAttribute("aria-checked")) === "true",
              JSON.stringify(pending),
            );
            for (const [id, label] of [
              [sourceId, "全部"],
              [dimId, "按模型"],
              [navId, "导航"],
              [themeId, "主题"],
            ]) {
              const rows = samples(cold, id);
              ctx.record(
                CONTRACT,
                `${tag} ${label} 首帧完整高亮`,
                rows.length >= 2 && aligned(rows[0]),
                JSON.stringify(rows[0]),
              );
            }
            recordAligned(ctx, `${tag} 冷启动逐帧无细线错位或双重高亮`, cold, [
              sourceId,
              dimId,
              navId,
              themeId,
            ]);
            await ctx.shot(CONTRACT, session, `${tag.replaceAll("/", "-")}-query-pending`);
            release();
            await page.waitForSelector(".usage-card");
            await page.waitForTimeout(400);
            const mouse = await switchSample(
              ctx,
              session,
              dimension.getByRole("radio", { name: "按项目", exact: true }),
              dimId,
              null,
              reduced,
              `${tag} 鼠标切换${reduced ? "直接到位" : "保留滑动中间帧"}`,
            );
            const keyboard = await switchSample(
              ctx,
              session,
              dimension.getByRole("radio", { name: "按项目", exact: true }),
              dimId,
              "ArrowLeft",
              reduced,
              `${tag} 方向键切换${reduced ? "直接到位" : "保留滑动中间帧"}`,
            );
            ctx.record(
              CONTRACT,
              `${tag} 方向键更新焦点与无障碍状态`,
              await dimension
                .getByRole("radio", { name: "按模型", exact: true })
                .evaluate(
                  (el) =>
                    document.activeElement === el &&
                    el.getAttribute("aria-checked") === "true" &&
                    el.tabIndex === 0,
                ),
              "ArrowLeft → 按模型",
            );

            // 共享导航与主题实例必须沿同一接管/切换规则工作。
            const navigation = await switchSample(
              ctx,
              session,
              nav.getByRole("radio", { name: "设置", exact: true }),
              navId,
              null,
              reduced,
              `${tag} 导航切换保持单层高亮与过渡`,
            );
            const selectedTheme = themeGroup.locator('[aria-checked="true"]');
            const themeSwitch = await switchSample(
              ctx,
              session,
              selectedTheme,
              themeId,
              "ArrowRight",
              reduced,
              `${tag} 主题方向键切换保持单层高亮与过渡`,
            );

            // 设置页语言切换改变共享导航文案宽度，汇总筛选隐藏后再恢复。
            await frames(page, true);
            await page
              .locator(".settings-nav-item, .settings-nav .ts-segmented-item")
              .filter({ hasText: /^通用$/ })
              .first()
              .click();
            await page.locator('[aria-label="界面语言"]').click();
            await page
              .locator(".n-base-select-option")
              .filter({ hasText: /^English$/ })
              .click();
            await page.waitForTimeout(350);
            const textChange = await frames(page, true);
            recordAligned(ctx, `${tag} 文案宽度变化即时对齐`, textChange, [navId, themeId]);
            await page.getByRole("radio", { name: "Summary", exact: true }).click();
            await page.waitForTimeout(350);
            const restored = await frames(page, true);
            recordAligned(ctx, `${tag} 隐藏筛选恢复首帧即时对齐`, restored, [sourceId, dimId]);

            await page.setViewportSize(
              viewport.width === 1280 ? { width: 980, height: 620 } : { width: 1280, height: 820 },
            );
            await page.waitForTimeout(350);
            const resized = await frames(page);
            recordAligned(ctx, `${tag} 窗口缩放逐帧对齐`, resized, [
              sourceId,
              dimId,
              navId,
              themeId,
            ]);
            Object.assign(entry, {
              mouse,
              keyboard,
              navigation,
              themeSwitch,
              textChange,
              restored,
              resized,
            });
            await page.evaluate(() => {
              window.__segmentProbe = false;
            });
            ctx.finishSession(session);
          } finally {
            release();
            await session.close();
          }
        }
  } finally {
    ctx.artifact("segmented-startup-frames.json", evidence);
  }
}
