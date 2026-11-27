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
import { costDisplayText } from "../lib/costDisplay";
import { formatSummaryTokenUnit } from "../lib/formatTokens";
import { TOKEN_BUCKETS } from "../lib/tokenDisplay";
import {
  percentageFixed2BigInt,
  ratioPercentBigInt,
  toTokenBigInt,
  totalInput,
  totalTokens,
} from "../lib/tokenUsage";
import { useT } from "../lib/i18n";
import GroupCostBreakdownTooltip from "./GroupCostBreakdownTooltip.vue";

const t = useT();

const props = defineProps<{ totals: Group }>();

const total = computed(() => totalTokens(props.totals.tokens));
const totalText = computed(() => fmtNum(total.value));
// 按千分位逗号切分，用于在极长整数（20~30+ 位）必须换行时只从逗号后断行（<wbr>
// 不产生多余字符，拖选复制与 textContent 仍为连续精确数字）。
const totalChunks = computed(() => totalText.value.split(/(?<=,)/));
const totalSizeClass = computed(() => {
  const len = totalText.value.length;
  if (len >= 27) return "metric-value-xlong";
  if (len >= 19) return "metric-value-long";
  return "";
});
const tokenUnit = computed(() => formatSummaryTokenUnit(total.value));

const copied = ref(false);
let copyTimer: ReturnType<typeof setTimeout> | null = null;
function clearCopyTimer(): void {
  if (copyTimer != null) {
    clearTimeout(copyTimer);
    copyTimer = null;
  }
}
async function copyExactTotal(): Promise<void> {
  const raw = total.value.toString();
  let ok = false;
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(raw);
      ok = true;
    }
  } catch {
    ok = false;
  }
  if (!ok && typeof document !== "undefined") {
    const el = document.createElement("textarea");
    el.value = raw;
    el.setAttribute("readonly", "");
    el.style.position = "fixed";
    el.style.opacity = "0";
    document.body.appendChild(el);
    el.select();
    try {
      ok = document.execCommand("copy");
    } catch {
      ok = false;
    }
    document.body.removeChild(el);
  }
  if (ok) {
    copied.value = true;
    clearCopyTimer();
    copyTimer = setTimeout(() => {
      copied.value = false;
      copyTimer = null;
    }, 1500);
  }
}
const hitRate = computed(() => {
  const base = totalInput(props.totals.tokens);
  if (base === 0n) return null;
  return ratioPercentBigInt(toTokenBigInt(props.totals.tokens.cache_read), base);
});

// UX07：汇总金额走单一入口（含 $、微小非零保护）。
// v0.9.2：主读数 = 单一费用金额（total_cost_usd，后端合并互不重叠贡献，
// 未舍入相加后这里只格式化一次）；构成与依据进悬浮详情，不再常驻展示。
const costDisplay = computed(() => costDisplayText(props.totals, t("dashboard.costUnknownOnly")));
/// 完全没有费用依据：显示「未知」而不是 $0.00（缺价 ≠ 免费）。
const costUnknownOnly = computed(() => props.totals.total_cost_usd == null);

// v0.9.2：费用悬浮三态（RC06 同契约：hover/focus/pinned，Escape 关闭）。
const costTipHover = ref(false);
const costTipFocus = ref(false);
const costTipPinned = ref(false);
const costTipOpen = computed(() => costTipHover.value || costTipFocus.value || costTipPinned.value);
function closeCostTip(): void {
  costTipHover.value = false;
  costTipFocus.value = false;
  costTipPinned.value = false;
}

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
onBeforeUnmount(() => {
  document.removeEventListener("click", onDocumentClick);
  clearCopyTimer();
});

// UX07/RC06：四类分项名称与顺序来自 tokenDisplay 单一来源（不再各自硬编码）。
// 2026-10-10 用户修订：英雄栏恢复**互斥四桶**口径——「输入」= 未缓存输入，
// 不含缓存写入/缓存命中；四类相加 = 总 token，比例条因此能显示四种系列色。
const parts = computed(() =>
  TOKEN_BUCKETS.map((b) => ({
    kind: b.key,
    label: b.label,
    value: toTokenBigInt(props.totals.tokens[b.key]),
  })),
);
</script>

