// 非时间横向柱状图（模型 / 项目 / 应用）的统一标签测量与左侧空间预留。
//
// ECharts 的 grid.containLabel 在类目数超过 40 行时会按 step = ceil(N / 40) 抽样
// 估算标签宽度，导致长短名称混排时漏掉奇数行等长标签并从画布左边缘裁切。
// 本模块按实际轴标签字体/字号逐一测量当前缩放范围内全部可见标签（支持测量缓存，
// 禁止抽样），显式计算 grid.left 与末尾省略预算。

export const AXIS_LABEL_FONT_SIZE = 12;
export const AXIS_LABEL_DEFAULT_FONT_FAMILY = "sans-serif";
export const AXIS_LABEL_MARGIN = 8;
export const GRID_LEFT_OUTER = 12;
export const GRID_TOP_HORIZONTAL = 16;
export const GRID_BOTTOM_HORIZONTAL = 64;
export const GRID_RIGHT_HORIZONTAL = 64;
export const MIN_PLOT_WIDTH = 220;
export const LABEL_WIDTH_SAFETY_PAD = 4;
export const MAX_LABEL_BUDGET_CAP = 260;
export const LABEL_BUDGET_RATIO = 0.28;
export const MIN_LABEL_BUDGET = 80;
const MAX_MEASURE_CACHE_ENTRIES = 4096;

export interface ZoomBounds {
  start: number;
  end: number;
  startValue?: number;
  endValue?: number;
}

export interface VisibleCategoryRange {
  startIndex: number;
  endIndex: number;
  visibleCount: number;
}

export type TextMeasurer = (text: string, font: string) => number;

export interface TruncatedLabel {
  text: string;
  width: number;
  truncated: boolean;
}

export interface CategoryLabelLayout {
  range: VisibleCategoryRange;
  height: number;
  fontFamily: string;
  font: string;
  maxLabelWidth: number;
  maxVisibleWidth: number;
  gridLeft: number;
  axisLabelWidth: number;
  formatter: (value: string) => string;
}

let measureCanvasCtx: CanvasRenderingContext2D | null | undefined;
const widthCache = new Map<string, number>();

function getMeasureContext(): CanvasRenderingContext2D | null {
  if (measureCanvasCtx !== undefined) return measureCanvasCtx;
  if (typeof document === "undefined") {
    measureCanvasCtx = null;
    return null;
  }
  try {
    const canvas = document.createElement("canvas");
    measureCanvasCtx = canvas.getContext("2d");
  } catch {
    measureCanvasCtx = null;
  }
  return measureCanvasCtx;
}

function fallbackMeasureWidth(text: string, font: string): number {
  const sizeMatch = /(\d+(?:\.\d+)?)px/.exec(font);
  const size = sizeMatch ? Number.parseFloat(sizeMatch[1]) : AXIS_LABEL_FONT_SIZE;
  let width = 0;
  for (const ch of text) {
    const code = ch.codePointAt(0) ?? 0;
    if (code > 0x2e7f) {
      width += size;
    } else if ("ilI1.,:;!'| ".includes(ch)) {
      width += size * 0.34;
    } else if (ch >= "A" && ch <= "Z") {
      width += size * 0.68;
    } else if ("mwMW@%#".includes(ch)) {
      width += size * 0.85;
    } else {
      width += size * 0.56;
    }
  }
  return width;
}

export function clearLabelMeasureCache(): void {
  widthCache.clear();
}

export function resolveAxisFontFamily(el: HTMLElement | null): string {
  if (el && typeof window !== "undefined" && typeof window.getComputedStyle === "function") {
    const family = window.getComputedStyle(el).fontFamily?.trim();
    if (family) return family;
  }
  return AXIS_LABEL_DEFAULT_FONT_FAMILY;
}

export function buildAxisFont(
  fontFamily: string = AXIS_LABEL_DEFAULT_FONT_FAMILY,
  fontSize: number = AXIS_LABEL_FONT_SIZE,
): string {
  return `${fontSize}px ${fontFamily}`;
}

/// 按实际字体与字号测量单个文本宽度（带上限缓存，避免重复 measureText 分配）。
export function measureAxisLabelWidth(text: string, font: string = buildAxisFont()): number {
  const cacheKey = `${font}\0${text}`;
  const cached = widthCache.get(cacheKey);
  if (cached !== undefined) return cached;

  const ctx = getMeasureContext();
  let width: number;
  if (ctx) {
    ctx.font = font;
    width = ctx.measureText(text).width;
  } else {
    width = fallbackMeasureWidth(text, font);
  }
  if (widthCache.size >= MAX_MEASURE_CACHE_ENTRIES) {
    widthCache.clear();
  }
  widthCache.set(cacheKey, width);
  return width;
}

