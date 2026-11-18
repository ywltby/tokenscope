<script setup lang="ts">
// v0.9.3：把表格后的「来源采集统计」与「统计说明」移到「聚合」标题浮层。
// 同一个浮层内保留两个小标题与内容；数据由 UsageTable 复用既有计算传入，
// 不在本组件重算统计。触发器为原生 button（RC06）：hover / focus / click
// 固定三态，Escape 关闭，固定态外点关闭；鼠标可从标题移入浮层继续阅读。
import { computed, onBeforeUnmount, onMounted, ref, useId } from "vue";
import { NTooltip } from "naive-ui";
import { t } from "../lib/translate";

export interface AggregateSourceLine {
  agent: string;
  parts: string[];
}

const props = defineProps<{
  sourceLines: AggregateSourceLine[];
  selectionNotes: string[];
}>();

const hover = ref(false);
const focus = ref(false);
const pinned = ref(false);
const open = computed(() => hover.value || focus.value || pinned.value);
const descId = `ts-aggregate-info-${useId()}`;
const triggerRef = ref<HTMLElement | null>(null);
let hoverLeaveTimer: ReturnType<typeof setTimeout> | null = null;

function clearHoverLeave(): void {
  if (hoverLeaveTimer != null) {
    clearTimeout(hoverLeaveTimer);
    hoverLeaveTimer = null;
  }
}
/// 鼠标从标题移到浮层会经过一段间隙；给短暂宽限，避免中途闪关。
/// 只清 hover：键盘焦点与固定态不受鼠标离开影响（PR #72）。
function scheduleHoverLeave(): void {
  clearHoverLeave();
  hoverLeaveTimer = setTimeout(() => {
    hover.value = false;
    hoverLeaveTimer = null;
  }, 120);
}
function close(): void {
  clearHoverLeave();
  hover.value = false;
  focus.value = false;
  pinned.value = false;
}
function onDocumentClick(e: MouseEvent): void {
  if (!pinned.value) return;
  const target = e.target as Node | null;
  if (!target) return;
  if (triggerRef.value?.contains(target)) return;
  if (document.getElementById(descId)?.contains(target)) return;
  close();
}
onMounted(() => document.addEventListener("click", onDocumentClick));
onBeforeUnmount(() => {
  document.removeEventListener("click", onDocumentClick);
  clearHoverLeave();
});
</script>

<template>
  <NTooltip
    trigger="manual"
    placement="bottom-start"
    :show="open"
    :style="'box-sizing: border-box; max-width: min(480px, calc(100vw - 32px)); backdrop-filter: var(--ts-glass-blur-popover); -webkit-backdrop-filter: var(--ts-glass-blur-popover); border-radius: var(--ts-radius-popover);'"
  >
    <template #trigger>
      <button
        ref="triggerRef"
        type="button"
        class="agg-info-trigger ts-focusable"
        :aria-label="t('dashboard.aggregateInfoAria')"
        :aria-expanded="open"
        :aria-describedby="open ? descId : undefined"
        @click="pinned = !pinned"
        @mouseenter="
          clearHoverLeave();
          hover = true;
        "
        @mouseleave="scheduleHoverLeave"
        @focus="focus = true"
        @blur="focus = false"
        @keydown.escape.prevent="close"
      >
        <slot />
      </button>
    </template>
    <div
      :id="descId"
      class="agg-info-body"
      @mouseenter="
        clearHoverLeave();
        hover = true;
      "
      @mouseleave="scheduleHoverLeave"
    >
      <div class="agg-info-section">
        <div class="agg-info-title">{{ t("dashboard.sourceStats") }}</div>
        <div v-for="s in props.sourceLines" :key="s.agent" class="agg-info-line">
          <strong>{{ s.agent }}</strong
          >：{{ s.parts.join(" · ") }}
        </div>
      </div>
      <div v-if="props.selectionNotes.length > 0" class="agg-info-section">
        <div class="agg-info-title">{{ t("dashboard.selectionStatsNote") }}</div>
        <div v-for="(note, i) in props.selectionNotes" :key="i" class="agg-info-line">
          {{ note }}
        </div>
      </div>
    </div>
  </NTooltip>
</template>

<style scoped>
/* 标题外观与 UsageTable .card-title 一致（17px/600），但由本组件自身承载样式。 */
.agg-info-trigger {
  appearance: none;
  margin: 0;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--ts-text);
  font: inherit;
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  cursor: help;
}
/* 窄窗口与长内容：浮层内部滚动，不撑破视口、不推动表格布局。 */
.agg-info-body {
  max-width: min(480px, calc(100vw - 48px));
  max-height: min(60vh, 480px);
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: var(--ts-space-1) 0;
}
.agg-info-section + .agg-info-section {
  border-top: 1px solid var(--ts-separator);
  margin-top: var(--ts-space-2);
  padding-top: var(--ts-space-2);
}
.agg-info-title {
  font-size: 12px;
  font-weight: 500;
  color: var(--ts-text-secondary);
  margin-bottom: var(--ts-space-1);
}
.agg-info-line {
  font-size: 12px;
  line-height: 1.9;
  color: var(--ts-text-secondary);
}
</style>
