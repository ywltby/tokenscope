// 真实 SVG 几何验收：模型 / 项目 / 应用三个非时间维度共用逐行自适应布局，
// 逐个核对标签、四桶堆叠柱、行距、中心对齐、窄窗防挤占，以及滑块 / Ctrl+滚轮 /
// 「显示全部」缩放联动；所有测量都在图表过渡稳定后采集，避免动画中间帧污染结果。
// 所有写入式交互走真实鼠标/键盘；evaluate 仅测量和采样，不调用图表/组件方法。
import { color } from "echarts";

const CONTRACT = "model_chart_readable_rows";
const SERIES_COLORS = {
  light: {
    input: "rgb(76, 141, 246)",
    output: "rgb(242, 162, 74)",
    cache_write: "rgb(91, 191, 122)",
    cache_read: "rgb(214, 91, 136)",
  },
  dark: {
    input: "rgb(100, 160, 255)",
    output: "rgb(255, 179, 92)",
    cache_write: "rgb(111, 211, 141)",
    cache_read: "rgb(240, 128, 171)",
  },
};

const DIMENSIONS = [
  {
    key: "model",
    radio: "按模型",
    prefix: "Model",
    fullTooltip0: "Model 00 / a-very-long-friendly-model-name-for-truncation",
    requireAxisEllipsis: false,
    requireAxisTruncated: true,
  },
  {
    key: "project",
    radio: "按项目",
    prefix: "Project",
    fullTooltip0:
      "/workspace/org/team/repo-group/Project 00 / a-very-long-project-directory-name-for-truncation",
    requireAxisEllipsis: true,
    requireAxisTruncated: true,
  },
  {
    key: "agent",
    radio: "按应用",
    prefix: "Agent",
    fullTooltip0: "Agent 00 / synthetic-coding-assistant",
    requireAxisEllipsis: false,
    requireAxisTruncated: false,
  },
];

function colorRanges(theme) {
  return Object.fromEntries(
    Object.entries(SERIES_COLORS[theme]).map(([key, css]) => [
      key,
      {
        normal: color.parse(css).slice(0, 3),
        emphasis: color.parse(color.lift(css, -0.1)).slice(0, 3),
      },
    ]),
  );
}

async function click(page, locator) {
  await locator.evaluate((el) => el.scrollIntoView({ block: "center" }));
  await locator.click();
  await page.waitForTimeout(180);
}

async function geometry(page, theme, prefix) {
  // ECharts 默认 emphasis 通过 zrender liftColor(-0.1) 提亮系列色。
  // 普通滚轮/移动鼠标不保证立即清除 hover；退场动画还会产生两端之间的
  // 插值色。对四桶系列色均按 [normal, emphasis] 有界通道区间识别，
  // 几何/名称/数值的逐行断言全部保留。
  const ranges = colorRanges(theme);
  return page.locator(".chart-body").evaluate(
    (body, { ranges, prefix }) => {
      const svg = body.querySelector(".chart-canvas svg");
      if (!svg) return null;
      const svgRect = svg.getBoundingClientRect();
      const bodyRect = body.getBoundingClientRect();
      const bodyStyle = getComputedStyle(body);
      const padX =
        (Number.parseFloat(bodyStyle.paddingLeft) || 0) +
        (Number.parseFloat(bodyStyle.paddingRight) || 0);
      const resetBtn = body.querySelector(".chart-reset");
      const resetRect = resetBtn ? resetBtn.getBoundingClientRect() : null;

      const parseRgb = (css) => {
        const components = /^rgba?\(([^)]+)\)$/
          .exec(css)?.[1]
          .split(/[,\s/]+/)
          .filter(Boolean)
          .map(Number);
        if (!components || (components.length === 4 && components[3] !== 1)) return null;
        return components;
      };
      const matchSeries = (p) => {
        const rgb = parseRgb(getComputedStyle(p).fill);
        if (!rgb) return null;
        for (const [key, { normal, emphasis }] of Object.entries(ranges)) {
          if (
            normal.every(
              (v, i) => rgb[i] >= Math.min(v, emphasis[i]) && rgb[i] <= Math.max(v, emphasis[i]),
            )
          ) {
            return key;
          }
        }
        return null;
      };
      const box = (el) => {
        const b = el.getBoundingClientRect();
        return {
          left: b.left,
          right: b.right,
          top: b.top,
          bottom: b.bottom,
          y: b.y + b.height / 2,
          width: b.width,
          height: b.height,
        };
      };

      const seriesCounts = { input: 0, output: 0, cache_write: 0, cache_read: 0 };
      const bars = [];
      const allBars = [];
      const sliderParts = [];
      for (const p of svg.querySelectorAll("path, rect")) {
        const s = matchSeries(p);
        const b = box(p);
        if (s) {
          seriesCounts[s] += 1;
          allBars.push(b);
          if (s === "input") bars.push(b);
          continue;
        }
        // 识别位于画布右侧的纵向 dataZoom 滑块（不含透明辅助节点与坐标轴线）。
        const style = getComputedStyle(p);
        const visibleFill = style.fill !== "none" && style.fill !== "rgba(0, 0, 0, 0)";
        if (
          visibleFill &&
          b.width >= 4 &&
          b.width <= 36 &&
          b.height >= 4 &&
          b.left >= svgRect.right - 48 &&
          b.right <= svgRect.right + 1
        ) {
          sliderParts.push(b);
        }
      }
      bars.sort((a, b) => a.y - b.y);
      const labelRe = new RegExp(`^${prefix} \\d\\d`);
      const labels = [...svg.querySelectorAll("text")]
        .filter((n) => labelRe.test(n.textContent ?? ""))
        .map((el) => ({ text: el.textContent ?? "", ...box(el) }))
        .sort((a, b) => a.y - b.y);
      const pairs = bars.map((bar, i) => ({
        label: labels[i]?.text,
        error: labels[i] ? Math.abs(bar.y - labels[i].y) : null,
        // 输入系列值降序，柱宽也须降序，不能仅有行数相等却名称错位。
        width: bar.width,
        y: bar.y,
      }));
      const minLabelLeft = labels.length ? Math.min(...labels.map((l) => l.left)) : null;
      const maxLabelRight = labels.length ? Math.max(...labels.map((l) => l.right)) : null;
      const minBarLeft = allBars.length ? Math.min(...allBars.map((b) => b.left)) : null;
      const maxBarRight = allBars.length ? Math.max(...allBars.map((b) => b.right)) : null;
      const maxBarBottom = allBars.length ? Math.max(...allBars.map((b) => b.bottom)) : null;
      const xAxisLabels = [...svg.querySelectorAll("text")]
        .map((el) => ({ text: el.textContent ?? "", ...box(el) }))
        .filter(
          (t) =>
            t.text.trim().length > 0 &&
            minBarLeft != null &&
            maxBarBottom != null &&
            t.left >= minBarLeft - 24 &&
            t.top >= maxBarBottom - 2,
        );
      const slider = sliderParts.length
        ? {
            left: Math.min(...sliderParts.map((s) => s.left)),
            right: Math.max(...sliderParts.map((s) => s.right)),
            top: Math.min(...sliderParts.map((s) => s.top)),
            bottom: Math.max(...sliderParts.map((s) => s.bottom)),
          }
        : null;
      return {
        height: svgRect.height,
        width: svgRect.width,
        svgLeft: svgRect.left,
        svgRight: svgRect.right,
        svgTop: svgRect.top,
        svgBottom: svgRect.bottom,
        bodyClientWidth: body.clientWidth,
        bodyScrollWidth: body.scrollWidth,
        bodyInnerWidth: bodyRect.width - padX,
        bars: bars.length,
        labels: labels.length,
        seriesCounts,
        minPitch: bars.length > 1 ? Math.min(...bars.slice(1).map((b, i) => b.y - bars[i].y)) : 0,
        maxError: Math.max(0, ...pairs.map((p) => p.error ?? 9999)),
        minLabelLeft,
        maxLabelRight,
        minBarLeft,
        maxBarRight,
        maxBarBottom,
        plotWidth: minBarLeft != null && maxBarRight != null ? maxBarRight - minBarLeft : 0,
        slider,
        xAxisLabels,
        resetRect: resetRect
          ? {
              left: resetRect.left,
              right: resetRect.right,
              top: resetRect.top,
              bottom: resetRect.bottom,
            }
          : null,
        pairs,
      };
    },
    { ranges, prefix },
  );
}

