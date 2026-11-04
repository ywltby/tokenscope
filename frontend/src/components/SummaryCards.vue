<script setup lang="ts">
// 设计系统 Task 4：单张指标卡（DESIGN.md §5 指标卡）——左侧主读数估算费用
// （44px），右侧三个次读数（26px）发丝线分隔；含未计价 token 时费用旁警告
// 胶囊；命中率公式收进 tooltip（hover/focus 均可打开）；底部四类 token
// 图例 + **互斥四桶**比例条（输入 = 未缓存输入，与缓存写/缓存命中并列，
// 四类颜色齐全）。命中率分母为含两类缓存的**总输入**（与比例条口径不同，
// tooltip 内写明）。
import { computed, onBeforeUnmount, onMounted, ref, useId } from "vue";
import { NTooltip } from "naive-ui";
import { fmtNum, type Group } from "../types";
import { formatMoney } from "../lib/formatMoney";
import { TOKEN_BUCKETS } from "../lib/tokenDisplay";
import { totalInput, totalTokens } from "../lib/tokenUsage";
import { useT } from "../lib/i18n";

const t = useT();

const props = defineProps<{ totals: Group }>();

const total = computed(() => totalTokens(props.totals.tokens));
const wan = computed(() => (total.value / 1e4).toFixed(2));
const hitRate = computed(() => {
  const base = totalInput(props.totals.tokens);
  if (base === 0) return null;
  return (props.totals.tokens.cache_read / base) * 100;
});

// UX07：汇总金额走单一入口（含 $、微小非零保护）。
const costText = computed(() => formatMoney(props.totals.cost_usd, "summary"));
/// 未知价格且已计价部分为 0：显示"未知†"而不是 $0.00（缺价 ≠ 免费）。
const costUnknownOnly = computed(() => props.totals.unknown_pricing && props.totals.cost_usd === 0);

// R09/F08：命中率公式 tooltip 受控显示——hover/focus/pinned 三态分离、
// 统一 show 计算：hover 随指针、focus 随键盘焦点（focus 在时鼠标离开不取消）、
// click 固定切换；Escape 一律关闭。
// RC06：触发器改为原生 `button type=button`——浏览器内建 Enter/Space 各产生
// 一次 click，删除原先手写的 keydown.enter/keydown.space（与原生重复 toggle），
// 并补上**固定态**的外部点击关闭（NTooltip 的 clickoutside 在 trigger="manual"
// 下不可靠，与 HelpTooltip/CostBreakdownTooltip 同一处理方式）。
const hoverOpen = ref(false);
const focusOpen = ref(false);
const pinned = ref(false);
const hitTipOpen = computed(() => hoverOpen.value || focusOpen.value || pinned.value);
function closeHitTip(): void {
  hoverOpen.value = false;
  focusOpen.value = false;
  pinned.value = false;
}
const descId = `ts-hit-rate-${useId()}`;
const triggerRef = ref<HTMLElement | null>(null);
function onDocumentClick(e: MouseEvent): void {
  if (!pinned.value) return;
  const target = e.target as Node | null;
  if (!target) return;
  if (triggerRef.value?.contains(target)) return;
  if (document.getElementById(descId)?.contains(target)) return;
  closeHitTip();
}
onMounted(() => document.addEventListener("click", onDocumentClick));
onBeforeUnmount(() => document.removeEventListener("click", onDocumentClick));

// UX07/RC06：四类分项名称与顺序来自 tokenDisplay 单一来源（不再各自硬编码）。
// 2026-10-10 用户修订：英雄栏恢复**互斥四桶**口径——「输入」= 未缓存输入，
// 不含缓存写入/缓存命中；四类相加 = 总 token，比例条因此能显示四种系列色。
const parts = computed(() =>
  TOKEN_BUCKETS.map((b) => ({
    kind: b.key,
    label: b.label,
    value: props.totals.tokens[b.key],
  })),
);
</script>

