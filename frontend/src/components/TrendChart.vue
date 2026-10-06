<script setup lang="ts">
// 设计系统 Task 4：趋势图接入统一图表主题（固定语义色/顺序，禁用默认
// 调色板），类别多时高度封顶 + 图内滚动，并提供等价文字摘要（可访问性）。
// 聚合口径不变：分类与 series 仍由 buildBarChartData 单源生成（F03）。
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts";
import { useTheme } from "../composables/theme";
import { fmtNum, type Group } from "../types";
import { buildBarChartData, chartStateText, chartSummaryLines, fullLabels } from "../lib/chartData";
import { chartTokens } from "../styles/chartTheme";

/// 数值轴紧凑刻度（token 数：万/亿）
function fmtCompact(v: number): string {
  const a = Math.abs(v);
  if (a >= 1e8) return `${Number((v / 1e8).toFixed(1))}亿`;
  if (a >= 1e4) return `${Number((v / 1e4).toFixed(0))}万`;
  return String(v);
}

const props = defineProps<{ groups: Group[]; by: string }>();
const { mode } = useTheme();

const el = ref<HTMLDivElement | null>(null);
let chart: echarts.ECharts | null = null;
let observer: ResizeObserver | null = null;

const isDay = computed(() => props.by === "day");
const stateText = computed(() => chartStateText(props.groups.length));
const summaryLines = computed(() => chartSummaryLines(props.groups, props.by));
const showSummary = ref(false);
const titleText = computed(() =>
  isDay.value ? "每日 token 趋势（堆叠）" : "token 分布（按用量排序）",
);

/// 非日维度为横向条形：高度封顶 560px，超出部分图内滚动（不无限撑高页面）。
const MAX_H = 560;
function canvasHeight(count: number): number {
  return isDay.value ? 320 : Math.min(MAX_H, Math.max(320, count * 34 + 70));
}

function render(): void {
  if (!el.value) return;
  const t = chartTokens(mode.value);
  if (chart) {
    chart.dispose();
    chart = null;
  }
  const groups = props.groups;
  const height = canvasHeight(groups.length);
  el.value.style.height = `${height}px`;
  chart = echarts.init(el.value, null);
  // F03（计划 A4）：分类轴与全部 series 由同一份排序结果生成，标签与数值不错位
  const { categories, series } = buildBarChartData(groups, props.by);
  // 长标签：轴上省略，tooltip 用完整原始键
  const full = fullLabels(groups, props.by);
  const tooltipFormatter = (params: unknown): string => {
    const arr = (Array.isArray(params) ? params : [params]) as {
      dataIndex: number;
      name?: string;
      seriesName: string;
      value: unknown;
    }[];
    const title = full[arr[0]?.dataIndex] ?? arr[0]?.name ?? "";
    const lines = arr.map((p) => `${p.seriesName} ${fmtNum(Number(p.value ?? 0))}`);
    return [title, ...lines].join("<br/>");
  };
  // 滚动：横向条形 >14 类启用 y 轴 dataZoom；日维度 >60 天启用 x 轴缩放
  const dataZoom = isDay.value
    ? categories.length > 60
      ? [{ type: "inside", xAxisIndex: 0 }]
      : undefined
    : groups.length > 14
      ? [
          { type: "inside", yAxisIndex: 0 },
          { type: "slider", yAxisIndex: 0, right: 0, width: 14 },
        ]
      : undefined;
  chart.setOption({
    backgroundColor: "transparent",
    // 固定语义色（与图例/摘要文字对应），禁用 ECharts 默认调色板
    color: t.series.map((s) => s.color),
    tooltip: {
      trigger: "axis",
      backgroundColor: t.tooltipBg,
      borderColor: t.tooltipBorder,
      borderWidth: 1,
      textStyle: { color: t.text },
      valueFormatter: (v: unknown) => fmtNum(Number(v ?? 0)),
      formatter: tooltipFormatter,
    },
    legend: { top: 0, textStyle: { color: t.legendText } },
    grid: { left: 8, right: 16, top: 32, bottom: 8, containLabel: true },
    xAxis: isDay.value
      ? {
          type: "category",
          data: categories,
          axisLine: { lineStyle: { color: t.border } },
          axisLabel: { color: t.textSecondary },
        }
      : {
          type: "value",
          axisLabel: { color: t.textMuted, formatter: (v: number) => fmtCompact(v) },
          splitLine: { lineStyle: { color: t.splitLine } },
        },
    yAxis: isDay.value
      ? {
          type: "value",
          axisLabel: { color: t.textMuted, formatter: (v: number) => fmtCompact(v) },
          splitLine: { lineStyle: { color: t.splitLine } },
        }
      : {
          type: "category",
          // interval 0 强制每个项目都显示名称；超长省略，完整值见 tooltip
          data: categories,
          axisLine: { lineStyle: { color: t.border } },
          axisLabel: { color: t.textSecondary, interval: 0, width: 220, overflow: "truncate" },
        },
    dataZoom,
    series: series.map((s) => ({
      name: s.name,
      type: "bar",
      stack: "tokens",
      // by != day 时换为普通并列条形，取值函数相同
      barMaxWidth: 36,
      data: s.values,
    })),
  });
}

onMounted(() => {
  render();
  observer = new ResizeObserver(() => chart?.resize());
  if (el.value) observer.observe(el.value);
});

onBeforeUnmount(() => {
  observer?.disconnect();
  chart?.dispose();
  chart = null;
});

// 主题切换 → dispose 重建（不残留旧主题的轴/文字颜色）
watch(() => [props.groups, props.by, mode.value], render);
</script>

<template>
  <section class="trend-chart">
    <div class="chart-head">
      <span class="chart-title">{{ titleText }}</span>
      <span class="chart-state">{{ stateText }}</span>
      <button
        type="button"
        class="summary-toggle ts-focusable"
        :aria-expanded="showSummary"
        @click="showSummary = !showSummary"
      >
        数据摘要
      </button>
    </div>
    <div v-if="showSummary" class="chart-summary" role="region" aria-label="图表数据摘要">
      <div v-for="l in summaryLines" :key="l" class="summary-line ts-num">{{ l }}</div>
    </div>
    <div
      ref="el"
      class="chart-canvas"
      style="width: 100%; height: 320px"
      role="img"
      :aria-label="`趋势图：${titleText}，${stateText}`"
    />
  </section>
</template>

<style scoped>
.chart-head {
  display: flex;
  align-items: baseline;
  gap: var(--ts-space-3);
  margin-bottom: var(--ts-space-2);
}

.chart-title {
  font-size: 15px;
  font-weight: 650;
  color: var(--ts-text);
}

.chart-state {
  font-size: 12px;
  color: var(--ts-text-muted);
}

.summary-toggle {
  margin-left: auto;
  border: 1px solid var(--ts-border);
  background: var(--ts-surface-solid);
  color: var(--ts-text-secondary);
  font: inherit;
  font-size: 12px;
  border-radius: var(--ts-radius-control);
  padding: 2px var(--ts-space-2);
  cursor: pointer;
}

.summary-toggle:hover {
  color: var(--ts-text);
  border-color: var(--ts-border-strong);
}

.chart-summary {
  border: 1px solid var(--ts-border);
  border-radius: var(--ts-radius);
  background: var(--ts-surface-solid);
  padding: var(--ts-space-2) var(--ts-space-3);
  margin-bottom: var(--ts-space-2);
  max-height: 220px;
  overflow-y: auto;
}

.summary-line {
  font-size: 12px;
  line-height: 1.6;
  color: var(--ts-text-secondary);
  font-family: var(--ts-font-mono);
}
</style>