function geometrySignature(m) {
  if (!m) return "null";
  const r = (v) => (v == null ? "n" : Math.round(v * 2) / 2);
  return JSON.stringify({
    h: r(m.height),
    w: r(m.width),
    bars: m.bars,
    labels: m.labels,
    sc: m.seriesCounts,
    lr: r(m.maxLabelRight),
    bl: r(m.minBarLeft),
    br: r(m.maxBarRight),
    sl: m.slider ? [r(m.slider.left), r(m.slider.top), r(m.slider.bottom)] : null,
    pairs: m.pairs.map((p) => [p.label, r(p.y), r(p.width)]),
  });
}

/**
 * 等待图表过渡动画完全停止后再返回最终几何（连续多帧完全一致），
 * 避免在柱形增长或缩放过渡中间帧误判宽高、行距或中心对齐。
 */
async function waitForChartSettled(
  page,
  theme,
  prefix,
  { minStableMs = 280, pollMs = 90, timeoutMs = 6000, predicate = null } = {},
) {
  const deadline = Date.now() + timeoutMs;
  let lastSig = null;
  let stableSince = 0;
  let latest = null;
  while (Date.now() < deadline) {
    latest = await geometry(page, theme, prefix);
    const ready =
      latest && latest.bars > 0 && latest.labels > 0 && (predicate == null || predicate(latest));
    if (ready) {
      const sig = geometrySignature(latest);
      if (sig === lastSig) {
        if (Date.now() - stableSince >= minStableMs) return latest;
      } else {
        lastSig = sig;
        stableSince = Date.now();
      }
    } else {
      lastSig = null;
      stableSince = Date.now();
    }
    await page.waitForTimeout(pollMs);
  }
  return latest;
}

