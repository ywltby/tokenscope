<script setup lang="ts">
// 设计系统 Task 4：趋势图接入统一图表主题（固定语义色/顺序，禁用默认
// 调色板），类别多时高度封顶并保留图内滚动。
// 聚合口径不变：分类与 series 仍由 buildBarChartData 单源生成（F03）。
//
// UX05（界面审查修复）：实例生命周期与数据更新分离——
//   - 初始化 / 同维度数据更新 / 维度切换 / 主题切换 / 卸载各自独立；
//   - 同维度数据更新走 setOption(merge)，不 dispose 重建、保留 zoom；
//   - 维度切换完整替换 option（清掉旧轴/series/dataZoom）；
//   - 主题切换允许一次 dispose/init，并恢复同维度适用的 zoom；
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
const titleText = computed(() =>
  isDay.value ? "每日 token 趋势（堆叠）" : "token 分布（按用量排序）",
);

/// 非日维度为横向条形：高度封顶 560px，超出部分图内滚动（不无限撑高页面）。
const MAX_H = 560;
function canvasHeight(count: number): number {
  return isDay.value ? 320 : Math.min(MAX_H, Math.max(320, count * 34 + 70));
}

function buildDataZoom(
  categories: string[],
): { type: "inside" | "slider"; [k: string]: unknown }[] | undefined {
  if (isDay.value) {
    return categories.length > 60 ? [{ type: "inside", xAxisIndex: 0 }] : undefined;
  }
  return props.groups.length > 14
    ? [
        { type: "inside", yAxisIndex: 0 },
        { type: "slider", yAxisIndex: 0, right: 0, width: 14 },
      ]
    : undefined;
}

/// 读取当前 zoom（用于主题重建后恢复同维度适用的缩放）。
function captureZoom(): { start?: number; end?: number }[] | null {
  const c = chart as unknown as { getOption?: () => unknown } | null;
  if (!c || typeof c.getOption !== "function") return null;
  try {
    const opt = c.getOption() as { dataZoom?: { start?: number; end?: number }[] };
    if (!Array.isArray(opt?.dataZoom) || opt.dataZoom.length === 0) return null;
    return opt.dataZoom.map((z) => ({ start: z.start, end: z.end }));
  } catch {
    return null;
  }
}

function buildOption(zoom?: { start?: number; end?: number }[] | null): echarts.EChartsOption {
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
  let dataZoom = buildDataZoom(categories);
  if (dataZoom && zoom) {
    dataZoom = dataZoom.map((d, i) => ({ ...d, ...(zoom[i] ?? {}) }));
  }
  return {
    backgroundColor: "transparent",
    // 固定语义色（与图例/摘要文字对应），禁用 ECharts 默认调色板
    color: t.series.map((s) => s.color),
    tooltip: {
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
    grid: { left: 8, right: 16, top: 16, bottom: 8, containLabel: true },
    xAxis: isDay.value
      ? {
          type: "category",
          data: categories,
          axisLine: { show: false },
          axisLabel: { color: t.textMuted, fontSize: 12 },
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
          // interval 0 强制每个项目都显示名称；超长省略，完整值见 tooltip
          data: categories,
          axisLine: { show: false },
          axisLabel: {
            color: t.textMuted,
            fontSize: 12,
            interval: 0,
            width: 220,
            overflow: "truncate",
          },
        },
    dataZoom,
    // 堆叠柱只让最上段（最后一个系列）带圆角：日维度柱顶 [4,4,0,0]，
    // 非日维度横向条尾 [0,4,4,0]（DESIGN.md §5 图表）
    series: series.map((s, i) => ({
      // 稳定 id：同维度数据更新按 id merge，不新建系列
      id: s.name,
      name: s.name,
      type: "bar",
      stack: "tokens",
      // by != day 时换为普通并列条形，取值函数相同
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
}

/// 首次初始化（0 类别不建实例）。
function initChart(): void {
  if (!el.value || props.groups.length === 0) return;
  applyHeight();
  chart = echarts.init(el.value, null);
  chart.setOption(buildOption(), { notMerge: true });
}

/// 同维度数据更新：merge（保留 zoom、不重建实例）。
function updateData(): void {
  if (props.groups.length === 0) return;
  if (!chart) {
    initChart();
    return;
  }
  applyHeight();
  chart.resize();
  chart.setOption(buildOption(), { notMerge: false });
}

/// 维度切换：完整替换（清掉旧轴 / series / dataZoom 残留）。
function switchDimension(): void {
  if (!chart) {
    initChart();
    return;
  }
  applyHeight();
  chart.resize();
  chart.setOption(buildOption(), { notMerge: true });
}

/// 主题切换：允许一次 dispose/init，恢复同维度适用的 zoom。
function rebuildForTheme(): void {
  if (!el.value || props.groups.length === 0) return;
  const zoom = captureZoom();
  chart?.dispose();
  chart = null;
  applyHeight();
  chart = echarts.init(el.value, null);
  chart.setOption(buildOption(zoom), { notMerge: true });
}

onMounted(() => {
  initChart();
  observer = new ResizeObserver(() => chart?.resize());
  if (el.value) observer.observe(el.value);
});

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
  chart?.dispose();
  chart = null;
});

// 数据更新与维度切换分离：只监听 by/mode 会漏掉新数据，只在每次数据更新
// 时重建实例又会丢失 zoom 与实例状态——两者都不是这里的做法。
// 合并为单一批次判断：同一次 flush 内若维度变了就完整替换（不先 merge 再
// 替换产生中间态），否则按同维度数据更新 merge（保留 zoom）。
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
    <div class="chart-body ts-card-solid">
      <div
        ref="el"
        class="chart-canvas"
        style="width: 100%; height: 320px"
        role="img"
        :aria-label="`趋势图：${titleText}`"
      />
    </div>
  </section>
</template>

<style scoped>
/* 绘图区实色衬底容器（.ts-card-solid 提供背景与圆角） */
.chart-body {
  padding: var(--ts-space-2) var(--ts-space-3);
  margin-top: var(--ts-space-2);
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
