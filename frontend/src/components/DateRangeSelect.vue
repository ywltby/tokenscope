<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NCheckbox, NDatePicker, NPopover } from "naive-ui";
import { addDays, rangeLabel, todayInTz } from "../lib/dates";

/**
 * ccs 风格的日期区间选择（M10；C3 修复 F07）：
 * - 弹层受控（:show 双向绑定）——修复前 show 未绑定，取消/确定关不掉、
 *   打开时的草稿同步 watch 永不触发；
 * - 「清除」显式入口（修复前清空起始日后确定被禁用，"全部时间"不可达）；
 * - 快捷条全部按自然日口径命名（原 "24h" 实为昨天+今天两个自然日，更名
 *   "近2天"，不与滚动小时窗口混称）；
 * 日期粒度（存储/过滤均为自然日，换算由 Dashboard 按解析时区完成）。
 */
// Task 6：对外契约 = 统计时区下的日历字符串（YYYY-MM-DD），不再传
// 本机零点毫秒（本机时区与统计时区不同会导致"今天"错日）。
const props = defineProps<{
  value: [string, string] | null;
  tz: string;
  iconOnly?: boolean;
}>();
const emit = defineEmits<{ (e: "update:value", v: [string, string] | null): void }>();

const show = ref(false);
const draftFrom = ref<string | null>(null);
const draftTo = ref<string | null>(null);
const followToday = ref(false);

function todayStr(): string {
  return todayInTz(props.tz);
}

// SF05：快捷项只声明起始日的偏移——两端由**同一次** today 读取派生，
// 一次操作（点击/比较/确认）跨午夜也不会取到两个不同的"今天"。
const shortcuts: { label: string; fromOffset: number }[] = [
  { label: "当天", fromOffset: 0 },
  { label: "近2天", fromOffset: -1 },
  { label: "近7天", fromOffset: -6 },
  { label: "近14天", fromOffset: -13 },
  { label: "近30天", fromOffset: -29 },
];

/// 一次操作只读取一次 today，再计算两端（近 N 天 = [今天-(N-1), 今天]）。
function shortcutRange(sc: { fromOffset: number }): [string, string] {
  const today = todayStr();
  return [addDays(today, sc.fromOffset), today];
}

function applyShortcut(sc: { label: string; fromOffset: number }): void {
  const [from, to] = shortcutRange(sc);
  draftFrom.value = from;
  draftTo.value = to;
  followToday.value = false;
}

// 受控开关：打开时草稿同步自当前值；取消/确定经 show 关闭。
// SF05：打开面板是一次操作，today 只读取一次。
watch(show, (open) => {
  if (!open) return;
  const today = todayStr();
  if (props.value) {
    draftFrom.value = props.value[0];
    draftTo.value = props.value[1];
    // 结束日为今天视为"跟随今天"勾选态
    followToday.value = draftTo.value >= today;
  } else {
    draftFrom.value = null;
    draftTo.value = null;
    followToday.value = false;
  }
});

function confirm(): void {
  // SF05：确认是一次操作，today 只读取一次（跟随今天分支派生其结束日）。
  const today = todayStr();
  if (draftFrom.value == null) {
    emit("update:value", null);
  } else if (followToday.value) {
    // 结束日跟随今天：自然日粒度下等价于"从起始日至今（含未来）"
    emit("update:value", [draftFrom.value, today]);
  } else if (draftTo.value == null) {
    emit("update:value", [draftFrom.value, draftFrom.value]);
  } else {
    emit("update:value", [draftFrom.value, draftTo.value]);
  }
  show.value = false;
}

function clear(): void {
  draftFrom.value = null;
  draftTo.value = null;
  followToday.value = false;
}

// 触发器标签从 props 派生（修复前是 ref，只在 confirm 更新，外部重置会失同步）。
// SF05：一次标签求值只读取一次 today。
// UX04：跨年/非当前年保留年份，"同日"按完整 ISO 日期比较（见 rangeLabel）。
const label = computed<string>(() => {
  const v = props.value;
  if (!v) return "全部时间";
  return rangeLabel(v[0], v[1], todayStr());
});

// R03：NDatePicker 用 v-model:formatted-value + value-format 直接桥接
// **字符串**（yyyy-MM-dd）——毫秒值在本机日历与 UTC 锚之间有日常一天
// 的歧义，是手选日期偏一天的根因。控件吐什么字符串就存什么字符串，
// 统计语义 = 控件日历语义，无任何时区重解释。

const shortcutActive = (sc: { fromOffset: number }): boolean => {
  if (draftFrom.value == null || draftTo.value == null) return false;
  const [from, to] = shortcutRange(sc);
  return draftFrom.value === from && draftTo.value === to;
};
</script>

