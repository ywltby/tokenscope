// F02：真实 ECharts 柱形在数据/布局更新时必须保留中间帧。
//
// 不 mock ECharts、不靠截图延时：连续采样 SVG 柱形的实际几何尺寸，要求至少
// 两个中间帧既不等于更新前、也不等于终态。旧实现的数据更新会改高度并
// resize，ResizeObserver 再补一次无动画 resize，首帧即跳到终态；这里会红。
//
// 场景全部使用 multi-category 合成 fixture：模型 30→20→4、模型→按日、
// 按日 1→7→14、24h 空→有值、连续切换与真实容器 resize。不启动用户实例。

const INPUT_FILL = "rgb(76, 141, 246)";

async function realClick(page, locator) {
  const el = locator.first();
  await el.scrollIntoViewIfNeeded();
  await el.evaluate((node) => node.scrollIntoView({ block: "center", inline: "nearest" }));
  await page.waitForTimeout(100);
  await el.click({ timeout: 10000 });
  await page.waitForTimeout(180);
}

async function clickSegment(page, label) {
  const btn = page.locator(".ts-segmented button", { hasText: label }).first();
  await btn.waitFor({ state: "visible", timeout: 5000 });
  await realClick(page, btn);
}

async function openDashboard(session) {
  await session.page.goto(session.url, { waitUntil: "networkidle" });
  await session.page.waitForSelector("#app .app-shell", { timeout: 10000 });
  await session.page.waitForSelector(".page-head", { timeout: 10000 });
  await session.page.evaluate(() => document.fonts?.ready);
  await session.page.waitForTimeout(400);
}

/** 真实用户路径：打开日期按钮 → 点快捷预设，并等待弹层关闭。 */
async function clickDatePreset(page, label) {
  await realClick(page, page.locator(".range-trigger").first());
  const shortcut = page.locator(".range-panel .shortcut-btn").filter({ hasText: label }).first();
  await shortcut.waitFor({ state: "visible", timeout: 5000 });
  await realClick(page, shortcut);
  await page.locator(".range-panel").waitFor({ state: "hidden", timeout: 5000 });
  await page.waitForTimeout(1500);
}

async function inputBarCount(page) {
  return page.locator(".chart-canvas svg").evaluate((svg, fill) => {
    return [...svg.querySelectorAll("path")].filter((path) => getComputedStyle(path).fill === fill)
      .length;
  }, INPUT_FILL);
}

/**
 * 从当前帧开始按 rAF 采样输入系列柱形的几何。`count` 是输入系列柱数，
 * `total` 是这些柱形的可见高度合计（横向柱时宽度才是增长方向，但 ECharts
 * 的 SVG path 包围盒高度也会随圆角/描边变化；同时记录 count 与 total，
 * 数据更新通常至少改变二者之一）。
 */
async function startSampling(page) {
  await page.evaluate(() => {
    window.__chartSampleId = (window.__chartSampleId ?? 0) + 1;
    window.__chartFrames = [];
    window.__chartSampling = true;
    const sampleId = window.__chartSampleId;
    const started = performance.now();
    const sample = () => {
      if (!window.__chartSampling || sampleId !== window.__chartSampleId) return;
      const svg = document.querySelector(".chart-canvas svg");
      let count = 0;
      let width = 0;
      let height = 0;
      if (svg) {
        for (const path of svg.querySelectorAll("path")) {
          let fill = "";
          try {
            fill = getComputedStyle(path).fill;
          } catch {
            continue;
          }
          if (fill !== "rgb(76, 141, 246)") continue;
          try {
            const box = path.getBBox();
            width += box.width;
            height += box.height;
            count += 1;
          } catch {
            /* 动画中节点可能被替换 */
          }
        }
      }
      window.__chartFrames.push({
        t: Math.round(performance.now() - started),
        count,
        width: Math.round(width * 100) / 100,
        height: Math.round(height * 100) / 100,
      });
      requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);
  });
}

async function stopSampling(page) {
  return page.evaluate(() => {
    window.__chartSampling = false;
    window.__chartSampleId = (window.__chartSampleId ?? 0) + 1;
    return window.__chartFrames ?? [];
  });
}