<template>
  <section class="metric-card ts-card" :aria-label="t('dashboard.metricsAria')">
    <div class="metric-row">
      <!-- 主读数：估算费用 -->
      <div class="metric-main">
        <div class="metric-label">
          {{ t("dashboard.costLabel") }}
          <span v-if="totals.unknown_pricing" class="ts-pill ts-pill-warning">
            {{ t("dashboard.unpricedPill") }}
          </span>
          <span
            v-if="totals.cache_write_split_estimated"
            class="ts-pill ts-pill-warning"
            :title="t('dashboard.tierEstimatedTitle')"
          >
            {{ t("dashboard.tierPill") }}
          </span>
        </div>
        <div class="metric-value metric-value-main ts-num">
          <template v-if="costUnknownOnly">{{ t("dashboard.costUnknownOnly") }}</template>
          <template v-else>{{ costText }}</template>
        </div>
      </div>

      <!-- 次读数：总 token / 请求数 / 缓存命中率，发丝线分隔 -->
      <div class="metric-secondary">
        <div class="metric-item">
          <div class="metric-label">{{ t("dashboard.totalTokens") }}</div>
          <div class="metric-value ts-num">{{ fmtNum(total) }}</div>
          <div class="metric-unit">{{ t("dashboard.tokensWan", { wan }) }}</div>
        </div>
        <div class="metric-sep" aria-hidden="true" />
        <div class="metric-item">
          <div class="metric-label">{{ t("dashboard.requests") }}</div>
          <div class="metric-value ts-num">{{ fmtNum(totals.requests) }}</div>
          <div class="metric-unit">{{ t("dashboard.requestsUnit") }}</div>
        </div>
        <div class="metric-sep" aria-hidden="true" />
        <div class="metric-item">
          <NTooltip placement="bottom" trigger="manual" :show="hitTipOpen">
            <template #trigger>
              <!-- RC06：原生 button + 局部外观重置；Enter/Space 由浏览器内建
                   激活（各一次 click），不再手写 keydown 切换。 -->
              <button
                ref="triggerRef"
                type="button"
                class="metric-label metric-label-help ts-focusable label-trigger"
                :aria-label="t('dashboard.hitRateAria')"
                :aria-expanded="hitTipOpen"
                :aria-describedby="hitTipOpen ? descId : undefined"
                @mouseenter="hoverOpen = true"
                @mouseleave="hoverOpen = false"
                @focus="focusOpen = true"
                @blur="focusOpen = false"
                @click="pinned = !pinned"
                @keydown.escape="closeHitTip"
              >
                {{ t("dashboard.hitRate") }}
              </button>
            </template>
            <div
              :id="descId"
              class="hit-tip"
              @mouseenter="hoverOpen = true"
              @mouseleave="hoverOpen = false"
            >
              <template v-for="(line, i) in t('dashboard.hitRateFormula').split('\n')" :key="i">
                <template v-if="i > 0"><br /></template>{{ line }}
              </template>
            </div>
          </NTooltip>
          <div class="metric-value ts-num">
            {{ hitRate == null ? t("dashboard.na") : `${hitRate.toFixed(1)}%` }}
          </div>
          <div class="metric-unit">{{ t("dashboard.hitRateUnit") }}</div>
        </div>
      </div>
    </div>

    <!-- 分项比例条：6px 高、3px 圆角、按互斥四桶占比分段（全零为空槽） -->
    <div class="parts-bar-container">
      <div v-if="total > 0" class="parts-bar" role="img" :aria-label="t('dashboard.partsBarAria')">
        <span
          v-for="p in parts"
          :key="p.kind"
          class="bar-segment"
          :class="`bar-${p.kind}`"
          :style="{ width: `${((p.value / total) * 100).toFixed(2)}%` }"
          :aria-label="`${p.label} ${fmtNum(p.value)}`"
        />
      </div>
      <div v-else class="parts-bar parts-bar-empty" />
    </div>

    <!-- 图例：色点 + 名称 + 数值（与比例条/图表同源 token） -->
    <div class="parts-legend" :aria-label="t('dashboard.partsLegendAria')">
      <span v-for="p in parts" :key="p.kind" class="legend-item">
        <span class="legend-dot" :class="`part-${p.kind}`" aria-hidden="true" />
        <span class="legend-label">{{ p.label }}</span>
        <span class="legend-value ts-num">{{ fmtNum(p.value) }}</span>
      </span>
    </div>
  </section>