<template>
  <NPopover
    trigger="click"
    :show="show"
    :show-arrow="false"
    placement="bottom-start"
    @update:show="show = $event"
  >
    <template #trigger>
      <!-- 任务 3：与分段控件同族——32px、--ts-fill 底、无描边、8px 圆角 -->
      <button
        type="button"
        class="range-trigger ts-focusable"
        :class="{ 'is-icon-only': iconOnly, 'has-range': iconOnly && value }"
        :aria-label="label"
        :aria-expanded="show"
        :title="iconOnly ? label : undefined"
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
    <div class="range-panel">
      <div class="shortcut-row">
        <NButton
          v-for="sc in shortcuts"
          :key="sc.label"
          size="tiny"
          class="shortcut-btn"
          :type="shortcutActive(sc) ? 'primary' : 'default'"
          @click="applyShortcut(sc)"
        >
          {{ sc.label }}
        </NButton>
      </div>
      <div class="field-label">开始日期</div>
      <NDatePicker
        v-model:formatted-value="draftFrom"
        type="date"
        clearable
        value-format="yyyy-MM-dd"
        placeholder="开始日期"
      />
      <div class="field-label field-label-gap">结束日期</div>
      <NDatePicker
        v-model:formatted-value="draftTo"
        type="date"
        clearable
        :disabled="followToday"
        value-format="yyyy-MM-dd"
        placeholder="结束日期"
      />
      <div class="follow-row">
        <NCheckbox v-model:checked="followToday">结束日跟随今天</NCheckbox>
      </div>
      <div class="actions-row">
        <!-- 清除入口（F07）：一键回"全部时间"，确定时提交 null -->
        <NButton v-if="draftFrom != null || draftTo != null" size="small" quaternary @click="clear">
          清除
        </NButton>
        <span v-else></span>
        <span class="actions-group">
          <NButton size="small" @click="show = false">取消</NButton>
          <NButton size="small" type="primary" @click="confirm">确定</NButton>
        </span>
      </div>
    </div>
  </NPopover>
</template>

<style scoped>
/* 弹层：elevated 玻璃表面（NPopover 浮层内容） */
.range-panel {
  /* UX04：宽度随内容增长（快捷项不裁字），并受视口约束不产生横向滚动 */
  width: max-content;
  min-width: 300px;
  max-width: min(380px, calc(100vw - 32px));
  padding: var(--ts-space-3);
  border-radius: var(--ts-radius-popover);
  /* RC09（date_outer_surface_matches_contract）：材质只保留**一层**——
     Naive 的 .n-popover 外壳已经是 85% elevated + 16px 模糊，这里再叠一次
     就是双层玻璃（两层半透明互相叠加、模糊采样自己的 backing，实际观感比
     单层更浊且对比不稳定）。内层保持透明，只负责排版。 */
  background-color: transparent;
  -webkit-backdrop-filter: none;
  backdrop-filter: none;
}

/* 触发按钮与筛选行分段控件同一族（DESIGN.md §5 筛选栏） */
.range-trigger {
  height: 32px;
  padding: 0 var(--ts-space-3);
  border: none;
  border-radius: var(--ts-radius-control);
  background: var(--ts-fill);
  color: var(--ts-text-secondary);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
}
.range-trigger:hover {
  color: var(--ts-text);
}

.range-trigger.is-icon-only {
  width: 32px;
  padding: 0;
  justify-content: center;
}
.range-trigger.has-range {
  background: var(--ts-accent-soft);
  color: var(--ts-accent);
}

.range-trigger-label {
  margin-right: var(--ts-space-1);
}

.shortcut-row {
  display: inline-flex;
  /* UX04：组宽不足整组换行（不挤压、裁字）；各按钮宽度由文案 + 水平内边距决定 */
  flex-wrap: wrap;
  max-width: 100%;
  border: 1px solid var(--ts-separator);
  border-radius: var(--ts-radius-control);
  overflow: hidden;
  margin-bottom: var(--ts-space-3);
}
.shortcut-btn {
  /* 宽度由文案 + 水平内边距决定；外高 32px（与筛选栏其它控件一致） */
  width: auto;
  min-width: 0;
  height: 32px;
  padding: 0 var(--ts-space-3);
  font-size: 12px;
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 0;
  flex: 0 0 auto;
}
/* 相邻快捷项之间的细分隔线 */
.shortcut-btn + .shortcut-btn {
  border-left: 1px solid var(--ts-separator);
}

.field-label {
  font-size: 12px;
  color: var(--ts-text-muted);
  margin-bottom: var(--ts-space-1);
}
.field-label-gap {
  margin-top: var(--ts-space-3);
}

.follow-row {
  margin-top: var(--ts-space-2);
}
.actions-row {
  display: flex;
  justify-content: space-between;
  margin-top: var(--ts-space-4);
}
.actions-group {
  display: inline-flex;
  gap: var(--ts-space-2);
}
</style>