/** 只比较终态类别数的柱形几何；轴标签/布局过渡不算数据动画。 */
function summarize(frames) {
  const geometry = (frame) => Math.round((frame.width + frame.height) * 10) / 10;
  const first = frames[0] ?? { count: 0, width: 0, height: 0 };
  const last = frames.at(-1) ?? first;
  const finalCount = last.count;
  const finalGeometry = geometry(last);
  // 数据动画发生在类别数已切到终态之后：旧实现在这一帧直接等于终态。
  const onset = frames.findIndex((frame) => frame.count === finalCount && finalCount > 0);
  const after = onset >= 0 ? frames.slice(onset) : [];
  const settled = after.filter((frame) => frame.count === finalCount);
  const intermediate = settled.filter(
    (frame) => geometry(frame) > 0.1 && Math.abs(geometry(frame) - finalGeometry) > 0.1,
  );
  const distinct = new Set(settled.map((frame) => Math.round(geometry(frame) * 10) / 10)).size;
  const firstChanged = after[0] ?? first;
  return {
    initialCount: first.count,
    finalCount,
    firstChangedCount: firstChanged.count,
    firstChangedGeometry: geometry(firstChanged),
    finalGeometry,
    onset,
    intermediate: intermediate.length,
    distinct,
    frames: frames.length,
  };
}

/** 记录一条过渡：至少两个中间帧、至少三种不同几何、首变帧不能已是终态。 */
function recordTransition(ctx, contract, label, frames, expectedFinal) {
  const s = summarize(frames);
  const notJumped =
    Math.abs(s.firstChangedGeometry - s.finalGeometry) > 0.1 ||
    s.firstChangedCount !== s.finalCount;
  const pass =
    s.intermediate >= 2 && s.distinct >= 3 && notJumped && s.finalCount === expectedFinal;
  ctx.record(
    contract,
    label,
    pass,
    `初始=${s.initialCount} 首变=${s.firstChangedCount}/${s.firstChangedGeometry} 终态=${s.finalCount}/${s.finalGeometry} 中间帧=${s.intermediate} 不同几何=${s.distinct}/${s.frames} 首变非终态=${notJumped}`,
  );
  return { ...s, pass };
}