function recordRows(ctx, label, metrics, count, prefix, totalCount = count) {
  const expectedHeight = Math.max(320, count * 24 + 80);
  const re = new RegExp(`${prefix} (\\d\\d)`);
  const names = metrics.pairs.map((p) => Number(re.exec(p.label ?? "")?.[1]));
  const ordered = names.every(
    (id, i) => Number.isFinite(id) && (i === 0 || id === names[i - 1] + 1),
  );
  const widths = metrics.pairs.every((p, i) => i === 0 || p.width < metrics.pairs[i - 1].width);
  const unitWidth = metrics.pairs[0]?.width / (totalCount - names[0]);
  const valuesMatchNames = metrics.pairs.every(
    (p, i) => Math.abs(p.width - unitWidth * (totalCount - names[i])) < 0.35,
  );
  const fourBuckets =
    metrics.seriesCounts.input === count &&
    metrics.seriesCounts.output === count &&
    metrics.seriesCounts.cache_write === count &&
    metrics.seriesCounts.cache_read === count;
  ctx.record(
    CONTRACT,
    label,
    Math.abs(metrics.height - expectedHeight) <= 1 &&
      metrics.bars === count &&
      metrics.labels === count &&
      fourBuckets &&
      metrics.maxError <= 2 &&
      (count < 2 || metrics.minPitch >= 24 - 0.001) &&
      ordered &&
      widths &&
      valuesMatchNames,
    JSON.stringify({
      expectedHeight,
      height: metrics.height,
      width: metrics.width,
      bars: metrics.bars,
      labels: metrics.labels,
      seriesCounts: metrics.seriesCounts,
      minPitch: metrics.minPitch,
      maxError: metrics.maxError,
      ordered,
      widths,
      valuesMatchNames,
    }),
  );
}