</template>

<style scoped>
.metric-row {
  display: flex;
  gap: var(--ts-space-6);
  align-items: flex-start;
}

.metric-main {
  flex: 0 0 auto;
  min-width: 200px;
}

.metric-secondary {
  flex: 1;
  display: flex;
  gap: var(--ts-space-4);
  align-items: flex-start;
}

.metric-item {
  flex: 1;
  min-width: 0;
}

/* 发丝线分隔（DESIGN.md §5 指标卡） */
.metric-sep {
  width: 1px;
  height: 48px;
  background: var(--ts-separator);
  flex-shrink: 0;
  align-self: center;
}

.metric-label {
  font-size: 12px;
  font-weight: 500;
  line-height: 1.4;
  color: var(--ts-text-secondary);
  display: flex;
  align-items: center;
  gap: var(--ts-space-2);
  margin-bottom: var(--ts-space-1);
}

/* 命中率公式 tooltip 触发器：hover/focus 都可打开 */
.metric-label-help {
  cursor: help;
  width: fit-content;
}

/* RC06：命中率说明触发器改为原生 button，这里只做局部外观重置——
   字号/颜色/对齐与原来的 div 标签一致，焦点环仍来自全局 .ts-focusable。 */
.label-trigger {
  appearance: none;
  margin: 0;
  border: none;
  background: transparent;
  color: inherit;
  font: inherit;
  padding: 0;
  text-align: inherit;
}

/* 次读数 26px/600 */
.metric-value {
  font-family: var(--ts-font-display);
  font-size: 26px;
  font-weight: 600;
  line-height: 1.2;
  letter-spacing: -0.02em;
  color: var(--ts-text);
  white-space: nowrap;
}

/* 主读数（估算费用）44px/600，用主文字色不染强调色 */
.metric-value-main {
  font-size: 44px;
  letter-spacing: -0.03em;
}

.metric-unit {
  font-size: 12px;
  line-height: 1.4;
  color: var(--ts-text-muted);
  margin-top: 2px;
}

.hit-tip {
  max-width: 320px;
  font-size: 12px;
  line-height: 1.6;
}

/* 分项比例条 */
.parts-bar-container {
  margin-top: var(--ts-space-5);
}

.parts-bar {
  display: flex;
  height: 6px;
  border-radius: 3px;
  overflow: hidden;
  background: var(--ts-fill);
}

.bar-segment {
  height: 100%;
}

.bar-input {
  background: var(--ts-chart-input);
}
.bar-output {
  background: var(--ts-chart-output);
}
.bar-cache_write {
  background: var(--ts-chart-cache-write);
}
.bar-cache_read {
  background: var(--ts-chart-cache-read);
}

/* 图例：色点 + 名称 + 数值 */
.parts-legend {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--ts-space-4);
  margin-top: var(--ts-space-3);
  font-size: 13px;
}

.legend-item {
  display: inline-flex;
  align-items: center;
  gap: var(--ts-space-2);
}

.legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

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

.legend-label {
  color: var(--ts-text-secondary);
}

.legend-value {
  color: var(--ts-text);
  font-weight: 600;
}

/* 窄窗口：主读数与次读数纵向堆叠 */
@media (max-width: 1024px) {
  .metric-row {
    flex-direction: column;
    gap: var(--ts-space-5);
  }
  .metric-secondary {
    width: 100%;
  }
}
</style>
