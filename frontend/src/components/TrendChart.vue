<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts";
import { useTheme } from "../composables/theme";
import { fmtNum, type Group } from "../types";
import { buildBarChartData } from "../lib/chartData";

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

function render(): void {
  if (!el.value) return;
  const dark = mode.value === "dark";
  if (chart) {
    chart.dispose();
    chart = null;
  }
  const groups = props.groups;
  const isDay = props.by === "day";
  // 非日维度为横向条形：按条目数动态撑高，保证每个项目一行且标签完整
  const height = isDay ? 320 : Math.max(320, groups.length * 34 + 70);
  el.value.style.height = `${height}px`;
  chart = echarts.init(el.value, dark ? "dark" : undefined);
  // F03（计划 A4）：分类轴与全部 series 由同一份排序结果生成，标签与数值不错位
  const { categories, series } = buildBarChartData(groups, props.by);
  chart.setOption({
    backgroundColor: "transparent",
    tooltip: {
      trigger: "axis",
      valueFormatter: (v: unknown) => fmtNum(Number(v ?? 0)),
    },
    legend: { top: 0 },
    grid: { left: 8, right: 8, top: 32, bottom: 8, containLabel: true },
    xAxis: isDay
      ? { type: "category", data: categories }
      : {
          type: "value",
          axisLabel: { formatter: (v: number) => fmtCompact(v) },
        },
    yAxis: isDay
      ? {
          type: "value",
          axisLabel: { formatter: (v: number) => fmtCompact(v) },
        }
      : {
          type: "category",
          // interval 0 强制每个项目都显示名称
          data: categories,
          axisLabel: { interval: 0, width: 220, overflow: "truncate" },
        },
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

watch(() => [props.groups, props.by, mode.value], render);
</script>

<template>
  <div ref="el" style="width: 100%; height: 320px" />
</template>