function recordLayoutZones(ctx, label, metrics) {
  const noOverflow = metrics.bodyScrollWidth <= metrics.bodyClientWidth + 1;
  const widthMatchesContainer =
    Math.abs(metrics.width - metrics.bodyInnerWidth) <= 2 &&
    metrics.width <= metrics.bodyClientWidth + 1;
  const labelsInCanvas =
    metrics.minLabelLeft != null && metrics.minLabelLeft >= metrics.svgLeft + 4;
  const labelBeforePlot =
    metrics.maxLabelRight != null &&
    metrics.minBarLeft != null &&
    metrics.maxLabelRight <= metrics.minBarLeft - 2;
  const plotWideEnough = metrics.plotWidth >= 180;
  const sliderValid = metrics.slider != null && metrics.slider.right <= metrics.svgRight + 1;
  const plotBeforeSlider =
    metrics.slider != null &&
    metrics.maxBarRight != null &&
    metrics.maxBarRight <= metrics.slider.left - 8;
  const resetClearOfBars =
    metrics.resetRect != null &&
    metrics.maxBarBottom != null &&
    (metrics.resetRect.top >= metrics.maxBarBottom - 1 ||
      metrics.resetRect.left >= metrics.maxBarRight + 4);
  const resetClearOfXAxis =
    metrics.resetRect != null &&
    (metrics.xAxisLabels ?? []).every(
      (t) =>
        t.bottom <= metrics.resetRect.top - 2 ||
        t.right <= metrics.resetRect.left - 2 ||
        t.left >= metrics.resetRect.right + 2,
    );
  ctx.record(
    CONTRACT,
    label,
    noOverflow &&
      widthMatchesContainer &&
      labelsInCanvas &&
      labelBeforePlot &&
      plotWideEnough &&
      sliderValid &&
      plotBeforeSlider &&
      resetClearOfBars &&
      resetClearOfXAxis,
    JSON.stringify({
      noOverflow,
      widthMatchesContainer,
      labelsInCanvas,
      labelBeforePlot,
      plotWideEnough,
      sliderValid,
      plotBeforeSlider,
      resetClearOfBars,
      resetClearOfXAxis,
      bodyClientWidth: metrics.bodyClientWidth,
      bodyScrollWidth: metrics.bodyScrollWidth,
      bodyInnerWidth: metrics.bodyInnerWidth,
      svgWidth: metrics.width,
      labelRange: [metrics.minLabelLeft, metrics.maxLabelRight],
      barRange: [metrics.minBarLeft, metrics.maxBarRight],
      plotWidth: metrics.plotWidth,
      slider: metrics.slider,
      resetRect: metrics.resetRect,
      maxBarBottom: metrics.maxBarBottom,
    }),
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

async function ctrlWheelZoom(page, firstLabel) {
  await firstLabel.evaluate((el) => el.scrollIntoView({ block: "center" }));
  const b = await firstLabel.boundingBox();
  const canvas = await page.locator(".chart-canvas").boundingBox();
  await page.mouse.move(canvas.x + canvas.width * 0.55, b.y + b.height / 2 + 30);
  await page.keyboard.down("Control");
  for (let step = 0; step < 7; step++) {
    await page.mouse.wheel(0, -600);
    await page.waitForTimeout(140);
  }
  await page.keyboard.up("Control");
  await page.mouse.move(2, 2);
}

async function dragVerticalSliderTopHandle(page) {
  // 把图表顶部滚入可视区域，定位纵向 dataZoom 真实顶手柄并向下拖动。
  await page.locator(".chart-head").evaluate((el) => el.scrollIntoView({ block: "start" }));
  await page.waitForTimeout(150);
  const topHandle = await page.locator(".chart-canvas svg").evaluate((svg) => {
    const sr = svg.getBoundingClientRect();
    const candidates = [...svg.querySelectorAll("path, rect")]
      .map((el) => {
        const b = el.getBoundingClientRect();
        const style = getComputedStyle(el);
        return {
          x: b.left + b.width / 2,
          y: b.top + b.height / 2,
          w: b.width,
          h: b.height,
          fill: style.fill,
        };
      })
      .filter(
        (item) =>
          item.fill !== "none" &&
          item.fill !== "rgba(0, 0, 0, 0)" &&
          item.x >= sr.right - 45 &&
          item.x <= sr.right &&
          item.w >= 10 &&
          item.w <= 24 &&
          item.h > 0 &&
          item.h <= 12,
      )
      .sort((a, b) => a.y - b.y);
    return candidates[0] ?? null;
  });
  if (!topHandle) throw new Error("未在纵向 dataZoom 滑块区域找到顶部手柄");
  const viewport = page.viewportSize();
  const maxVisibleBottom = viewport.height - 32;
  const dragDistance = Math.min(320, Math.max(180, maxVisibleBottom - topHandle.y));
  await page.mouse.move(topHandle.x, topHandle.y);
  await page.mouse.down();
  await page.mouse.move(topHandle.x, topHandle.y + dragDistance, { steps: 14 });
  await page.mouse.up();
  await page.mouse.move(2, 2);
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

            for (const dim of DIMENSIONS) {
              const tag = `${dim.key}/${count}/${theme}/${viewport.label}`;
              await click(page, page.getByRole("radio", { name: dim.radio, exact: true }));
              const full = await waitForChartSettled(page, theme, dim.prefix, {
                predicate: (m) => m.bars === count && m.labels === count,
              });
              measurements.push({
                dim: dim.key,
                count,
                theme,
                viewport: viewport.label,
                state: "full",
                ...full,
              });
              recordRows(ctx, `${tag} 全部类别逐条对齐`, full, count, dim.prefix);
              recordLayoutZones(ctx, `${tag} 宽度自适应且标签/绘图/滑块不挤占`, full);
              if (count === 40 || dim.key === "project") {
                await ctx.shot(CONTRACT, session, `${dim.key}-${count}-full`);
              }

              // 核对长轴标签已省略，并真实悬浮首行柱条检查 .chart-tooltip 显示完整名称（项目为完整路径）。
              const firstLabel = page
                .locator(".chart-canvas svg text")
                .filter({ hasText: new RegExp(`^${dim.prefix} 00`) })
                .first();
              await firstLabel.scrollIntoViewIfNeeded();
              const axisText = (await firstLabel.textContent()) ?? "";
              const firstBarPoint = await page
                .locator(".chart-canvas svg")
                .evaluate((svg, fill) => {
                  const bars = [...svg.querySelectorAll("path")]
                    .filter((p) => getComputedStyle(p).fill === fill)
                    .map((p) => {
                      const b = p.getBoundingClientRect();
                      return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
                    })
                    .sort((a, b) => a.y - b.y);
                  return bars[0] ?? null;
                }, SERIES_COLORS[theme].input);
              if (firstBarPoint) {
                await page.mouse.move(firstBarPoint.x, firstBarPoint.y);
                await page.waitForTimeout(300);
              }
              const tooltipState = await page.evaluate(() => {
                const nodes = [...document.querySelectorAll(".chart-tooltip")];
                const visible = nodes.find((el) => {
                  const host = el.parentElement ?? el;
                  const s = getComputedStyle(host);
                  return (
                    s.display !== "none" && s.visibility !== "hidden" && Number(s.opacity) > 0.5
                  );
                });
                return visible ? (visible.textContent ?? "") : "";
              });
              const axisTruncatedOk =
                (!dim.requireAxisTruncated || axisText.length < dim.fullTooltip0.length) &&
                (!dim.requireAxisEllipsis || axisText.includes("…"));
              ctx.record(
                CONTRACT,
                `${tag} 轴标签省略且悬浮显示全名`,
                axisTruncatedOk && tooltipState.includes(dim.fullTooltip0),
                `轴标签=${axisText} tooltip=${JSON.stringify(tooltipState.slice(0, 96))}`,
              );

              if (dim.key === "model") {
                // 已真实悬浮首行输入柱：验证 emphasis 与退场中间帧不漏计柱条。
                recordRows(
                  ctx,
                  `${tag} 悬浮强调色不漏计柱条`,
                  await geometry(page, theme, dim.prefix),
                  count,
                  dim.prefix,
                );
                await page.mouse.move(2, 2);
                await page.waitForTimeout(50);
                recordRows(
                  ctx,
                  `${tag} 强调色退场中间帧不漏计柱条`,
                  await geometry(page, theme, dim.prefix),
                  count,
                  dim.prefix,
                );
                if (count === 5) await filterInteractions(ctx, session);
              } else {
                await page.mouse.move(2, 2);
                await page.waitForTimeout(120);
              }

              if (count === 40) {
                // 1) Ctrl + 滚轮缩放：持续采样高度，检查不出现 A→B→A 振荡。
                await page.evaluate(() => {
                  window.__chartHeights = [];
                  window.__chartMeasure = true;
                  const sample = () => {
                    if (!window.__chartMeasure) return;
                    window.__chartHeights.push(
                      document.querySelector(".chart-canvas svg")?.getBoundingClientRect().height,
                    );
                    requestAnimationFrame(sample);
                  };
                  requestAnimationFrame(sample);
                });
                await ctrlWheelZoom(page, firstLabel);
                const zoomed = await waitForChartSettled(page, theme, dim.prefix, {
                  predicate: (m) => m.bars > 0 && m.bars < 40,
                });
                measurements.push({
                  dim: dim.key,
                  count,
                  theme,
                  viewport: viewport.label,
                  state: "ctrl-wheel-zoomed",
                  ...zoomed,
                });
                recordRows(
                  ctx,
                  `${tag} Ctrl+滚轮缩放后逐条对齐`,
                  zoomed,
                  zoomed.bars,
                  dim.prefix,
                  40,
                );
                recordLayoutZones(ctx, `${tag} Ctrl+滚轮缩放后不挤占`, zoomed);
                const heights = await page.evaluate(() => {
                  window.__chartMeasure = false;
                  return window.__chartHeights.filter((v, i, a) => i === 0 || v !== a[i - 1]);
                });
                ctx.record(
                  CONTRACT,
                  `${tag} Ctrl+滚轮缩放收短且无高度振荡`,
                  zoomed.bars > 0 &&
                    zoomed.bars < 40 &&
                    zoomed.height < full.height &&
                    heights.every((h, i) => i === 0 || h < heights[i - 1]) &&
                    resizeErrors.length === 0,
                  JSON.stringify({ heights, resizeErrors }),
                );

                // 2) 普通滚轮只滚动页面，不能改动类别范围或高度。
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
                await page.waitForTimeout(350);
                const scrollAfter = await page
                  .locator(".scroll-container")
                  .first()
                  .evaluate((el) => el.scrollTop);
                await page.mouse.move(2, 2);
                const afterWheel = await waitForChartSettled(page, theme, dim.prefix);
                measurements.push({
                  dim: dim.key,
                  count,
                  theme,
                  viewport: viewport.label,
                  state: "ordinary-wheel",
                  ...afterWheel,
                });
                ctx.record(
                  CONTRACT,
                  `${tag} 普通滚轮滚动页面不缩放`,
                  scrollAfter > scrollBefore &&
                    afterWheel.height === zoomed.height &&
                    JSON.stringify(afterWheel.pairs.map((p) => p.label)) ===
                      JSON.stringify(zoomed.pairs.map((p) => p.label)),
                  JSON.stringify({
                    scrollBefore,
                    scrollAfter,
                    before: zoomed.height,
                    after: afterWheel.height,
                  }),
                );

                // 3) 「显示全部」恢复 40 行与原高度。
                await click(page, page.getByRole("button", { name: "显示全部", exact: true }));
                const resetAfterWheel = await waitForChartSettled(page, theme, dim.prefix, {
                  predicate: (m) => m.bars === 40 && m.labels === 40,
                });
                measurements.push({
                  dim: dim.key,
                  count,
                  theme,
                  viewport: viewport.label,
                  state: "reset-after-wheel",
                  ...resetAfterWheel,
                });
                recordRows(ctx, `${tag} 显示全部恢复40行`, resetAfterWheel, 40, dim.prefix);
                ctx.record(
                  CONTRACT,
                  `${tag} 显示全部恢复相同行高`,
                  resetAfterWheel.height === full.height,
                  `${full.height}→${zoomed.height}→${resetAfterWheel.height}`,
                );

                // 4) 拖动纵向滑块触发同一套高度计算，再点「显示全部」恢复。
                await dragVerticalSliderTopHandle(page);
                const sliderZoomed = await waitForChartSettled(page, theme, dim.prefix, {
                  predicate: (m) => m.bars > 0 && m.bars < 40,
                });
                measurements.push({
                  dim: dim.key,
                  count,
                  theme,
                  viewport: viewport.label,
                  state: "slider-zoomed",
                  ...sliderZoomed,
                });
                recordRows(
                  ctx,
                  `${tag} 滑块缩放后逐条对齐与重算高度`,
                  sliderZoomed,
                  sliderZoomed.bars,
                  dim.prefix,
                  40,
                );
                ctx.record(
                  CONTRACT,
                  `${tag} 滑块缩放收短画布`,
                  sliderZoomed.bars > 0 &&
                    sliderZoomed.bars < 40 &&
                    sliderZoomed.height < full.height &&
                    resizeErrors.length === 0,
                  `${full.height}→${sliderZoomed.height} (bars=${sliderZoomed.bars})`,
                );
                await click(page, page.getByRole("button", { name: "显示全部", exact: true }));
                const resetAfterSlider = await waitForChartSettled(page, theme, dim.prefix, {
                  predicate: (m) => m.bars === 40 && m.labels === 40,
                });
                ctx.record(
                  CONTRACT,
                  `${tag} 滑块缩放后显示全部恢复40行与原高度`,
                  resetAfterSlider.bars === 40 &&
                    resetAfterSlider.labels === 40 &&
                    resetAfterSlider.height === full.height,
                  `${full.height}→${sliderZoomed.height}→${resetAfterSlider.height}`,
                );
              }
            }

            // 40 行场景下再核对：缩放后切换维度应重置缩放范围，「按时间」固定 360px 保持原样。
            if (count === 40) {
              const agentFirst = page
                .locator(".chart-canvas svg text")
                .filter({ hasText: /^Agent 00/ })
                .first();
              await ctrlWheelZoom(page, agentFirst);
              const agentZoomed = await waitForChartSettled(page, theme, "Agent", {
                predicate: (m) => m.bars > 0 && m.bars < 40,
              });
              await click(page, page.getByRole("radio", { name: "按项目", exact: true }));
              const projectReset = await waitForChartSettled(page, theme, "Project", {
                predicate: (m) => m.bars === 40 && m.labels === 40,
              });
              ctx.record(
                CONTRACT,
                `40/${theme}/${viewport.label} 切换维度重置缩放并按全量类别计高`,
                agentZoomed.bars < 40 &&
                  projectReset.bars === 40 &&
                  projectReset.labels === 40 &&
                  projectReset.height === 40 * 24 + 80,
                `agent=${agentZoomed.bars}→project=${projectReset.bars}/${projectReset.height}`,
              );

              await click(page, page.getByRole("radio", { name: "按时间", exact: true }));
              await page.waitForTimeout(700);
              const dayHeight = await page
                .locator(".chart-canvas svg")
                .evaluate((svg) => svg.getBoundingClientRect().height);
              ctx.record(
                CONTRACT,
                `40/${theme}/${viewport.label} 按时间保持固定360px`,
                Math.abs(dayHeight - 360) <= 1,
                `dayHeight=${dayHeight}`,
              );
            }

            ctx.finishSession(session);
          } finally {
            await session.close();
          }
        }
      }

    await verifyMixedUnsampledRows(ctx, measurements);
  } finally {
    ctx.artifact("model-rows-measurements.json", measurements);
  }
}

