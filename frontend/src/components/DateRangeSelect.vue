<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NCheckbox, NDatePicker, NPopover } from "naive-ui";
import { presetAriaLabel, presetKey, presetLabel, type RangePreset } from "../lib/rangePreset";
import type { RangeView } from "../types";

/**
 * U01/U02（UI 调整计划 2026-10-13）：时间预设触发器与日期弹层。
 *
 * - 全部时间（无预设）只显示日历图标、不着色；其余显示语义标签（32px 高、
 *   强调色底、最长 220px 省略）；
 * - 快捷项（当日 / 24h / 7d / 14d / 30d / 全部时间）**点一下立即生效并关闭**；
 * - 自定义区间用**一个**内联的 `type="datetimerange"` 面板（naive 2.45.3 的
 *   `panel` 组合，已在真实组件上验证：选择即回吐 formatted-value），
 *   `default-time` 让"只选日期"落在 00:00 / 23:59，前端不做任何加减；
 * - 只有自定义需要点「确定」；取消与 Escape 丢弃草稿、不改变当前值。
 */
const props = defineProps<{
  preset: RangePreset | null;
  tz: string;
  range?: RangeView | null;
}>();
const emit = defineEmits<{ (e: "update:preset", v: RangePreset | null): void }>();

const show = ref(false);
const triggerElement = ref<HTMLButtonElement | null>(null);
const panelMaxHeight = ref<string>();
function updatePanelHeight(): void {
  const rect = triggerElement.value?.getBoundingClientRect();
  if (!rect) return;
  // 交给 NPopover 在上下两侧自动翻转；预留外壳 padding、间隙和视口边距。
  const available = Math.min(
    window.innerHeight,
    Math.max(rect.top, window.innerHeight - rect.bottom),
  );
  panelMaxHeight.value = `${Math.max(0, available - 40)}px`;
}
/** 草稿：统计时区本地时刻 "YYYY-MM-DDTHH:MM" 字符串（无时区重解释）。 */
const draft = ref<[string, string] | null>(null);
const untilNow = ref(false);

const view = computed<RangeView | null>(() => props.range ?? null);
const label = computed(() => presetLabel(props.preset, view.value, props.tz));
const ariaLabel = computed(() => presetAriaLabel(props.preset, view.value, props.tz));
/// 全部时间（无预设）显示日历图标；其余显示语义标签。
const iconOnly = computed(() => !props.preset || props.preset.kind === "all");

/// 快捷项顺序固定：当日 / 24h / 7d / 14d / 30d / 全部时间。
const SHORTCUTS: { label: string; preset: RangePreset }[] = [
  { label: "当日", preset: { kind: "days", days: 1 } },
  { label: "24h", preset: { kind: "rolling", hours: 24 } },
  { label: "7d", preset: { kind: "days", days: 7 } },
  { label: "14d", preset: { kind: "days", days: 14 } },
  { label: "30d", preset: { kind: "days", days: 30 } },
  { label: "全部时间", preset: { kind: "all" } },
];

/// 自带操作行交给本组件的「清除 / 取消 / 确定」——不出现两套确认入口。
const NO_ACTIONS: ("clear" | "confirm" | "now")[] = [];
/// 只选日期时的默认时刻（naive `defaultTime`），结束端 = 选中日的最后一分钟。
const DEFAULT_TIME: [string, string] = ["00:00:00", "23:59:59"];

/// 打开弹层时草稿同步自当前预设：自定义直接回填选定值；其余预设用后端回传的
/// 冻结范围回填（没有范围可回填时留空，由 naive 的默认时刻补 00:00 / 23:59）。
function resetDraft(): void {
  const p = props.preset;
  if (p && p.kind === "custom") {
    untilNow.value = p.end === "now";
    draft.value = [p.start, p.end === "now" ? p.start : p.end];
    return;
  }
  untilNow.value = false;
  const r = view.value;
  if (r?.granularity === "day" && r.from && r.to) {
    draft.value = [`${r.from}T00:00`, `${r.to}T23:59`];
  } else if (r?.granularity === "instant") {
    draft.value = [r.start_local, r.end_local];
  } else {
    draft.value = null;
  }
}

// 打开时一次性同步草稿：取消/Escape 关闭不改值，重开回到当前预设。
watch(show, (open, _previous, onCleanup) => {
  if (!open) return;
  resetDraft();
  updatePanelHeight();
  window.addEventListener("resize", updatePanelHeight);
  document.addEventListener("scroll", updatePanelHeight, true);
  onCleanup(() => {
    window.removeEventListener("resize", updatePanelHeight);
    document.removeEventListener("scroll", updatePanelHeight, true);
  });
});

const draftReady = computed(() => {
  const d = draft.value;
  if (!d || !d[0]) return false;
  return untilNow.value || !!d[1];
});

function close(): void {
  // 草稿丢弃：下次打开时由 resetDraft 重新同步（Escape / 取消语义一致）。
  show.value = false;
}

function applyShortcut(preset: RangePreset): void {
  emit("update:preset", preset);
  show.value = false;
}

function confirmCustom(): void {
  const d = draft.value;
  if (!d || !d[0] || (!untilNow.value && !d[1])) return;
  emit("update:preset", { kind: "custom", start: d[0], end: untilNow.value ? "now" : d[1] });
  show.value = false;
}

function clearAll(): void {
  emit("update:preset", null);
  show.value = false;
}

