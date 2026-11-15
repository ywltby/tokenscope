<script setup lang="ts">
// v0.9.2：分组级费用悬浮详情——费用列/总览卡的单金额触发器共享本组件。
//
// 触发器：金额本身（原生 button，Enter/Space 固定切换；hover/focus 三态
// 与 CostBreakdownTooltip 同一可访问契约，RC06：固定态外点关闭、Escape
// 一律关闭、aria-describedby 指向真实存在的内容节点）。
//
// 内容分区（只有组内实际存在的部分才显示）：
// - 请求明细估算（cost_usd 中来自明细的部分——后端把汇总估算也并入
//   cost_usd，这里不区分来源时按"估算"统一说明）；
// - 日汇总来源记录费用（source_cost_usd，含逐条依据说明）；
// - 覆盖完整性（source_cost_state：partial 说明部分汇总未提供费用）；
// - 缺失部分（cost_partially_unknown → unknown_tokens 分项）；
// - 估算依据：日汇总按价格估算时注明「按日汇总估算（prompt 总量），
//   不等于历史精确费用；未编造请求时间与缓存分档」。
// 不称为实际账单，不放大段来源说明。
import { computed, h, onBeforeUnmount, onMounted, ref, useId } from "vue";
import { NTooltip } from "naive-ui";
import { formatMoney } from "../lib/formatMoney";
import { t } from "../lib/translate";
import type { Group, TokenCounts } from "../types";

const props = defineProps<{
  group: Group;
  /** 主金额文本（CostCell/SummaryCards 已格式化好的同一字符串）。 */
  displayText: string;
  /** 完全没有费用依据（显示未知†，无浮层内容价值时仍允许打开说明缺项）。 */
  unknownOnly: boolean;
  open: boolean;
  pinned: boolean;
}>();

const emit = defineEmits<{
  (e: "hover-enter"): void;
  (e: "hover-leave"): void;
  (e: "focus-enter"): void;
  (e: "focus-leave"): void;
  (e: "toggle"): void;
  (e: "escape"): void;
}>();

const ariaLabel = computed(() =>
  props.unknownOnly
    ? t("dashboard.groupCostAriaUnknown")
    : t("dashboard.groupCostAria", { cost: props.displayText }),
);
const descId = `ts-group-cost-${useId()}`;
const triggerRef = ref<HTMLElement | null>(null);

// RC06：固定态的外部点击关闭（点触发器自身或浮层内容不算外部）。
function onDocumentClick(e: MouseEvent): void {
  if (!props.pinned) return;
  const target = e.target as Node | null;
  if (!target) return;
  if (triggerRef.value?.contains(target)) return;
  const body = document.getElementById(descId);
  if (body?.contains(target)) return;
  emit("toggle");
}
onMounted(() => document.addEventListener("click", onDocumentClick));
onBeforeUnmount(() => document.removeEventListener("click", onDocumentClick));

const g = computed(() => props.group);
/// 主金额旁的短标记：真实费用缺失 → †（原因在悬浮详情内）。
/// unknownOnly（完全无依据 → 「未知†」）时不再叠加——未知标记自身已含 †。
const partialMark = computed(() =>
  props.unknownOnly || g.value.cost_partially_unknown ? (props.unknownOnly ? "" : "†") : "",
);
const hasEstimate = computed(() => g.value.cost_usd !== 0);
const estimateText = computed(() => formatMoney(g.value.cost_usd, "summary"));
const hasSource = computed(() => g.value.source_cost_usd != null);
const sourceText = computed(() => formatMoney(g.value.source_cost_usd ?? 0, "summary"));
const sourcePartial = computed(() => g.value.source_cost_state === "partial");
const hasUnknown = computed(
  () =>
    g.value.unknown_tokens.input +
      g.value.unknown_tokens.output +
      g.value.unknown_tokens.cache_write +
      g.value.unknown_tokens.cache_read >
    0,
);
const unknownTotal = computed(
  () =>
    g.value.unknown_tokens.input +
    g.value.unknown_tokens.output +
    g.value.unknown_tokens.cache_write +
    g.value.unknown_tokens.cache_read,
);
const unknownParts = computed(() => {
  const u: TokenCounts = g.value.unknown_tokens;
  const rows: { label: string; n: number }[] = [];
  for (const [key, label] of [
    ["input", t("dashboard.colInput")],
    ["output", t("dashboard.colOutput")],
    ["cache_write", t("dashboard.colCacheWrite")],
    ["cache_read", t("dashboard.colCacheRead")],
  ] as const) {
    const n = u[key];
    if (n > 0) rows.push({ label, n });
  }
  return rows;
});
const detailAbsent = computed(() => !hasEstimate.value && !hasSource.value && !hasUnknown.value);
const hasRollupContribution = computed(
  () => g.value.cost_detail === "rollup_only" || g.value.cost_detail === "mixed",
);

