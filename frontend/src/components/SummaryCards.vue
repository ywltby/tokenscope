<script setup lang="ts">
import { computed } from "vue";
import { NCard, NGrid, NGi, NProgress, NStatistic } from "naive-ui";
import { fmtCost, fmtNum, type Group } from "../types";

const props = defineProps<{ totals: Group }>();

const total = computed(
  () =>
    props.totals.tokens.input +
    props.totals.tokens.output +
    props.totals.tokens.cache_write +
    props.totals.tokens.cache_read,
);
const wan = computed(() => (total.value / 1e4).toFixed(2));
const hitRate = computed(() => {
  const base = props.totals.tokens.input + props.totals.tokens.cache_read;
  if (base === 0) return null;
  return (props.totals.tokens.cache_read / base) * 100;
});

const costText = computed(() => {
  const t = props.totals;
  const c = `$${fmtCost(t.cost_usd)}`;
  return t.unknown_pricing ? `${c}†` : c;
});

const statCards = computed(() => [
  { label: "新增输入", value: fmtNum(props.totals.tokens.input), color: "#4098fc", mark: "↓" },
  { label: "Output", value: fmtNum(props.totals.tokens.output), color: "#722ed1", mark: "↑" },
  {
    label: "创建",
    value: props.totals.tokens.cache_write === 0 ? "0" : fmtNum(props.totals.tokens.cache_write),
    color: "#8f8f8f",
    mark: "▣",
  },
  { label: "命中", value: fmtNum(props.totals.tokens.cache_read), color: "#18a058", mark: "✦" },
]);
</script>

<template>
  <div>
    <!-- 英雄区：真实消耗 Tokens 大数 + 总请求/总成本 -->
    <NCard size="small">
      <div class="hero-row">
        <div>
          <div class="hero-label"><span class="hero-icon">⚡</span> 真实消耗 Tokens</div>
          <div class="hero-num">
            {{ fmtNum(total) }}
            <span class="hero-wan">≈ {{ wan }} 万</span>
          </div>
        </div>
        <div class="hero-right">
          <NStatistic label="总请求数" :value="fmtNum(props.totals.requests)" />
          <NStatistic label="总成本">
            <template #default>
              <span :class="{ unknown: props.totals.unknown_pricing }">{{ costText }}</span>
            </template>
          </NStatistic>
        </div>
      </div>
    </NCard>

    <!-- 分项：新增输入 / Output / 创建 / 命中 -->
    <NGrid
      :cols="4"
      :x-gap="12"
      :y-gap="12"
      item-responsive
      responsive="screen"
      style="margin-top: 12px"
    >
      <NGi v-for="c in statCards" :key="c.label" span="1">
        <NCard size="small">
          <NStatistic :label="c.label" :value="c.value">
            <template #label>
              <span class="stat-mark" :style="{ color: c.color }">{{ c.mark }}</span>
              {{ c.label }}
            </template>
          </NStatistic>
        </NCard>
      </NGi>
    </NGrid>

    <!-- 缓存命中率 -->
    <NCard size="small" style="margin-top: 12px">
      <div class="hit-row">
        <span class="hit-label">缓存命中率</span>
        <span class="hit-value">
          {{ hitRate == null ? "N/A" : `${hitRate.toFixed(1)}%` }}
        </span>
      </div>
      <NProgress
        type="line"
        :percentage="hitRate ?? 0"
        :show-indicator="false"
        :height="8"
        border-radius="4px"
      />
      <div style="font-size: 12px; opacity: 0.6; margin-top: 4px">
        命中 = 缓存读 token ÷（新增输入 + 缓存读）
      </div>
    </NCard>
  </div>
</template>

<style scoped>
.hero-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}
.hero-label {
  font-size: 13px;
  opacity: 0.75;
}
.hero-icon {
  font-size: 20px;
}
.hero-num {
  font-size: 34px;
  font-weight: 700;
  line-height: 1.3;
  font-variant-numeric: tabular-nums;
}
.hero-wan {
  font-size: 13px;
  font-weight: 400;
  opacity: 0.6;
  margin-left: 6px;
}
.hero-right {
  display: flex;
  gap: 28px;
}
.stat-mark {
  margin-right: 2px;
}
.hit-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 8px;
}
.hit-label {
  font-size: 13px;
  opacity: 0.75;
}
.hit-value {
  font-size: 15px;
  font-weight: 600;
  color: #18a058;
}
.unknown {
  color: #d97706;
}
</style>
