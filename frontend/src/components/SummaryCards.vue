<script setup lang="ts">
// 设计系统 Task 3：统一指标读数条 + 轻量 token 分项行。
// 首屏先回答"用了多少、花费多少、覆盖什么时间"（DESIGN.md §4）；
// 数字 tabular lining，未知价格用 † 与警告色显式标注（不得伪装成 0），
// 命中率公式保持 cache_read / (input + cache_read) 不变。
import { computed } from "vue";
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

const costText = computed(() => `$${fmtCost(props.totals.cost_usd)}`);
/// 未知价格且已计价部分为 0：显示"未知†"而不是 $0.00（缺价 ≠ 免费）。
const costUnknownOnly = computed(() => props.totals.unknown_pricing && props.totals.cost_usd === 0);

const parts = computed(() => [
  { kind: "input", label: "输入", value: props.totals.tokens.input },
  { kind: "output", label: "输出", value: props.totals.tokens.output },
  { kind: "cache_write", label: "缓存写", value: props.totals.tokens.cache_write },
  { kind: "cache_read", label: "缓存命中", value: props.totals.tokens.cache_read },
]);
</script>

<template>
  <section class="metric-strip" aria-label="用量指标">
    <div class="metric metric-cost">
      <div class="metric-label">
        估算费用
        <span v-if="totals.unknown_pricing" class="unknown-mark">† 含未计价 token</span>
      </div>
      <div class="metric-value ts-num">
        <template v-if="costUnknownOnly">未知†</template>
        <template v-else>{{ costText }}</template>
      </div>
      <div class="metric-unit">USD · 估算值，非账单</div>
    </div>
    <div class="metric">
      <div class="metric-label">总 token</div>
      <div class="metric-value ts-num">{{ fmtNum(total) }}</div>
      <div class="metric-unit">≈ {{ wan }} 万 tokens</div>
    </div>
    <div class="metric">
      <div class="metric-label">请求数</div>
      <div class="metric-value ts-num">{{ fmtNum(totals.requests) }}</div>
      <div class="metric-unit">次请求</div>
    </div>
    <div class="metric">
      <div class="metric-label">缓存命中率</div>
      <div class="metric-value ts-num">
        {{ hitRate == null ? "N/A" : `${hitRate.toFixed(1)}%` }}
      </div>
      <div class="metric-unit">缓存读 ÷（新增输入 + 缓存读）</div>
    </div>
  </section>

  <!-- 轻量分项行：低饱和语义色点 + 文字 + 细竖线分隔（不再是四张彩卡） -->
  <div class="token-parts" aria-label="token 分项">
    <template v-for="(p, i) in parts" :key="p.kind">
      <span v-if="i > 0" class="part-sep" aria-hidden="true"></span>
      <span class="part">
        <span class="part-dot" :class="`part-${p.kind}`" aria-hidden="true"></span>
        <span class="part-label">{{ p.label }}</span>
        <span class="part-value ts-num">{{ fmtNum(p.value) }}</span>
      </span>
    </template>
  </div>
</template>

<style scoped>
.metric-strip {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--ts-space-4);
  background: var(--ts-surface-solid);
  border: 1px solid var(--ts-border);
  border-radius: var(--ts-radius-lg);
  padding: var(--ts-space-4) var(--ts-space-6);
}

.metric + .metric {
  border-left: 1px solid var(--ts-border);
  padding-left: var(--ts-space-4);
}

.metric-label {
  font-size: 12px;
  line-height: 1.4;
  color: var(--ts-text-secondary);
  display: flex;
  align-items: center;
  gap: var(--ts-space-2);
  white-space: nowrap;
}

.unknown-mark {
  color: var(--ts-warning);
  font-weight: 600;
}

.metric-value {
  font-size: 32px;
  font-weight: 700;
  line-height: 1.15;
  color: var(--ts-text);
  margin-top: var(--ts-space-1);
  white-space: nowrap;
}

.metric-cost .metric-value {
  color: var(--ts-accent);
}

.metric-unit {
  font-size: 12px;
  line-height: 1.4;
  color: var(--ts-text-muted);
  margin-top: var(--ts-space-1);
}

.token-parts {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--ts-space-3);
  margin-top: var(--ts-space-3);
  font-size: 13px;
  color: var(--ts-text-secondary);
}

.part {
  display: inline-flex;
  align-items: center;
  gap: var(--ts-space-2);
  white-space: nowrap;
}

.part-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

/* 低饱和图表系列色（与 chartTheme.ts / --ts-chart-* 同源，非状态色） */
.part-input {
  background: var(--ts-chart-input);
}
.part-output {
  background: var(--ts-chart-output);
}
.part-cache_write {
  background: var(--ts-chart-cache-write);
}
.part-cache_read {
  background: var(--ts-chart-cache-read);
}

.part-value {
  color: var(--ts-text);
  font-weight: 600;
}

.part-sep {
  width: 1px;
  height: 14px;
  background: var(--ts-border);
}

/* 窄窗口：读数条 2×2，避免挤压 */
@media (max-width: 1024px) {
  .metric-strip {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    row-gap: var(--ts-space-4);
    padding: var(--ts-space-4);
  }
  .metric:nth-child(3) {
    border-left: none;
    padding-left: 0;
  }
}
</style>
