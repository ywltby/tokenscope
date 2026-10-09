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
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts";
import { useTheme } from "../composables/theme";
import { useTokenColors } from "../composables/tokenColors";
import { fmtNum, type Group } from "../types";
import { buildBarChartData, fullLabels } from "../lib/chartData";
import { buildTooltipNode } from "../lib/chartTooltip";
import { chartTokens, POPOVER_BLUR_CSS } from "../styles/chartTheme";

/// 数值轴紧凑刻度（token 数：万/亿）
function fmtCompact(v: number): string {
  const a = Math.abs(v);
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

const isDay = computed(() => props.by === "day");
const titleText = computed(
  () =>
    ({ day: "使用趋势", model: "模型用量", agent: "应用用量", project: "项目用量" })[props.by] ??
    "用量分布",
);

/// 画布始终适配容器；类别增多只调整轴标签密度，不撑开页面。
function canvasHeight(count: number): number {
  return isDay.value ? 360 : Math.min(480, Math.max(320, count * 30 + 70));
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
  // 长标签：轴上省略，tooltip 用完整原始键
  const full = fullLabels(groups, props.by);
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
            width: Math.min(220, (el.value?.clientWidth || 800) * 0.25),
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
        fillerColor: t.separator,
        showDataShadow: false,
        handleStyle: { color: t.textMuted, borderColor: "transparent" },
        moveHandleStyle: { color: t.textMuted, opacity: 0.5 },
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

function applyHeight(): void {
  if (!el.value) return;
  el.value.style.height = `${canvasHeight(props.groups.length)}px`;
  el.value.style.minWidth = "0px";
}

/// 首次初始化（0 类别不建实例）。
function initChart(): void {
  if (!el.value || props.groups.length === 0) return;
  applyHeight();
  chart = echarts.init(el.value, null, { renderer: "svg" });
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
  applyHeight();
  chart.resize();
  // 同维更新不要重新下发百分比范围，否则离散类别边界会再次取整而漂移。
  const option = buildOption();
  delete option.dataZoom;
  chart.setOption(option, { notMerge: false });
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
  applyHeight();
  chart.resize();
  chart.setOption(buildOption(), { notMerge: true });
}

/// 主题切换：允许一次 dispose/init，保留外层滚动位置。
function rebuildForTheme(): void {
  if (!el.value || props.groups.length === 0) return;
  const zoom = captureZoom();
  chart?.dispose();
  chart = null;
  applyHeight();
  chart = echarts.init(el.value, null, { renderer: "svg" });
  chart.setOption(buildOption(zoom), { notMerge: true });
}

onMounted(() => {
  initChart();
  observer = new ResizeObserver(() => {
    chart?.resize();
    if (chart && !isDay.value)
      chart.setOption({
        yAxis: { axisLabel: { width: Math.min(220, (el.value?.clientWidth || 800) * 0.25) } },
      });
  });
  if (el.value) observer.observe(el.value);
});

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
  chart?.dispose();
  chart = null;
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
);
watch(mode, () => rebuildForTheme());
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
      :aria-label="`${titleText}，拖动范围滑块缩放，Ctrl 加滚轮缩放`"
    >
      <div
        ref="el"
        class="chart-canvas"
        style="width: 100%; height: 320px"
        role="img"
        :aria-label="titleText"
      />
      <button class="chart-reset" type="button" @click="resetZoom">显示全部</button>
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
  width: 64px;
  height: 28px;
  color: var(--ts-text-secondary);
  background: transparent;
  border: none;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
  padding: 4px;
}
.chart-body-time .chart-reset {
  right: 12px;
  bottom: 8px;
}
.chart-reset:hover {
  color: var(--ts-text);
  background: var(--ts-fill-hover);
  border-radius: var(--ts-radius-control);
}
.chart-reset:focus-visible {
  outline: 2px solid var(--ts-accent);
  outline-offset: 2px;
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
