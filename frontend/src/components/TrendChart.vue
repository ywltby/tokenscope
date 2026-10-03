<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts";
import { useTheme } from "../composables/theme";
import { fmtNum, type Group } from "../types";

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
  chart = echarts.init(el.value, dark ? "dark" : undefined);
  const groups = props.groups.slice(0, 40);
  const isDay = props.by === "day";
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
          axisLabel: { formatter: (v: number) => fmtNum(v) },
        },
    yAxis: isDay
      ? {
          type: "value",
          axisLabel: { formatter: (v: number) => fmtNum(v) },
        }
      : {
          type: "category",
          data: [...groups]
            .sort((a, b) => b.tokens.input + b.tokens.output - (a.tokens.input + a.tokens.output))
            .map((g) => g.key),
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
