// 真实 SVG 几何验收：逐个标签与四桶堆叠柱的中心对齐，不以 option 代替绘制结果。
// 所有写入式交互走真实鼠标/键盘；evaluate 仅测量和采样，不调用图表/组件方法。
import { color } from "echarts";

const CONTRACT = "model_chart_readable_rows";
const COLORS = {
  light: "rgb(76, 141, 246)",
  dark: "rgb(100, 160, 255)",
};

async function click(page, locator) {
  await locator.evaluate((el) => el.scrollIntoView({ block: "center" }));
  await locator.click();
  await page.waitForTimeout(250);
}

async function geometry(page, theme) {
  // ECharts 默认 emphasis 通过 zrender liftColor(-0.1) 提亮输入色。
  // 普通滚轮/移动鼠标不保证立即清除 hover；退场动画还会产生两端之间的
  // 插值色。识别同一输入色到默认强调色的有界通道区间，而不是只认两端，
  // 也不扩大到任意蓝色；几何/名称/数值的逐行断言仍全部保留。
  const normal = color.parse(COLORS[theme]).slice(0, 3);
  const emphasis = color.parse(color.lift(COLORS[theme], -0.1));
  return page.locator(".chart-canvas svg").evaluate(
    (svg, { normal, emphasis }) => {
      const center = (el) => {
        const b = el.getBoundingClientRect();
        return { y: b.y + b.height / 2, width: b.width, height: b.height };
      };
      const isInput = (p) => {
        const components = /^rgba?\(([^)]+)\)$/
          .exec(getComputedStyle(p).fill)?.[1]
          .split(/[,\s/]+/)
          .filter(Boolean)
          .map(Number);
        if (!components || (components.length === 4 && components[3] !== 1)) return false;
        return normal.every(
          (v, i) =>
            components[i] >= Math.min(v, emphasis[i]) && components[i] <= Math.max(v, emphasis[i]),
        );
      };
      const bars = [...svg.querySelectorAll("path")]
        .filter(isInput)
        .map(center)
        .sort((a, b) => a.y - b.y);
      const labels = [...svg.querySelectorAll("text")]
        .filter((n) => /^Model \d\d/.test(n.textContent))
        .map((el) => ({ text: el.textContent, ...center(el) }))
        .sort((a, b) => a.y - b.y);
      const pairs = bars.map((bar, i) => ({
        label: labels[i]?.text,
        error: labels[i] ? Math.abs(bar.y - labels[i].y) : null,
        // 输入系列值降序，柱宽也须降序，不能仅有行数相等却名称错位。
        width: bar.width,
      }));
      return {
        pathFills: [...svg.querySelectorAll("path")].reduce((counts, p) => {
          const fill = getComputedStyle(p).fill;
          counts[fill] = (counts[fill] ?? 0) + 1;
          return counts;
        }, {}),
        height: svg.getBoundingClientRect().height,
        width: svg.getBoundingClientRect().width,
        bars: bars.length,
        labels: labels.length,
        minPitch: bars.length > 1 ? Math.min(...bars.slice(1).map((b, i) => b.y - bars[i].y)) : 0,
        maxError: Math.max(0, ...pairs.map((p) => p.error ?? 9999)),
        pairs,
      };
    },
    { normal, emphasis },
  );
}

function recordRows(ctx, label, metrics, count, totalCount = count) {
  const names = metrics.pairs.map((p) => Number(/Model (\d\d)/.exec(p.label ?? "")?.[1]));
  const ordered = names.every(
    (id, i) => Number.isFinite(id) && (i === 0 || id === names[i - 1] + 1),
  );
  const widths = metrics.pairs.every((p, i) => i === 0 || p.width < metrics.pairs[i - 1].width);
  const unitWidth = metrics.pairs[0]?.width / (totalCount - names[0]);
  const valuesMatchNames = metrics.pairs.every(
    (p, i) => Math.abs(p.width - unitWidth * (totalCount - names[i])) < 0.2,
  );
  ctx.record(
    CONTRACT,
    label,
    metrics.bars === count &&
      metrics.labels === count &&
      metrics.maxError <= 2 &&
      (count < 2 || metrics.minPitch >= 24 - 0.001) &&
      ordered &&
      widths &&
      valuesMatchNames,
    JSON.stringify(metrics),
  );
}

