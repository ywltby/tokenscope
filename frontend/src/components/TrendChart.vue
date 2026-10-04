<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts";
import { useTheme } from "../composables/theme";
import { fmtNum, type Group } from "../types";

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
  const seriesNames = ["input", "output", "cache_write", "cache_read"] as const;
  const seriesLabels: Record<(typeof seriesNames)[number], string> = {
    input: "输入",
    output: "输出",
    cache_write: "缓存写",
    cache_read: "缓存读",
  };
  chart.setOption({
    backgroundColor: "transparent",
    tooltip: {
      trigger: "axis",
      valueFormatter: (v: unknown) => fmtNum(Number(v ?? 0)),
    },
    legend: { top: 0 },
    grid: { left: 8, right: 8, top: 32, bottom: 8, containLabel: true },
    xAxis: isDay
      ? { type: "category", data: groups.map((g) => g.key) }
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
          // 按用量降序；interval 0 强制每个项目都显示名称
          data: [...groups]
            .sort((a, b) => b.tokens.input + b.tokens.output - (a.tokens.input + a.tokens.output))
            .map((g) => g.key),
          axisLabel: { interval: 0, width: 220, overflow: "truncate" },
        },
    series: seriesNames.map((name) => ({
      name: seriesLabels[name],
      type: "bar",
      stack: "tokens",
      // by != day 时换为普通并列条形，取值函数相同
      barMaxWidth: 36,
      data: groups.map((g) => g.tokens[name]),
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