/// 空间足够时返回原全名；空间不足时保留开头、末尾加省略号「…」，绝不从左侧裁切。
export function truncateLabelEnd(
  text: string,
  maxWidth: number,
  font: string = buildAxisFont(),
  measure: TextMeasurer = measureAxisLabelWidth,
): TruncatedLabel {
  const fullWidth = measure(text, font);
  if (fullWidth <= maxWidth) {
    return { text, width: fullWidth, truncated: false };
  }
  const ellipsis = "…";
  const ellipsisWidth = measure(ellipsis, font);
  if (ellipsisWidth >= maxWidth) {
    return { text: ellipsis, width: ellipsisWidth, truncated: true };
  }
  const chars = Array.from(text);
  let lo = 1;
  let hi = chars.length - 1;
  let best = ellipsis;
  let bestWidth = ellipsisWidth;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const candidate = `${chars.slice(0, mid).join("").trimEnd()}${ellipsis}`;
    const candidateWidth = measure(candidate, font);
    if (candidateWidth <= maxWidth) {
      best = candidate;
      bestWidth = candidateWidth;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return { text: best, width: bestWidth, truncated: true };
}

/// 计算当前缩放范围内实际可见的离散类目闭区间 `[startIndex, endIndex]` 与数量。
export function visibleCategoryRange(count: number, zoom: ZoomBounds): VisibleCategoryRange {
  if (count <= 0) {
    return { startIndex: 0, endIndex: -1, visibleCount: 0 };
  }
  const last = count - 1;
  const rawStart = zoom.startValue ?? Math.round((last * zoom.start) / 100);
  const rawEnd = zoom.endValue ?? Math.round((last * zoom.end) / 100);
  const lo = Math.ceil(Math.min(rawStart, rawEnd));
  const hi = Math.floor(Math.max(rawStart, rawEnd));
  // 数据变少时 ECharts 将 value 窗口整体平移进新 extent（保留跨度），不是分别截断两端。
  const visibleCount = Math.min(count, Math.max(0, hi - lo + 1));
  if (visibleCount === 0) {
    return { startIndex: 0, endIndex: -1, visibleCount: 0 };
  }
  let startIndex: number;
  let endIndex: number;
  if (hi > last) {
    endIndex = last;
    startIndex = Math.max(0, last - visibleCount + 1);
  } else if (lo < 0) {
    startIndex = 0;
    endIndex = Math.min(last, visibleCount - 1);
  } else {
    startIndex = lo;
    endIndex = hi;
  }
  return { startIndex, endIndex, visibleCount };
}

/// 计算容器宽度允许的最大标签预算（在保证绘图区和右侧滑块不挤占的前提下尽量展示全名）。
export function computeMaxLabelBudget(containerWidth: number): number {
  const width = containerWidth > 0 ? containerWidth : 800;
  const availableForLabel =
    width - GRID_RIGHT_HORIZONTAL - GRID_LEFT_OUTER - AXIS_LABEL_MARGIN - MIN_PLOT_WIDTH;
  const ratioBudget = Math.floor(width * LABEL_BUDGET_RATIO);
  return Math.max(
    MIN_LABEL_BUDGET,
    Math.min(MAX_LABEL_BUDGET_CAP, ratioBudget, Math.max(MIN_LABEL_BUDGET, availableForLabel)),
  );
}

/// 逐一测量当前可见范围内的全部标签（严禁抽样），统一产出画布高度、左侧预留与省略函数。
export function computeCategoryLabelLayout(
  categories: readonly string[],
  zoom: ZoomBounds,
  containerWidth: number,
  fontFamily: string = AXIS_LABEL_DEFAULT_FONT_FAMILY,
  measure: TextMeasurer = measureAxisLabelWidth,
): CategoryLabelLayout {
  const range = visibleCategoryRange(categories.length, zoom);
  const height = Math.max(320, range.visibleCount * 24 + 80);
  const font = buildAxisFont(fontFamily, AXIS_LABEL_FONT_SIZE);
  const maxLabelWidth = computeMaxLabelBudget(containerWidth);

  let maxVisibleWidth = 0;
  for (let i = range.startIndex; i <= range.endIndex; i++) {
    const label = categories[i];
    if (label == null) continue;
    const truncated = truncateLabelEnd(label, maxLabelWidth, font, measure);
    if (truncated.width > maxVisibleWidth) {
      maxVisibleWidth = truncated.width;
    }
  }

  const reservedLabelWidth =
    maxVisibleWidth > 0 ? Math.ceil(maxVisibleWidth) + LABEL_WIDTH_SAFETY_PAD : 0;
  const gridLeft = GRID_LEFT_OUTER + reservedLabelWidth + AXIS_LABEL_MARGIN;
  const axisLabelWidth = Math.ceil(maxLabelWidth) + LABEL_WIDTH_SAFETY_PAD;
  const formatter = (value: string): string =>
    truncateLabelEnd(value, maxLabelWidth, font, measure).text;

  return {
    range,
    height,
    fontFamily,
    font,
    maxLabelWidth,
    maxVisibleWidth,
    gridLeft,
    axisLabelWidth,
    formatter,
  };
}