export async function uiChartTransition(ctx) {
  const contract = "ui_chart_transition_preserves_animation";
  const allFrames = {};
  const session = await ctx.open({
    fixture: "multi-category",
    viewport: { width: 1280, height: 820, label: "1280x820" },
    theme: "light",
  });
  try {
    await openDashboard(session);
    const page = session.page;
    // 默认维度可能是按日（上次偏好或内置默认）；先落到模型维度再开始比较。
    await clickSegment(page, "按模型");
    await page.waitForTimeout(900);
    await page.waitForFunction(
      () => document.querySelectorAll(".chart-canvas svg path").length > 0,
      { timeout: 5000 },
    );
    const initial = await inputBarCount(page);
    ctx.record(contract, "初始模型 30 类", initial === 30, `输入柱=${initial}`);

    // 模型 30→20（7d）
    await startSampling(page);
    await clickDatePreset(page, "7d");
    allFrames.model30to20 = await stopSampling(page);
    recordTransition(ctx, contract, "模型 30→20 保留中间帧", allFrames.model30to20, 20);

    // 模型 20→4（当日）
    await startSampling(page);
    await clickDatePreset(page, "当日");
    allFrames.model20to4 = await stopSampling(page);
    const model4 = recordTransition(
      ctx,
      contract,
      "模型 20→4 保留中间帧",
      allFrames.model20to4,
      4,
    );

    // 模型→按日（4 类 → 1 天）
    await startSampling(page);
    await clickSegment(page, "按时间");
    await page.waitForTimeout(1500);
    allFrames.modelToDay = await stopSampling(page);
    const modelToDay = recordTransition(
      ctx,
      contract,
      "模型→按日保留中间帧",
      allFrames.modelToDay,
      1,
    );
    ctx.record(
      contract,
      "维度切换后标题与类别数一致",
      (await page.locator(".chart-title").textContent()) === "使用趋势" &&
        modelToDay.finalCount === 1,
      `标题=${await page.locator(".chart-title").textContent()} 类别=${modelToDay.finalCount}`,
    );

    // 按日 1→7→14
    await startSampling(page);
    await clickDatePreset(page, "7d");
    allFrames.day1to7 = await stopSampling(page);
    recordTransition(ctx, contract, "按日 1→7 保留中间帧", allFrames.day1to7, 7);

    await startSampling(page);
    await clickDatePreset(page, "14d");
    allFrames.day7to14 = await stopSampling(page);
    recordTransition(ctx, contract, "按日 7→14 保留中间帧", allFrames.day7to14, 14);

    // 24h 空数据 → 7d 有值（组件重新挂载也不得首帧直接终态）。
    await clickDatePreset(page, "24h");
    await page.locator(".empty-state").waitFor({ state: "visible", timeout: 5000 });
    const emptyVisible = await page.locator(".empty-state").isVisible();
    const emptyCanvas = await page.locator(".chart-canvas svg").count();
    ctx.record(
      contract,
      "24h 无数据显示明确空状态（不残留旧图）",
      emptyVisible && emptyCanvas === 0,
      `空状态=${emptyVisible} 残留画布=${emptyCanvas}`,
    );

    await startSampling(page);
    await clickDatePreset(page, "7d");
    allFrames.emptyToValues = await stopSampling(page);
    recordTransition(ctx, contract, "空→有值保留中间帧", allFrames.emptyToValues, 7);

    // 连续切换：每次点击都是一次真实事件，最终只保留最新维度。
    await startSampling(page);
    await clickSegment(page, "按模型");
    await clickSegment(page, "按应用");
    await clickSegment(page, "按项目");
    await page.waitForTimeout(1000);
    allFrames.continuousSwitch = await stopSampling(page);
    const projectTitle = await page.locator(".chart-title").textContent();
    const projectCount = await inputBarCount(page);
    ctx.record(
      contract,
      "连续切换最终落到最新维度且图表完整",
      projectTitle === "项目用量" && projectCount === 20,
      `标题=${projectTitle} 输入柱=${projectCount}`,
    );

    // 真实容器 resize：宽度变化由 ResizeObserver 处理，不重建实例、不改数据。
    const body = page.locator(".chart-body");
    const wide = await body.evaluate((el) => ({ width: el.clientWidth }));
    await page.setViewportSize({ width: 980, height: 620 });
    await page.waitForTimeout(700);
    const narrow = await body.evaluate((el) => ({
      width: el.clientWidth,
      canvasWidth: el.querySelector("svg")?.getBoundingClientRect().width ?? 0,
    }));
    const narrowCount = await inputBarCount(page);
    ctx.record(
      contract,
      "真实容器 resize 自适应且不改变数据",
      narrow.width < wide.width &&
        narrow.canvasWidth <= narrow.width &&
        narrowCount === projectCount &&
        projectCount === 20,
      `宽度 ${wide.width}→${narrow.width} 画布=${narrow.canvasWidth.toFixed(1)} 类别=${narrowCount}`,
    );
    await page.setViewportSize({ width: 1280, height: 820 });
    await page.waitForTimeout(400);
    await ctx.shot(contract, session, "after-transitions");

    // 组件级卸载：切到空数据后重建，不应留下旧 svg 或页面错误。
    await clickDatePreset(page, "24h");
    await page.locator(".empty-state").waitFor({ state: "visible", timeout: 5000 });
    ctx.record(
      contract,
      "清空数据后卸载图表且无残留",
      (await page.locator(".chart-canvas svg").count()) === 0 &&
        session.pageErrors.length === 0,
      `画布=${await page.locator(".chart-canvas svg").count()} 页面错误=${session.pageErrors.length}`,
    );

    ctx.artifact("ui-chart-transition-frames.json", allFrames);
    ctx.finishSession(session);
  } finally {
    try {
      ctx.artifact("ui-chart-transition-frames.json", allFrames);
    } catch {
      /* 失败取证不阻断关闭 */
    }
    await session.close();
  }
}