async function mixedGeometry(page, theme) {
  const ranges = colorRanges(theme);
  return page.locator(".chart-body").evaluate(
    (body, { ranges }) => {
      const svg = body.querySelector(".chart-canvas svg");
      if (!svg) return null;
      const svgRect = svg.getBoundingClientRect();
      const bodyRect = body.getBoundingClientRect();
      const bodyStyle = getComputedStyle(body);
      const padX =
        (Number.parseFloat(bodyStyle.paddingLeft) || 0) +
        (Number.parseFloat(bodyStyle.paddingRight) || 0);
      const resetBtn = body.querySelector(".chart-reset");
      const resetRect = resetBtn ? resetBtn.getBoundingClientRect() : null;
      const parseRgb = (css) => {
        const components = /^rgba?\(([^)]+)\)$/
          .exec(css)?.[1]
          .split(/[,\s/]+/)
          .filter(Boolean)
          .map(Number);
        if (!components || (components.length === 4 && components[3] !== 1)) return null;
        return components;
      };
      const matchSeries = (p) => {
        const rgb = parseRgb(getComputedStyle(p).fill);
        if (!rgb) return null;
        for (const [key, { normal, emphasis }] of Object.entries(ranges)) {
          if (
            normal.every(
              (v, i) => rgb[i] >= Math.min(v, emphasis[i]) && rgb[i] <= Math.max(v, emphasis[i]),
            )
          ) {
            return key;
          }
        }
        return null;
      };
      const box = (el) => {
        const b = el.getBoundingClientRect();
        return {
          left: b.left,
          right: b.right,
          top: b.top,
          bottom: b.bottom,
          y: b.y + b.height / 2,
          width: b.width,
          height: b.height,
        };
      };
      const bars = [];
      const allBars = [];
      const sliderParts = [];
      for (const p of svg.querySelectorAll("path, rect")) {
        const s = matchSeries(p);
        const b = box(p);
        if (s) {
          allBars.push(b);
          if (s === "input") bars.push(b);
          continue;
        }
        const style = getComputedStyle(p);
        const visibleFill = style.fill !== "none" && style.fill !== "rgba(0, 0, 0, 0)";
        if (
          visibleFill &&
          b.width >= 4 &&
          b.width <= 36 &&
          b.height >= 4 &&
          b.left >= svgRect.right - 48 &&
          b.right <= svgRect.right + 1
        ) {
          sliderParts.push(b);
        }
      }
      bars.sort((a, b) => a.y - b.y);
      const minBarLeft = allBars.length ? Math.min(...allBars.map((b) => b.left)) : null;
      const maxBarRight = allBars.length ? Math.max(...allBars.map((b) => b.right)) : null;
      const maxBarBottom = allBars.length ? Math.max(...allBars.map((b) => b.bottom)) : null;
      const xAxisLabels = [...svg.querySelectorAll("text")]
        .map((el) => ({ text: el.textContent ?? "", ...box(el) }))
        .filter(
          (t) =>
            t.text.trim().length > 0 &&
            minBarLeft != null &&
            maxBarBottom != null &&
            t.left >= minBarLeft - 24 &&
            t.top >= maxBarBottom - 2,
        );
      // 收集全部纵轴标签（位于绘图区左侧且垂直范围落在柱条区内），不做前缀白名单过滤。
      const yLabels = [...svg.querySelectorAll("text")]
        .map((el) => ({ text: el.textContent ?? "", ...box(el) }))
        .filter(
          (t) =>
            t.text.trim().length > 0 &&
            minBarLeft != null &&
            t.right <= minBarLeft + 2 &&
            maxBarBottom != null &&
            t.y <= maxBarBottom + 16,
        )
        .sort((a, b) => a.y - b.y);
      const pairs = bars.map((bar, i) => ({
        label: yLabels[i]?.text,
        leftOffset: yLabels[i] ? Math.round((yLabels[i].left - svgRect.left) * 10) / 10 : null,
        error: yLabels[i] ? Math.abs(bar.y - yLabels[i].y) : null,
        width: bar.width,
        y: bar.y,
        barCenterX: bar.left + bar.width / 2,
      }));
      const minLabelLeft = yLabels.length ? Math.min(...yLabels.map((l) => l.left)) : null;
      const maxLabelRight = yLabels.length ? Math.max(...yLabels.map((l) => l.right)) : null;
      const slider = sliderParts.length
        ? {
            left: Math.min(...sliderParts.map((s) => s.left)),
            right: Math.max(...sliderParts.map((s) => s.right)),
            top: Math.min(...sliderParts.map((s) => s.top)),
            bottom: Math.max(...sliderParts.map((s) => s.bottom)),
          }
        : null;
      return {
        height: svgRect.height,
        width: svgRect.width,
        svgLeft: svgRect.left,
        svgRight: svgRect.right,
        bodyClientWidth: body.clientWidth,
        bodyScrollWidth: body.scrollWidth,
        bodyInnerWidth: bodyRect.width - padX,
        bars: bars.length,
        labels: yLabels.length,
        minPitch: bars.length > 1 ? Math.min(...bars.slice(1).map((b, i) => b.y - bars[i].y)) : 0,
        maxError: Math.max(0, ...pairs.map((p) => p.error ?? 9999)),
        minLabelLeft,
        minLeftOffset:
          minLabelLeft != null ? Math.round((minLabelLeft - svgRect.left) * 10) / 10 : null,
        maxLabelRight,
        minBarLeft,
        maxBarRight,
        maxBarBottom,
        plotWidth: minBarLeft != null && maxBarRight != null ? maxBarRight - minBarLeft : 0,
        slider,
        xAxisLabels,
        resetRect: resetRect
          ? {
              left: resetRect.left,
              right: resetRect.right,
              top: resetRect.top,
              bottom: resetRect.bottom,
            }
          : null,
        pairs,
      };
    },
    { ranges },
  );
}