async function filterInteractions(ctx, session) {
  const page = session.page;
  const rows = page.locator(".usage-card tbody tr:not(.total-row)");
  const row = rows.nth(1);
  const text = await row.locator("td").first().innerText();
  const aria = await row.getAttribute("aria-label");
  ctx.record(
    CONTRACT,
    "模型行内只有展示名且无箭头，无障碍名称仍用展示名",
    text === "Model 01 / a-very-long-friendly-model-name-for-truncation" &&
      aria.includes(text) &&
      !text.includes("›"),
    text,
  );
  const calls = () => session.ipcCalls.filter((c) => c.cmd === "query_events");
  for (const action of ["click", "Enter", "Space"]) {
    if (action === "click") await click(page, row.locator("td").first());
    else {
      await row.focus();
      await page.keyboard.press(action);
    }
    await page.waitForTimeout(350);
    const last = calls().at(-1)?.args;
    const detail = await page.locator(".events-card").innerText();
    ctx.record(
      CONTRACT,
      `行 ${action} 应用正确模型筛选并显示匹配明细`,
      last?.model === "readable-model-1" &&
        detail.includes("readable-model-1") &&
        !detail.includes("readable-model-0"),
      JSON.stringify({ last, detail }),
    );
    // 点击合计行清除筛选：沿用产品本身的 total-row 行为。
    await click(page, page.locator(".usage-card tbody tr.total-row td").first());
    await page.waitForTimeout(300);
  }
  for (const action of ["click", "Enter", "Space"]) {
    const before = calls().length;
    const button = row.locator(".gcb-trigger");
    if (action === "click") await click(page, button);
    else {
      await button.focus();
      await page.keyboard.press(action);
    }
    await page.waitForTimeout(250);
    ctx.record(
      CONTRACT,
      `费用按钮 ${action} 只打开详情不触发行筛选`,
      calls().length === before && !!(await button.getAttribute("aria-describedby")),
      `明细调用 ${before}→${calls().length} 描述=${await button.getAttribute("aria-describedby")}`,
    );
    await page.keyboard.press("Escape");
    await page.mouse.move(2, 2);
    await page.locator(".chart-reset").focus();
  }
}