<template>
  <section class="metric-card ts-card" :aria-label="t('dashboard.metricsAria')">
    <div class="metric-row">
      <!-- 主读数：估算费用 -->
      <div class="metric-main">
        <div class="metric-label">{{ t("dashboard.costLabel") }}</div>
        <!-- v0.9.2：主读数 = 单一费用金额（total_cost_usd，后端合并）；
             悬浮/键盘聚焦打开构成详情（GroupCostBreakdownTooltip）。 -->
        <div class="metric-value metric-value-main ts-num">
          <GroupCostBreakdownTooltip
            :group="totals"
            :display-text="costDisplay"
            :unknown-only="costUnknownOnly"
            :open="costTipOpen"
            :pinned="costTipPinned"
            @hover-enter="costTipHover = true"
            @hover-leave="costTipHover = false"
            @focus-enter="costTipFocus = true"
            @focus-leave="costTipFocus = false"
            @toggle="costTipPinned = !costTipPinned"
            @escape="closeCostTip"
          />
        </div>
      </div>

      <!-- 次读数：总 token / 请求数 / 缓存命中率，发丝线分隔 -->
      <div class="metric-secondary">
        <div class="metric-item metric-item-tokens">
          <div class="metric-label metric-label-row">
            <span>{{ t("dashboard.totalTokens") }}</span>
            <button
              type="button"
              class="copy-total-btn ts-focusable"
              :aria-label="
                copied ? t('dashboard.copiedExactTokens') : t('dashboard.copyExactTokens')
              "
              :title="copied ? t('dashboard.copiedExactTokens') : t('dashboard.copyExactTokens')"
              @click="copyExactTotal"
            >
              <svg
                v-if="!copied"
                class="copy-icon"
                viewBox="0 0 16 16"
                fill="none"
                aria-hidden="true"
              >
                <rect
                  x="5.25"
                  y="5.25"
                  width="8.5"
                  height="8.5"
                  rx="1.75"
                  stroke="currentColor"
                  stroke-width="1.5"
                />
                <path
                  d="M3.75 10.75H3.5A1.75 1.75 0 0 1 1.75 9V3.5A1.75 1.75 0 0 1 3.5 1.75H9A1.75 1.75 0 0 1 10.75 3.5v.25"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                />
              </svg>
              <svg
                v-else
                class="copy-icon copy-icon-done"
                viewBox="0 0 16 16"
                fill="none"
                aria-hidden="true"
              >
                <path
                  d="M3.25 8.5 6.5 11.75 12.75 4.5"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
              <span v-if="copied" class="copy-feedback">{{
                t("dashboard.copiedExactTokens")
              }}</span>
            </button>
          </div>
          <div
            class="metric-value metric-value-tokens ts-num"
            :class="totalSizeClass"
            :title="totalText"
          >
            <template v-for="(chunk, idx) in totalChunks" :key="idx"
              ><wbr v-if="idx > 0" /><span class="num-chunk">{{ chunk }}</span></template
            >
          </div>
          <div class="metric-unit ts-num">{{ tokenUnit }}</div>
        </div>
        <div class="metric-sep" aria-hidden="true" />
        <div class="metric-item metric-item-requests">
          <div class="metric-label">{{ t("dashboard.requests") }}</div>
          <div class="metric-value ts-num">{{ fmtNum(totals.requests) }}</div>
          <div class="metric-unit">{{ t("dashboard.requestsUnit") }}</div>
        </div>
        <div class="metric-sep" aria-hidden="true" />
        <div class="metric-item metric-item-hit">
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
      <div v-if="total > 0n" class="parts-bar" role="img" :aria-label="t('dashboard.partsBarAria')">
        <span
          v-for="p in parts"
          :key="p.kind"
          class="bar-segment"
          :class="`bar-${p.kind}`"
          :style="{ width: `${percentageFixed2BigInt(p.value, total)}%` }"
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
  flex-wrap: wrap;
  gap: var(--ts-space-5) var(--ts-space-6);
  align-items: flex-start;
}

.metric-main {
  flex: 0 0 auto;
  min-width: 200px;
  max-width: 100%;
}

.metric-secondary {
  flex: 1 1 560px;
  min-width: 0;
  display: flex;
  gap: var(--ts-space-4);
  align-items: flex-start;
}

.metric-item {
  flex: 1 1 0;
  min-width: 0;
}

/* 总 token 列比请求数/命中率更宽：为 20~30+ 位大数留足空间，避免挤占兄弟列 */
.metric-item-tokens {
  flex: 1.85 1 0;
}

.metric-item-requests {
  flex: 0.9 1 0;
}

.metric-item-hit {
  flex: 0.85 1 0;
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

/* 总 token 支持任意长度大整数：常规长度保持 26px 单行；20 位缩至 21px 单行；
   30+ 位缩至 17px 并按千分位逗号优雅折行，绝不溢出栏位且保持可拖选与一键复制。 */
.metric-value-tokens {
  white-space: normal;
  overflow-wrap: anywhere;
  max-width: 100%;
  user-select: text;
}

.metric-value-long {
  font-size: 21px;
  line-height: 1.25;
}

.metric-value-xlong {
  font-size: 17px;
  line-height: 1.3;
}

.num-chunk {
  display: inline;
  white-space: nowrap;
}

.metric-label-row {
  gap: 6px;
}

.copy-total-btn {
  appearance: none;
  border: none;
  background: transparent;
  color: var(--ts-text-muted);
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 2px 4px;
  margin: -2px 0;
  border-radius: 6px;
  cursor: pointer;
  font: inherit;
  line-height: 1;
  transition:
    color 0.15s ease,
    background-color 0.15s ease;
}

.copy-total-btn:hover {
  color: var(--ts-text);
  background: var(--ts-fill);
}

.copy-icon {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.copy-icon-done,
.copy-feedback {
  color: var(--ts-success, #34c759);
  font-size: 11px;
  font-weight: 500;
}

/* 主读数（估算费用）44px/600，用主文字色不染强调色 */
.metric-value-main {
  font-size: 44px;
  letter-spacing: -0.03em;
  max-width: 100%;
  white-space: normal;
  overflow-wrap: anywhere;
}

/* T2：来源记录费用独立行（估算费用下方），不得与主估算混同。 */
.metric-source-cost {
  margin-top: var(--ts-space-1);
  font-size: 12px;
  line-height: 1.4;
  color: var(--ts-text-secondary);
  font-variant-numeric: tabular-nums;
}

/* v0.9.1 缺陷 C 规则1：主读数 = 来源记录金额时的「来源记录」标识行。 */
.metric-source-tag {
  margin-top: var(--ts-space-1);
}

.metric-source-amount {
  font-variant-numeric: tabular-nums;
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
  flex-wrap: wrap;
  gap: var(--ts-space-2);
  min-width: 0;
  max-width: 100%;
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
  white-space: normal;
  overflow-wrap: anywhere;
  min-width: 0;
  max-width: 100%;
}

/* 窄窗口：主读数与次读数纵向堆叠 */
@media (max-width: 1024px) {
  .metric-row {
    flex-direction: column;
    gap: var(--ts-space-5);
  }
  .metric-secondary {
    flex: 0 0 auto;
    width: 100%;
  }
}
</style>
