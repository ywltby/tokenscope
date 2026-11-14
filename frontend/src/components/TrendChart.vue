<script setup lang="ts">
// 设计系统 Task 4：趋势图接入统一图表主题（固定语义色/顺序，禁用默认
// 调色板），类别多时高度封顶并保留图内滚动。
// 聚合口径不变：分类与 series 仍由 buildBarChartData 单源生成（F03）。
//
// UX05（界面审查修复）：实例生命周期与数据更新分离——
//   - 初始化 / 同维度数据更新 / 维度切换 / 主题切换 / 卸载各自独立；
//   - 同维度数据更新走 setOption(merge)，不 dispose 重建、保留滚动位置；
//   - 维度切换完整替换 option（清掉旧轴/series/dataZoom）；
//   - 主题切换允许一次 dispose/init，保留外层滚动位置；
//   - resize 只 resize；卸载释放实例与观察器；
//   - 0 类别不初始化图表（父级另给明确空状态）；
//   - 所有 setOption 路径继续使用 SF01 的 DOM/textContent 安全 formatter。
import { computed, onActivated, onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts";
import { useTheme } from "../composables/theme";
import { useTokenColors } from "../composables/tokenColors";
import { fmtNum, type Group } from "../types";
import { buildBarChartData, tooltipLabels } from "../lib/chartData";
import { buildTooltipNode } from "../lib/chartTooltip";
import { chartTokens, POPOVER_BLUR_CSS } from "../styles/chartTheme";
import { t, useAppLocale } from "../lib/i18n";

const { locale } = useAppLocale();

/// 数值轴紧凑刻度（token 数）：中文按万/亿，英文按 K/M/B。
/// 只改刻度的显示单位，不改任何数值或统计口径。
function fmtCompact(v: number): string {
  const a = Math.abs(v);
  if (locale.value === "en-US") {
    if (a >= 1e9) return `${Number((v / 1e9).toFixed(1))}B`;
    if (a >= 1e6) return `${Number((v / 1e6).toFixed(1))}M`;
    if (a >= 1e3) return `${Number((v / 1e3).toFixed(1))}K`;
    return String(v);
  }
  if (a >= 1e8) return `${Number((v / 1e8).toFixed(1))}亿`;
  if (a >= 1e4) return `${Number((v / 1e4).toFixed(0))}万`;
  return String(v);
}

const props = defineProps<{ groups: Group[]; by: string }>();
const { mode } = useTheme();
const { overrides } = useTokenColors();
const legendItems = computed(() => chartTokens(mode.value, overrides.value).series);

const el = ref<HTMLDivElement | null>(null);
let chart: echarts.ECharts | null = null;
let observer: ResizeObserver | null = null;
/// 最近一次真正应用到 ECharts 的画布尺寸。ResizeObserver 对相同尺寸不再
/// 重复 resize——由数据更新改高度产生的回调不能打断刚开始的柱形动画。
/// v0.9.1（缺陷 A）：只记录**真实下发**的尺寸——隐藏容器量到的旧宽度
/// 不再当作已应用尺寸。
let appliedWidth = 0;
let appliedHeight = 0;
/// 布局/数据统一更新：同一轮内的数据、维度与尺寸变化只下发一次。
type PendingKind = "data" | "dimension" | "layout";
/// v0.9.1（缺陷 A）：KeepAlive 保留的页面隐藏时容器不可测量，任何
/// init/rebuild 都会得到 100px 默认画布。隐藏期间不再触碰实例，只记录
/// 待应用的**最新**意图（同类型覆盖：连续主题+语言 = 一次 rebuild）。
/// `rebuild` 同时快照隐藏前一刻的缩放范围，恢复时还原。
type PendingAction = { kind: PendingKind } | { kind: "rebuild"; zoom: ZoomRange };
let pending: PendingAction | null = null;
let disposed = false;

const isDay = computed(() => props.by === "day");
const titleText = computed(
  () =>
    ({
      day: t("dashboard.chartDay"),
      model: t("dashboard.chartModel"),
      agent: t("dashboard.chartAgent"),
      project: t("dashboard.chartProject"),
    })[props.by] ?? t("dashboard.chartFallback"),
);

/// 画布始终适配容器；类别增多只调整轴标签密度，不撑开页面。
function canvasHeight(count: number): number {
  return isDay.value ? 360 : Math.min(480, Math.max(320, count * 30 + 70));
}

function categoryLabelWidth(): number {
  const width = el.value?.clientWidth || 800;
  return props.by === "project" ? Math.min(180, width * 0.2) : Math.min(220, width * 0.25);
}

function formatCategoryLabel(value: string): string {
  return props.by === "project"
    ? echarts.format.truncateText(value, categoryLabelWidth(), "12px sans-serif", "…")
    : value;
}

/// 系统减少动态效果时允许取消动画；数值与交互不受影响。
function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

type ZoomRange = {
  start: number;
  end: number;
  startValue?: number;
  endValue?: number;
  rangeMode?: ["value", "value"];
};
function captureZoom(): ZoomRange {
  const option = chart?.getOption() as { dataZoom?: Partial<ZoomRange>[] } | undefined;
  const range = option?.dataZoom?.[0];
  return {
    start: range?.start ?? 0,
    end: range?.end ?? 100,
    ...(range?.startValue !== undefined && range?.endValue !== undefined
      ? {
          startValue: range.startValue,
          endValue: range.endValue,
          rangeMode: ["value", "value"] as ["value", "value"],
        }
      : {}),
  };
}
function resetZoom(): void {
  chart?.dispatchAction({ type: "dataZoom", start: 0, end: 100 });
}

function buildOption(zoom: ZoomRange = { start: 0, end: 100 }): echarts.EChartsOption {
  const t = chartTokens(mode.value, overrides.value);
  const groups = props.groups;
  // F03（计划 A4）：分类轴与全部 series 由同一份排序结果生成，标签与数值不错位
  const { categories, series } = buildBarChartData(groups, props.by);
  // 长标签：轴上省略，tooltip 用展示名（模型维度不能用压缩后的身份键）
  const full = tooltipLabels(groups, props.by);
  // SF01：tooltip 安全输出——返回 HTMLElement（DOM 分支），原始键只经
  // textNode 写入；模型/项目名可含任意字符（<>&"'、中文、长串）不注入。
  const tooltipFormatter = (params: unknown): HTMLElement => {
    const arr = (Array.isArray(params) ? params : [params]) as {
      dataIndex: number;
      name?: string;
      seriesName: string;
      value: unknown;
    }[];
    return buildTooltipNode({
      title: full[arr[0]?.dataIndex] ?? null,
      fallbackTitle: arr[0]?.name ?? null,
      entries: arr.map((p) => ({ seriesName: p.seriesName, value: Number(p.value ?? 0) })),
      formatValue: (v) => fmtNum(v),
    });
  };
  return {
    backgroundColor: "transparent",
    // 固定语义色（与图例/摘要文字对应），禁用 ECharts 默认调色板
    color: t.series.map((s) => s.color),
    // 尊重系统减少动态效果；数据与交互仍完整更新。
    animation: !prefersReducedMotion(),
    tooltip: {
      appendToBody: true,
      trigger: "axis",
      backgroundColor: t.tooltipBg,
      borderColor: t.tooltipBorder,
      borderWidth: 1,
      textStyle: { color: t.text },
      // UX01：ECharts HTML tooltip 走同一浮层配方（16px 模糊 + 12px 圆角）
      extraCssText: POPOVER_BLUR_CSS,
      valueFormatter: (v: unknown) => fmtNum(Number(v ?? 0)),
      formatter: tooltipFormatter,
    },
    // 图例移至卡片头部（HTML 圆点图例），画布内不再渲染
    grid: {
      left: 8,
      right: isDay.value ? 16 : 64,
      top: 16,
      bottom: isDay.value ? 56 : 44,
      containLabel: true,
    },
    xAxis: isDay.value
      ? {
          type: "category",
          data: categories,
          axisLine: { show: false },
          axisLabel: { color: t.textMuted, fontSize: 12, interval: "auto", hideOverlap: true },
        }
      : {
          type: "value",
          axisLabel: { color: t.textMuted, fontSize: 12, formatter: (v: number) => fmtCompact(v) },
          splitLine: { lineStyle: { color: t.splitLine } },
        },
    yAxis: isDay.value
      ? {
          type: "value",
          axisLabel: { color: t.textMuted, fontSize: 12, formatter: (v: number) => fmtCompact(v) },
          splitLine: { lineStyle: { color: t.splitLine } },
        }
      : {
          type: "category",
          inverse: true,
          // 标签密度随视口与缩放范围自适应；完整名称见 tooltip。
          data: categories,
          axisLine: { show: false },
          axisLabel: {
            color: t.textMuted,
            fontSize: 12,
            interval: "auto",
            hideOverlap: true,
            width: categoryLabelWidth(),
            formatter: formatCategoryLabel,
            overflow: "truncate",
          },
        },
    // 默认总览全部数据；类别轴筛选后，数值轴自动按可见数据重新定标。
    dataZoom: [
      {
        id: "category-slider",
        type: "slider",
        ...(isDay.value
          ? { xAxisIndex: 0, bottom: 4, right: 80, height: 20 }
          : { yAxisIndex: 0, top: 16, bottom: 44, right: 16, width: 16 }),
        ...zoom,
        filterMode: "filter",
        borderColor: "transparent",
        backgroundColor: "transparent",
        fillerColor: t.sliderFill,
        showDataShadow: false,
        handleStyle: { color: t.sliderHandle, borderColor: "transparent" },
        moveHandleStyle: { color: t.sliderHandle, opacity: 0.5 },
        emphasis: {
          handleStyle: { color: t.textSecondary, borderColor: "transparent" },
          moveHandleStyle: { color: t.textSecondary, opacity: 0.7 },
        },
        textStyle: { color: t.textMuted },
      },
      {
        id: "category-inside",
        type: "inside",
        ...(isDay.value ? { xAxisIndex: 0 } : { yAxisIndex: 0 }),
        ...zoom,
        filterMode: "filter",
        zoomOnMouseWheel: "ctrl",
        moveOnMouseWheel: false,
        preventDefaultMouseMove: false,
      },
    ],
    // 堆叠柱只让最上段（最后一个系列）带圆角：日维度柱顶 [4,4,0,0]，
    // 非日维度横向条尾 [0,4,4,0]（DESIGN.md §5 图表）
    series: series.map((s, i) => ({
      // 稳定 id：同维度数据更新按 id merge，不新建系列
      id: s.name,
      name: s.name,
      type: "bar",
      // 互斥四桶堆叠：每类一根柱、四段叠加，柱高 = 该类总 token
      //（2026-10-10 用户修订：口径恢复互斥四桶，堆叠相加即总量，不再并列）。
      stack: "tokens",
      // 非日维度横向堆叠，四类 token 全部保留
      barMaxWidth: 36,
      data: s.values,
      ...(i === series.length - 1
        ? { itemStyle: { borderRadius: isDay.value ? [4, 4, 0, 0] : [0, 4, 4, 0] } }
        : {}),
    })),
  };
}

/// v0.9.1（缺陷 A）：容器当前是否可测量。KeepAlive 隐藏的页面（display:none
/// 或未挂到布局树）clientWidth 为 0 且 offsetParent 为 null——任一成立都
/// 不能初始化 ECharts（只能得到 100px 默认画布）。恢复必须等重新可见且
/// 布局完成。clientWidth 是主判据：隐藏元素没有盒模型；offsetParent 兜底
/// 覆盖绝对定位但可见的极端场景。
function containerMeasurable(): boolean {
  const node = el.value;
  if (node == null) return false;
  return node.clientWidth > 0 || node.offsetWidth > 0 || node.offsetParent !== null;
}

/// 应用目标高度并量测实际尺寸；只在可测量容器上调用。
function applyHeight(): { width: number; height: number } {
  if (!el.value) return { width: appliedWidth, height: appliedHeight };
  const target = canvasHeight(props.groups.length);
  el.value.style.height = `${target}px`;
  el.value.style.minWidth = "0px";
  return {
    width: el.value.clientWidth || appliedWidth,
    height: el.value.clientHeight || target,
  };
}

function resizeIfNeeded(size: { width: number; height: number }): boolean {
  if (size.width === appliedWidth && size.height === appliedHeight) return false;
  const options: { width?: number; height?: number } = {};
  if (size.width > 0) options.width = size.width;
  if (size.height > 0) options.height = size.height;
  chart?.resize(options);
  appliedWidth = size.width;
  appliedHeight = size.height;
  return true;
}

/// 单次布局后更新：先应用高度与尺寸，确实变尺寸时先 resize，再下发数据。
/// 数据/维度/轴标签都在同一批内完成，ResizeObserver 不再补发第二次 resize。
/// v0.9.1（缺陷 A）：隐藏容器不触碰实例，只记录 pending。
function flushChart(kind: PendingKind): void {
  if (!el.value) return;
  if (!containerMeasurable()) {
    recordPending({ kind });
    return;
  }
  if (!chart) {
    initChart();
    return;
  }
  const size = applyHeight();
  const resized = resizeIfNeeded(size);
  if (kind === "dimension") {
    chart.setOption(buildOption(), { notMerge: true });
  } else if (kind === "data") {
    // 同维更新不要重新下发百分比范围，否则离散类别边界会再次取整而漂移。
    const option = buildOption();
    delete option.dataZoom;
    chart.setOption(option, { notMerge: false });
  } else if (resized && !isDay.value) {
    // 外部容器尺寸变化：轴标签宽度并入同一布局批次，不发独立空转 setOption。
    chart.setOption({
      yAxis: { axisLabel: { width: categoryLabelWidth(), formatter: formatCategoryLabel } },
    });
  }
}

/// v0.9.1（缺陷 A）：pending 合并语义——rebuild 覆盖一切（重建天然包含
/// 最新数据/维度/主题）；data/dimension 覆盖 layout；rebuild 之外取更高优先。
function recordPending(next: PendingAction): void {
  if (next.kind === "rebuild" || pending?.kind === "rebuild") {
    if (next.kind === "rebuild") {
      pending = next;
    } else if (pending?.kind !== "rebuild") {
      pending = next;
    }
    return;
  }
  if (next.kind === "layout" && pending != null) return;
  pending = next;
}

/// 首次初始化（0 类别不建实例；隐藏容器不建实例——记 pending 等恢复）。
/// 显式传容器尺寸：隐藏后恢复的容器在 init 一瞬间可能尚未完成布局，
/// 不传尺寸会被 ECharts 量成 100px（缺陷 A 的根因之一）。
function initChart(): void {
  if (!el.value || props.groups.length === 0) return;
  if (!containerMeasurable()) {
    recordPending({ kind: "layout" });
    return;
  }
  const size = applyHeight();
  chart = echarts.init(el.value, null, {
    renderer: "svg",
    width: size.width || undefined,
    height: size.height || undefined,
  });
  appliedWidth = size.width;
  appliedHeight = size.height;
  chart.setOption(buildOption(), { notMerge: true });
}

/// 同维度数据更新：merge（保留滚动位置、不重建实例）。
function updateData(): void {
  if (props.groups.length === 0) {
    chart?.dispose();
    chart = null;
    return;
  }
  if (!chart) {
    initChart();
    return;
  }
  flushChart("data");
}

/// 维度切换：完整替换（清掉旧轴 / series / dataZoom 残留）。
function switchDimension(): void {
  const viewport = el.value?.parentElement;
  if (viewport) {
    viewport.scrollTop = 0;
    viewport.scrollLeft = 0;
  }
  if (!chart) {
    initChart();
    return;
  }
  flushChart("dimension");
}

/// 主题/语言切换：允许一次 dispose/init，保留外层滚动位置与缩放范围。
/// v0.9.1（缺陷 A）：隐藏容器只记 pending（保留缩放快照），重新可见后
/// 按真实容器尺寸重建。
function rebuildForTheme(): void {
  if (!el.value || props.groups.length === 0) return;
  if (!containerMeasurable()) {
    recordPending({ kind: "rebuild", zoom: captureZoom() });
    return;
  }
  const zoom = captureZoom();
  chart?.dispose();
  chart = null;
  const size = applyHeight();
  chart = echarts.init(el.value, null, {
    renderer: "svg",
    width: size.width || undefined,
    height: size.height || undefined,
  });
  appliedWidth = size.width;
  appliedHeight = size.height;
  chart.setOption(buildOption(zoom), { notMerge: true });
}

/// v0.9.1（缺陷 A）：页面重新可见后的恢复。
/// - 有 pending：按记录的意图重建/刷新，全部使用**当下**真实容器尺寸；
/// - 无 pending：核对 ECharts 实际输出与容器尺寸，失配时 resize 校正
///   （隐藏期间的容器变宽不会触发 observer 回调，只能在这里补齐）。
///   校正只 resize、不 setOption，不打断可能正在进行的动画。
/// rAF 两帧等待 KeepAlive 重新插入后的布局完成（一帧可能还在旧布局）。
function maybeRestore(): void {
  if (disposed || !el.value) return;
  if (!containerMeasurable()) return;
  if (props.groups.length === 0) return;
  if (!chart) {
    // 从未初始化（隐藏期挂载或数据后到）→ 走 init 路径。
    initChart();
    pending = null;
    return;
  }
  const action = pending;
  pending = null;
  if (action?.kind === "rebuild") {
    const zoom = action.zoom;
    chart.dispose();
    chart = null;
    const size = applyHeight();
    chart = echarts.init(el.value, null, {
      renderer: "svg",
      width: size.width || undefined,
      height: size.height || undefined,
    });
    appliedWidth = size.width;
    appliedHeight = size.height;
    chart.setOption(buildOption(zoom), { notMerge: true });
    return;
  }
  // 无重建：尺寸核对（真实容器 vs 已应用 vs 实例输出）。
  // getWidth/getHeight 是 ECharts 实例**实际输出**的画布尺寸——隐藏期
  // init 出来的 100px 或 0 都会被它揭穿，不能只信 applied 缓存。
  // outputWidth <= 0 视为「实例未输出」的过渡态（布局未完成/测试 mock），
  // 不据它判失配；applied 缓存与容器不等仍然校正（缺陷 A 的缓存错位场景）。
  const size = applyHeight();
  const outputWidth = chart.getWidth();
  const outputHeight = chart.getHeight();
  const widthMismatch =
    size.width > 0 &&
    ((outputWidth > 0 && outputWidth !== size.width) || appliedWidth !== size.width);
  const heightMismatch =
    size.height > 0 &&
    ((outputHeight > 0 && outputHeight !== size.height) || appliedHeight !== size.height);
  if (action?.kind === "dimension") {
    flushChart("dimension");
    return;
  }
  if (action?.kind === "data") {
    flushChart("data");
    return;
  }
  if (widthMismatch || heightMismatch) {
    resizeIfNeeded(size);
    if (!isDay.value) {
      chart.setOption({
        yAxis: { axisLabel: { width: categoryLabelWidth(), formatter: formatCategoryLabel } },
      });
    }
  }
}

onMounted(() => {
  disposed = false;
  initChart();
  observer = new ResizeObserver(() => {
    if (disposed || !el.value || !chart) return;
    // v0.9.1（缺陷 A）：隐藏 → 可见的转变（KeepAlive 回页不触发 observer
    // 尺寸变化之外的场景，但容器从 display:none 恢复时 clientWidth 从 0
    // 变为真实值会触发）。可见性转变交给 maybeRestore 处理 pending 与
    // 尺寸失配；稳定可见态走原布局批次逻辑。
    if (!containerMeasurable()) return;
    maybeRestore();
    if (!chart) return;
    const width = el.value.clientWidth || appliedWidth;
    const height = el.value.clientHeight || appliedHeight;
    if (width === appliedWidth && height === appliedHeight) return;
    flushChart("layout");
  });
  if (el.value) observer.observe(el.value);
});

onBeforeUnmount(() => {
  disposed = true;
  observer?.disconnect();
  observer = null;
  chart?.dispose();
  chart = null;
});

/// v0.9.1（缺陷 A）：KeepAlive 页面切换回汇总页时恢复。
/// rAF 两帧后再量：KeepAlive 重新插入 DOM 的同一帧布局未必完成。
onActivated(() => {
  if (disposed) return;
  requestAnimationFrame(() => requestAnimationFrame(() => maybeRestore()));
});

// 数据更新与维度切换分离：只监听 by/mode 会漏掉新数据，只在每次数据更新
// 时重建实例又会丢失滚动位置 与实例状态——两者都不是这里的做法。
// 合并为单一批次判断：同一次 flush 内若维度变了就完整替换（不先 merge 再
// 替换产生中间态），否则按同维度数据更新 merge（保留滚动位置）。
watch(
  () => [props.groups, props.by] as const,
  (cur, prev) => {
    const curBy = cur[1];
    const prevBy = prev ? prev[1] : curBy;
    if (curBy !== prevBy) switchDimension();
    else updateData();
  },
  { flush: "post" },
);
watch(mode, () => rebuildForTheme());
// i18n：语言切换后图例/坐标轴文案要跟着换——一次完整重建最省事且不会留下
// 旧语言的系列名（键与颜色不变，只有显示名变）。
watch(locale, () => rebuildForTheme());
// 仅更新调色板，不重建实例或覆盖 dataZoom。
watch(overrides, () => chart?.setOption({ color: legendItems.value.map((s) => s.color) }));
</script>

<template>
  <section class="ts-card trend-card">
    <div class="chart-head">
      <span class="chart-title">{{ titleText }}</span>
      <!-- 圆点图例（与图表系列同源 token，DESIGN.md §5 图表） -->
      <div class="chart-legend" aria-hidden="true">
        <span v-for="s in legendItems" :key="s.key" class="legend-item">
          <span class="legend-dot" :style="{ background: s.color }" />
          <span class="legend-label">{{ s.label }}</span>
        </span>
      </div>
    </div>
    <!-- 任务 8（硬约束）：绘图区实色衬底——ECharts 背景透明但容器 .ts-card-solid -->
    <div
      class="chart-body ts-card-solid"
      :class="{ 'chart-body-time': isDay }"
      tabindex="0"
      role="region"
      :aria-label="t('dashboard.chartZoomAria', { title: titleText })"
    >
      <div
        ref="el"
        class="chart-canvas"
        style="width: 100%; height: 320px"
        role="img"
        :aria-label="titleText"
      />
      <button class="ts-btn ts-btn-sm chart-reset" type="button" @click="resetZoom">
        {{ t("dashboard.chartResetZoom") }}
      </button>
    </div>
  </section>
</template>

<style scoped>
/* 绘图区实色衬底容器（.ts-card-solid 提供背景与圆角） */
.chart-body {
  position: relative;
  min-width: 0;
  overflow: hidden;
  padding: var(--ts-space-2) var(--ts-space-3);
  margin-top: var(--ts-space-2);
}
.chart-reset {
  position: absolute;
  right: 4px;
  bottom: 12px;
}
.chart-body-time .chart-reset {
  right: 12px;
  bottom: 8px;
}
.chart-body:focus-visible {
  outline: 2px solid var(--ts-accent);
  outline-offset: 2px;
}

.chart-head {
  display: flex;
  align-items: baseline;
  gap: var(--ts-space-3);
  margin-bottom: var(--ts-space-2);
  flex-wrap: wrap;
}

/* 卡片标题 17px/600（DESIGN.md §3） */
.chart-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.chart-legend {
  display: inline-flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--ts-space-3);
  margin-left: auto;
}

.legend-item {
  display: inline-flex;
  align-items: center;
  gap: var(--ts-space-1);
  font-size: 12px;
  color: var(--ts-text-secondary);
}

.legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.legend-label {
  white-space: nowrap;
}
</style>