async function waitForMixedSettled(
  page,
  theme,
  { minStableMs = 260, pollMs = 85, timeoutMs = 6000, predicate = null } = {},
) {
  const deadline = Date.now() + timeoutMs;
  let lastSig = null;
  let stableSince = 0;
  let latest = null;
  while (Date.now() < deadline) {
    latest = await mixedGeometry(page, theme);
    const ready =
      latest && latest.bars > 0 && latest.labels > 0 && (predicate == null || predicate(latest));
    if (ready) {
      const r = (v) => (v == null ? "n" : Math.round(v * 2) / 2);
      const sig = JSON.stringify({
        h: r(latest.height),
        w: r(latest.width),
        b: latest.bars,
        l: latest.labels,
        ml: r(latest.minLabelLeft),
        bl: r(latest.minBarLeft),
        br: r(latest.maxBarRight),
        p: latest.pairs.map((x) => [x.label, r(x.y), r(x.width)]),
      });
      if (sig === lastSig) {
        if (Date.now() - stableSince >= minStableMs) return latest;
      } else {
        lastSig = sig;
        stableSince = Date.now();
      }
    } else {
      lastSig = null;
      stableSince = Date.now();
    }
    await page.waitForTimeout(pollMs);
  }
  return latest;
}

/**
 * 覆盖 40 / 41 / 43 行长短名称混排（把最长名称放在旧 ECharts containLabel
 * 抽样会跳过的奇数索引 1、19、39、41），实测宽窄窗口下的左边界、末尾省略、
 * 悬浮全名（项目显示完整路径）以及缩放与「显示全部」恢复后的布局。
 */