const shortcutActive = (preset: RangePreset): boolean =>
  presetKey(preset) === presetKey(props.preset);

// 「结束于现在」：结束端整体禁用（值非 null 时 naive 会按 end 位置询问规则）。
const isDateDisabled = (_ts: number, position: "start" | "end"): boolean =>
  untilNow.value && position === "end";

const DISABLED_TIME = {
  isHourDisabled: () => true,
  isMinuteDisabled: () => true,
  isSecondDisabled: () => true,
};

const isTimeDisabled = (_ts: number, position: "start" | "end") =>
  untilNow.value && position === "end" ? DISABLED_TIME : {};
</script>

<template>
  <NPopover
    class="ts-popover-solid"
    trigger="click"
    :show="show"
    :show-arrow="false"
    placement="bottom-start"
    @update:show="show = $event"
  >
    <template #trigger>
      <button
        ref="triggerElement"
        type="button"
        class="range-trigger ts-btn ts-focusable"
        :class="{ 'is-icon': iconOnly, 'has-preset': !iconOnly, 'ts-selected-surface': !iconOnly }"
        :aria-label="ariaLabel"
        :aria-expanded="show"
        :title="ariaLabel"
      >
        <svg
          v-if="iconOnly"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.7"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <rect x="3" y="5" width="18" height="16" rx="3" />
          <path d="M7 3v4M17 3v4M3 11h18M8 15h2M14 15h2" />
        </svg>
        <span v-else class="range-trigger-label">{{ label }}</span>
      </button>
    </template>
    <div class="range-panel" :style="{ maxHeight: panelMaxHeight }" @keydown.esc="close">
      <div class="shortcut-row">
        <button
          v-for="s in SHORTCUTS"
          :key="s.label"
          type="button"
          class="ts-btn ts-choice shortcut-btn"
          :aria-pressed="shortcutActive(s.preset)"
          @click="applyShortcut(s.preset)"
        >
          {{ s.label }}
        </button>
      </div>
      <!-- 自定义区间：一个日期+时刻面板（精确到分钟），不再套第二层弹层 -->
      <div class="custom-block">
        <NDatePicker
          v-model:formatted-value="draft"
          class="custom-picker"
          type="datetimerange"
          panel
          :actions="NO_ACTIONS"
          value-format="yyyy-MM-dd'T'HH:mm"
          format="yyyy-MM-dd HH:mm"
          time-picker-format="HH:mm"
          :default-time="DEFAULT_TIME"
          :is-date-disabled="isDateDisabled"
          :is-time-disabled="isTimeDisabled"
        />
        <div class="follow-row">
          <NCheckbox v-model:checked="untilNow">结束于现在</NCheckbox>
          <span class="follow-hint">统计到当前时刻，不使用右侧结束日期</span>
        </div>
      </div>
      <div class="actions-row">
        <!-- 清除入口：一键回"全部时间"并关闭 -->
        <NButton v-if="!iconOnly" size="small" text @click="clearAll">清除</NButton>
        <span v-else></span>
        <span class="actions-group">
          <NButton size="small" @click="close">取消</NButton>
          <NButton size="small" type="primary" :disabled="!draftReady" @click="confirmCustom">
            确定
          </NButton>
        </span>
      </div>
    </div>
  </NPopover>
</template>

<style scoped>
/* 弹层：实色操作浮层（NPopover 浮层内容） */
.range-panel {
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  /* UX04：宽度随内容增长（快捷项不裁字），并受视口约束不产生横向滚动；
     U02：自定义区是内联的双月日历（约 600px），上限放宽到 720px 以免压缩日历。 */
  width: max-content;
  min-width: 300px;
  max-width: min(720px, calc(100vw - 32px));
  padding: var(--ts-space-3);
  border-radius: var(--ts-radius-popover);
  /* 实色操作浮层：材质只保留**一层**——
     Naive 的 .n-popover 外壳已经是实色操作浮层，内层保持透明，只负责排版。 */
  background-color: transparent;
  -webkit-backdrop-filter: none;
  backdrop-filter: none;
}

/* 触发按钮与筛选行分段控件同一族（DESIGN.md §5 筛选栏） */
.range-trigger {
  height: 32px;
  padding: 0 var(--ts-space-3);
  /* 语义标签最长 220px，超出省略（不撑破筛选行） */
  max-width: 220px;
  min-width: 0;
}

.range-trigger.is-icon {
  width: 32px;
  padding: 0;
  justify-content: center;
}

.range-trigger-label {
  margin-right: var(--ts-space-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.shortcut-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--ts-space-1);
  margin-bottom: var(--ts-space-3);
}
.shortcut-btn {
  height: 32px;
  font-size: 12px;
  white-space: nowrap;
}

/* 内联日期面板只做排版：玻璃材质由弹层外壳承担（避免面板自己再上一块实色） */
.custom-block {
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
}
.custom-picker :deep(.n-date-panel) {
  background-color: transparent;
  box-shadow: none;
}

.follow-row {
  display: inline-flex;
  align-items: center;
  gap: var(--ts-space-2);
  margin-top: var(--ts-space-2);
}
.follow-hint {
  font-size: 12px;
  color: var(--ts-text-muted);
}

.actions-row {
  flex-shrink: 0;
  display: flex;
  justify-content: space-between;
  margin-top: var(--ts-space-4);
}
.actions-group {
  display: inline-flex;
  gap: var(--ts-space-2);
}
</style>