export async function uiModelRows(ctx) {
  const measurements = [];
  try {
    for (const count of [5, 20, 40])
      for (const theme of ["light", "dark"]) {
        for (const viewport of [
          { width: 1280, height: 820, label: "1280x820" },
          { width: 980, height: 620, label: "980x620" },
        ]) {
          const session = await ctx.open({ fixture: `model-rows-${count}`, theme, viewport });
          try {
            const page = session.page;
            // 主 runner 为兼容旧契约会过滤 RO loop 日志，本契约单独严格采集。
            const resizeErrors = [];
            page.on("pageerror", (e) => {
              if (/ResizeObserver/i.test(e.message)) resizeErrors.push(e.message);
            });
            await page.goto(session.url, { waitUntil: "networkidle" });
            await page.waitForSelector(".page-head");
            await click(page, page.getByRole("radio", { name: "按模型", exact: true }));
            await page.waitForTimeout(1000);
            const full = await geometry(page, theme);
            measurements.push({ count, theme, viewport: viewport.label, state: "full", ...full });
            recordRows(ctx, `${count}/${theme}/${viewport.label} 全部模型逐条对齐`, full, count);
            await ctx.shot(CONTRACT, session, "full");

            // 悬浮实际省略过的轴标签可看到完整展示名（而非身份键）。
            const firstLabel = page
              .locator(".chart-canvas svg text")
              .filter({ hasText: /^Model 00/ })
              .first();
            await firstLabel.scrollIntoViewIfNeeded();
            await firstLabel.hover();
            await page.waitForTimeout(400);
            const tooltip = await page.locator("body").innerText();
            ctx.record(
              CONTRACT,
              `${count}/${theme}/${viewport.label} 长名称悬浮完整`,
              tooltip.includes("Model 00 / a-very-long-friendly-model-name-for-truncation"),
              `轴标签=${await firstLabel.textContent()}`,
            );
            // 强制真实悬浮输入柱，不能假定 CI 的 emphasis 已按固定时长解除。
            const inputIndex = await page
              .locator(".chart-canvas svg")
              .evaluate(
                (svg, fill) =>
                  [...svg.querySelectorAll("path")].findIndex(
                    (p) => getComputedStyle(p).fill === fill,
                  ),
                COLORS[theme],
              );
            await page.locator(".chart-canvas svg path").nth(inputIndex).hover();
            await page.waitForTimeout(400);
            recordRows(
              ctx,
              `${count}/${theme}/${viewport.label} 悬浮强调色不漏计柱条`,
              await geometry(page, theme),
              count,
            );
            await page.mouse.move(2, 2);
            await page.waitForTimeout(50);
            recordRows(
              ctx,
              `${count}/${theme}/${viewport.label} 强调色退场中间帧不漏计柱条`,
              await geometry(page, theme),
              count,
            );

            if (count === 40) {
              // 持续采样高度，检查缩放与 observer 不出现 A→B→A 反复振荡。
              await page.evaluate(() => {
                window.__modelHeights = [];
                window.__modelMeasure = true;
                const sample = () => {
                  if (!window.__modelMeasure) return;
                  window.__modelHeights.push(
                    document.querySelector(".chart-canvas svg")?.getBoundingClientRect().height,
                  );
                  requestAnimationFrame(sample);
                };
                requestAnimationFrame(sample);
              });
              await firstLabel.evaluate((el) => el.scrollIntoView({ block: "center" }));
              const b = await firstLabel.boundingBox();
              const canvas = await page.locator(".chart-canvas").boundingBox();
              await page.mouse.move(canvas.x + canvas.width * 0.55, b.y + b.height / 2 + 30);
              await page.keyboard.down("Control");
              for (let step = 0; step < 7; step++) {
                await page.mouse.wheel(0, -600);
                await page.waitForTimeout(150);
              }
              await page.keyboard.up("Control");
              await page.mouse.move(2, 2);
              await page.waitForTimeout(1100);
              const zoomed = await geometry(page, theme);
              measurements.push({
                count,
                theme,
                viewport: viewport.label,
                state: "zoomed",
                ...zoomed,
              });
              recordRows(
                ctx,
                `40/${theme}/${viewport.label} 缩放后逐条对齐`,
                zoomed,
                zoomed.bars,
                40,
              );
              const heights = await page.evaluate(() => {
                window.__modelMeasure = false;
                return window.__modelHeights.filter((v, i, a) => i === 0 || v !== a[i - 1]);
              });
              ctx.record(
                CONTRACT,
                `40/${theme}/${viewport.label} 缩放收短且无高度振荡`,
                zoomed.bars > 0 &&
                  zoomed.bars < 40 &&
                  zoomed.height < full.height &&
                  heights.every((h, i) => i === 0 || h < heights[i - 1]) &&
                  resizeErrors.length === 0,
                JSON.stringify({ heights, resizeErrors }),
              );

              // 普通滚轮只滚动页面，不能改动类别范围。
              await page
                .locator(".chart-body")
                .evaluate((el) => el.scrollIntoView({ block: "center" }));
              const body = await page.locator(".chart-body").boundingBox();
              await page.mouse.move(body.x + body.width / 2, body.y + body.height / 2);
              const scrollBefore = await page
                .locator(".scroll-container")
                .first()
                .evaluate((el) => el.scrollTop);
              await page.mouse.wheel(0, 240);
              await page.waitForTimeout(400);
              const scrollAfter = await page
                .locator(".scroll-container")
                .first()
                .evaluate((el) => el.scrollTop);
              // 悬停会临时改变命中的柱段颜色；移开后再按语义 fill 计数，不能把
              // emphasis 色的那一根误报为缩放筛掉。
              await page.mouse.move(2, 2);
              await page.waitForTimeout(200);
              const afterWheel = await geometry(page, theme);
              measurements.push({
                count,
                theme,
                viewport: viewport.label,
                state: "ordinary-wheel",
                ...afterWheel,
              });
              ctx.record(
                CONTRACT,
                `40/${theme}/${viewport.label} 普通滚轮滚动页面不缩放`,
                scrollAfter > scrollBefore &&
                  afterWheel.height === zoomed.height &&
                  JSON.stringify(afterWheel.pairs.map((p) => p.label)) ===
                    JSON.stringify(zoomed.pairs.map((p) => p.label)),
                JSON.stringify({ scrollBefore, scrollAfter, before: zoomed, after: afterWheel }),
              );
              await click(page, page.getByRole("button", { name: "显示全部", exact: true }));
              await page.waitForTimeout(800);
              const reset = await geometry(page, theme);
              measurements.push({
                count,
                theme,
                viewport: viewport.label,
                state: "reset",
                ...reset,
              });
              recordRows(ctx, `40/${theme}/${viewport.label} 显示全部恢复40行`, reset, 40);
              ctx.record(
                CONTRACT,
                `40/${theme}/${viewport.label} 恢复相同行高`,
                reset.height === full.height,
                `${full.height}→${zoomed.height}→${reset.height}`,
              );
            }
            if (count === 5) await filterInteractions(ctx, session);
            ctx.finishSession(session);
          } finally {
            await session.close();
          }
        }
      }
  } finally {
    ctx.artifact("model-rows-measurements.json", measurements);
  }
}