async function verifyMixedUnsampledRows(ctx, measurements) {
  const MIXED_DIMS = [
    {
      key: "model",
      radio: "按模型",
      full1: "DeepSeek V4.1 Flash 1017",
      full19: "DeepSeek V4 Flash 0731",
      full41: "Nemotron 3 Ultra 550B A55B",
      prefix39: "Model 39 / ",
      tooltip39:
        "Model 39 / an-extra-long-friendly-model-name-that-must-truncate-at-end-never-left",
    },
    {
      key: "project",
      radio: "按项目",
      full1: "DeepSeek-Workspace-1017",
      full19: "DeepSeek-Workspace-0731",
      full41: "Nemotron 3 Ultra 550B Repo",
      prefix39: "Project 39 / ",
      tooltip39:
        "/workspace/org/team/repo-group/Project 39 / an-extra-long-project-directory-name-that-must-truncate-at-end-never-left",
    },
    {
      key: "agent",
      radio: "按应用",
      full1: "DeepSeek Agent 1017",
      full19: "DeepSeek Agent 0731",
      full41: "Nemotron 3 Ultra 550B Agent",
      prefix39: "Agent 39 / ",
      tooltip39: "Agent 39 / an-extra-long-coding-assistant-display-name-for-end-ellipsis-check",
    },
  ];
  const MATRIX = [
    { count: 40, themes: ["light"] },
    { count: 41, themes: ["dark"] },
    { count: 43, themes: ["light", "dark"] },
  ];
  for (const { count, themes } of MATRIX) {
    for (const theme of themes) {
      for (const viewport of [
        { width: 1280, height: 820, label: "1280x820" },
        { width: 980, height: 620, label: "980x620" },
      ]) {
        const session = await ctx.open({
          fixture: `model-rows-mixed-${count}`,
          theme,
          viewport,
        });
        try {
          const page = session.page;
          await page.goto(session.url, { waitUntil: "networkidle" });
          await page.waitForSelector(".page-head");

          for (const dim of MIXED_DIMS) {
            const tag = `mixed-${count}/${dim.key}/${theme}/${viewport.label}`;
            await click(page, page.getByRole("radio", { name: dim.radio, exact: true }));
            const full = await waitForMixedSettled(page, theme, {
              predicate: (m) => m.bars === count && m.labels === count,
            });
            measurements.push({
              dim: dim.key,
              count,
              theme,
              viewport: viewport.label,
              state: `mixed-${count}-full`,
              minLeftOffset: full.minLeftOffset,
              height: full.height,
              width: full.width,
              bars: full.bars,
              labels: full.labels,
              minPitch: full.minPitch,
              maxError: full.maxError,
              plotWidth: full.plotWidth,
              row1: full.pairs[1],
              row19: full.pairs[19],
              row39: full.pairs[39],
              row41: full.pairs[41] ?? null,
            });

            const expectedHeight = Math.max(320, count * 24 + 80);
            const row1 = full.pairs[1];
            const row19 = full.pairs[19];
            const row39 = full.pairs[39];
            const row41 = count >= 42 ? full.pairs[41] : null;
            const noLeftClip =
              full.minLeftOffset != null &&
              full.minLeftOffset >= 8 &&
              full.pairs.every((p) => p.leftOffset != null && p.leftOffset >= 8);
            const fullNamesShown =
              row1?.label === dim.full1 &&
              row19?.label === dim.full19 &&
              (row41 == null || row41.label === dim.full41);
            const endTruncatedOk =
              row39?.label != null &&
              row39.label.startsWith(dim.prefix39) &&
              row39.label.endsWith("…") &&
              row39.label.length < dim.tooltip39.length;

            ctx.record(
              CONTRACT,
              `${tag} 全部标签无左裁切且空间充足项完整显示、超长项末尾省略`,
              Math.abs(full.height - expectedHeight) <= 1 &&
                full.bars === count &&
                full.labels === count &&
                full.minPitch >= 24 - 0.001 &&
                full.maxError <= 2 &&
                noLeftClip &&
                fullNamesShown &&
                endTruncatedOk,
              JSON.stringify({
                height: full.height,
                bars: full.bars,
                labels: full.labels,
                minLeftOffset: full.minLeftOffset,
                row1,
                row19,
                row39,
                row41,
              }),
            );
            recordLayoutZones(ctx, `${tag} 宽窄窗标签/绘图/滑块不挤占`, full);

            // 滚动到第 39 行（被末尾省略的超长项）并悬浮其柱条，验证 tooltip 显示完整名称/完整路径。
            const label39 = page
              .locator(".chart-canvas svg text")
              .filter({ hasText: new RegExp(`^${dim.prefix39.replace("/", "\\/")}`) })
              .first();
            await label39.scrollIntoViewIfNeeded();
            await page.waitForTimeout(120);
            const bar39Point = await page.locator(".chart-canvas svg").evaluate((svg, fill) => {
              const bars = [...svg.querySelectorAll("path")]
                .filter((p) => getComputedStyle(p).fill === fill)
                .map((p) => {
                  const b = p.getBoundingClientRect();
                  return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
                })
                .sort((a, b) => a.y - b.y);
              return bars[39] ?? null;
            }, SERIES_COLORS[theme].input);
            if (bar39Point) {
              await page.mouse.move(bar39Point.x, bar39Point.y);
              await page.waitForTimeout(300);
            }
            const tip39 = await page.evaluate(() => {
              const nodes = [...document.querySelectorAll(".chart-tooltip")];
              const visible = nodes.find((el) => {
                const host = el.parentElement ?? el;
                const s = getComputedStyle(host);
                return s.display !== "none" && s.visibility !== "hidden" && Number(s.opacity) > 0.5;
              });
              return visible ? (visible.textContent ?? "") : "";
            });
            ctx.record(
              CONTRACT,
              `${tag} 末尾省略超长项悬浮显示全名`,
              tip39.includes(dim.tooltip39),
              `轴标签=${row39?.label} tooltip=${JSON.stringify(tip39.slice(0, 110))}`,
            );
            await page.mouse.move(2, 2);
            await page.waitForTimeout(100);

            if (count === 43) {
              if (dim.key === "model") {
                await ctx.shot(CONTRACT, session, `mixed-43-${theme}-${viewport.label}`);
              }
              // 拖动纵向滑块顶部手柄向下，保留包含 39/41 行的后半窗口，核对缩放后高度与左边界仍无裁切。
              await dragVerticalSliderTopHandle(page);
              const zoomed = await waitForMixedSettled(page, theme, {
                predicate: (m) => m.bars > 0 && m.bars < 43,
              });
              const expectedZoomHeight = Math.max(320, zoomed.bars * 24 + 80);
              const has41 = zoomed.pairs.some((p) => p.label === dim.full41);
              const has39Ellipsis = zoomed.pairs.some(
                (p) => p.label?.startsWith(dim.prefix39) && p.label.endsWith("…"),
              );
              ctx.record(
                CONTRACT,
                `${tag} 滑块缩放后高度收短且长标签无左裁切`,
                zoomed.bars > 0 &&
                  zoomed.bars < 43 &&
                  Math.abs(zoomed.height - expectedZoomHeight) <= 1 &&
                  zoomed.minLeftOffset != null &&
                  zoomed.minLeftOffset >= 8 &&
                  zoomed.minPitch >= 24 - 0.001 &&
                  zoomed.maxError <= 2 &&
                  has41 &&
                  has39Ellipsis,
                JSON.stringify({
                  bars: zoomed.bars,
                  height: zoomed.height,
                  minLeftOffset: zoomed.minLeftOffset,
                  has41,
                  has39Ellipsis,
                }),
              );
              await click(page, page.getByRole("button", { name: "显示全部", exact: true }));
              const restored = await waitForMixedSettled(page, theme, {
                predicate: (m) => m.bars === 43 && m.labels === 43,
              });
              ctx.record(
                CONTRACT,
                `${tag} 显示全部恢复43行且无左裁切`,
                restored.bars === 43 &&
                  restored.labels === 43 &&
                  Math.abs(restored.height - expectedHeight) <= 1 &&
                  restored.minLeftOffset != null &&
                  restored.minLeftOffset >= 8,
                `height=${restored.height} minLeftOffset=${restored.minLeftOffset}`,
              );
            }
          }

          ctx.finishSession(session);
        } finally {
          await session.close();
        }
      }
    }
  }
}