function renderContent(): ReturnType<typeof h> {
  const sections: ReturnType<typeof h>[] = [];
  // 结果区
  sections.push(
    h("div", { class: "gcb-section" }, [
      h("div", { class: "gcb-title" }, t("dashboard.groupCostResultTitle")),
      h("div", { class: "gcb-row gcb-main" }, [
        h("span", {}, t("dashboard.groupCostMainLabel")),
        h("span", { class: "gcb-amount ts-num" }, props.displayText),
      ]),
    ]),
  );
  // 构成区（实色衬底）
  const compRows: ReturnType<typeof h>[] = [];
  if (hasEstimate.value) {
    compRows.push(
      h("div", { class: "gcb-row" }, [
        h("span", { class: "gcb-label" }, t("dashboard.groupCostEstimateLabel")),
        h("span", { class: "gcb-amount ts-num" }, estimateText.value),
      ]),
    );
    if (hasRollupContribution.value) {
      compRows.push(h("div", { class: "gcb-note" }, t("dashboard.groupCostEstimateNote")));
    }
  }
  if (hasSource.value) {
    compRows.push(
      h("div", { class: "gcb-row" }, [
        h("span", { class: "gcb-label" }, t("dashboard.groupCostSourceLabel")),
        h("span", { class: "gcb-amount ts-num" }, sourceText.value),
      ]),
    );
    if (sourcePartial.value) {
      compRows.push(h("div", { class: "gcb-note" }, t("dashboard.groupCostSourcePartialNote")));
    } else {
      compRows.push(h("div", { class: "gcb-note" }, t("dashboard.groupCostSourceNote")));
    }
  }
  if (compRows.length > 0) {
    sections.push(
      h("div", { class: "gcb-section gcb-solid" }, [
        h("div", { class: "gcb-title" }, t("dashboard.groupCostPartsTitle")),
        ...compRows,
      ]),
    );
  }
  // 缺失区
  if (hasUnknown.value) {
    sections.push(
      h("div", { class: "gcb-section" }, [
        h("div", { class: "gcb-title" }, t("dashboard.groupCostUnknownTitle")),
        h("div", { class: "gcb-row" }, [
          h("span", { class: "gcb-label" }, t("dashboard.groupCostUnknownTotal")),
          h("span", { class: "gcb-amount ts-num" }, String(unknownTotal.value)),
        ]),
        ...unknownParts.value.map((p) =>
          h("div", { class: "gcb-row" }, [
            h("span", { class: "gcb-label" }, p.label),
            h("span", { class: "gcb-amount ts-num" }, String(p.n)),
          ]),
        ),
        h("div", { class: "gcb-note" }, t("dashboard.groupCostUnknownNote")),
      ]),
    );
  }
  if (detailAbsent.value) {
    sections.push(
      h("div", { class: "gcb-section" }, [
        h("div", { class: "gcb-note" }, t("dashboard.groupCostNoBasis")),
      ]),
    );
  }
  return h("div", { class: "gcb-body" }, sections);
}
</script>

<template>
  <NTooltip
    trigger="manual"
    placement="left"
    :show="open"
    :style="'box-sizing: border-box; max-width: min(480px, calc(100vw - 32px)); backdrop-filter: var(--ts-glass-blur-popover); -webkit-backdrop-filter: var(--ts-glass-blur-popover); border-radius: var(--ts-radius-popover);'"
  >
    <template #trigger>
      <!-- 触发器 = 金额本身（原生 button：Enter/Space 各一次 click）；
           不手写 keydown（与原生重复会 toggle 两次，RC06）。 -->
      <button
        :ref="
          (node) => {
            if (node) triggerRef = node as HTMLElement;
          }
        "
        type="button"
        class="gcb-trigger"
        :aria-label="ariaLabel"
        :aria-describedby="open ? descId : undefined"
        @click="emit('toggle')"
        @mouseenter="emit('hover-enter')"
        @mouseleave="emit('hover-leave')"
        @focus="emit('focus-enter')"
        @blur="emit('focus-leave')"
        @keydown.escape.prevent="emit('escape')"
      >
        <span class="gcb-amount-main" :class="{ 'gcb-unknown': unknownOnly }">{{
          displayText + partialMark
        }}</span>
      </button>
    </template>
    <div :id="descId" class="gcb-tooltip">
      <!-- renderContent 返回 vnode 数组；直接插值会被 toDisplayString 压成
           字符串（圆形结构崩溃）。包一层函数式组件让 Vue 走正常 vnode 渲染。 -->
      <component :is="() => renderContent()" />
    </div>
  </NTooltip>
</template>

<style scoped>
.gcb-trigger {
  /* 组件局部重置：保留数值对齐与焦点环（DESIGN.md §5 RC06 约定）。 */
  padding: 0;
  border: none;
  background: none;
  color: inherit;
  font: inherit;
  cursor: default;
  font-variant-numeric: tabular-nums;
}
.gcb-trigger:focus-visible {
  outline: 2px solid var(--ts-accent);
  outline-offset: 2px;
}
.gcb-unknown {
  color: var(--ts-warning);
}
.gcb-body {
  max-width: min(480px, calc(100vw - 48px));
}
.gcb-section {
  padding: var(--ts-space-2) 0;
}
.gcb-section + .gcb-section {
  border-top: 1px solid var(--ts-separator);
}
.gcb-solid {
  background: var(--ts-surface-solid);
  border-radius: var(--ts-radius-popover);
  padding: var(--ts-space-2) var(--ts-space-3);
  margin: var(--ts-space-1) 0;
}
.gcb-title {
  font-size: 12px;
  font-weight: 500;
  color: var(--ts-text-secondary);
  margin-bottom: var(--ts-space-1);
}
.gcb-row {
  display: flex;
  justify-content: space-between;
  gap: var(--ts-space-3);
  font-size: 13px;
  line-height: 1.6;
}
.gcb-main .gcb-amount {
  font-weight: 600;
}
.gcb-label {
  color: var(--ts-text-secondary);
}
.gcb-amount {
  color: var(--ts-text);
  white-space: nowrap;
}
.gcb-note {
  font-size: 12px;
  line-height: 1.5;
  color: var(--ts-text-muted);
  margin-top: var(--ts-space-1);
}
</style>
