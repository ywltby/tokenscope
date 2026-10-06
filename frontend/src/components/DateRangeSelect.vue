<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NCheckbox, NDatePicker, NPopover } from "naive-ui";
import { addDays, calendarToMs, msToUtcCalendar, todayInTz } from "../lib/dates";

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
const props = defineProps<{ value: [string, string] | null; tz: string }>();
const emit = defineEmits<{ (e: "update:value", v: [string, string] | null): void }>();

const show = ref(false);
const draftFrom = ref<string | null>(null);
const draftTo = ref<string | null>(null);
const followToday = ref(false);

function todayStr(): string {
  return todayInTz(props.tz);
}

const shortcuts: { label: string; range: () => [string, string] }[] = [
  { label: "当天", range: () => [todayStr(), todayStr()] },
  // 自然日口径：昨天 + 今天两个自然日（后端过滤为自然日闭区间）。
  { label: "近2天", range: () => [addDays(todayStr(), -1), todayStr()] },
  { label: "近7天", range: () => [addDays(todayStr(), -6), todayStr()] },
  { label: "近14天", range: () => [addDays(todayStr(), -13), todayStr()] },
  { label: "近30天", range: () => [addDays(todayStr(), -29), todayStr()] },
];

function applyShortcut(sc: { label: string; range: () => [string, string] }): void {
  const [from, to] = sc.range();
  draftFrom.value = from;
  draftTo.value = to;
  followToday.value = false;
}

function fmtShort(dateStr: string): string {
  const [, m, d] = dateStr.split("-");
  return `${Number(m)}/${Number(d)}`;
}

// 受控开关：打开时草稿同步自当前值；取消/确定经 show 关闭。
watch(show, (open) => {
  if (!open) return;
  if (props.value) {
    draftFrom.value = props.value[0];
    draftTo.value = props.value[1];
    // 结束日为今天视为"跟随今天"勾选态
    followToday.value = draftTo.value >= todayStr();
  } else {
    draftFrom.value = null;
    draftTo.value = null;
    followToday.value = false;
  }
});

function confirm(): void {
  if (draftFrom.value == null) {
    emit("update:value", null);
  } else if (followToday.value) {
    // 结束日跟随今天：自然日粒度下等价于"从起始日至今（含未来）"
    emit("update:value", [draftFrom.value, todayStr()]);
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
const label = computed<string>(() => {
  const v = props.value;
  if (!v) return "全部时间";
  if (v[1] >= todayStr() && v[0] < todayStr()) return `${fmtShort(v[0])} ~ 今天`;
  return fmtShort(v[0]) === fmtShort(v[1])
    ? fmtShort(v[0])
    : `${fmtShort(v[0])} ~ ${fmtShort(v[1])}`;
});

// NDatePicker 以毫秒为输入：日历字符串 ↔ UTC 零点毫秒的桥接（仅组件
// 输入，不携带统计语义——统计语义是字符串本身）。
const draftFromMs = computed<number | null>({
  get: () => (draftFrom.value ? calendarToMs(draftFrom.value) : null),
  set: (ms) => {
    // 审阅 Task 1：picker 毫秒是 UTC 零点锚的 UI 桥接值，setter 用同一
    // UTC 日历语义还原——统计时区不得重解释（负偏移时会变前一天）。
    draftFrom.value = ms == null ? null : msToUtcCalendar(ms);
  },
});
const draftToMs = computed<number | null>({
  get: () => (draftTo.value ? calendarToMs(draftTo.value) : null),
  set: (ms) => {
    draftTo.value = ms == null ? null : msToUtcCalendar(ms);
  },
});

const shortcutActive = (sc: { range: () => [string, string] }): boolean => {
  if (draftFrom.value == null || draftTo.value == null) return false;
  const [from, to] = sc.range();
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
      <button type="button" class="range-trigger ts-focusable">
        <span class="range-trigger-label">{{ label }}</span>
      </button>
    </template>
    <div class="range-panel ts-glass">
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
      <NDatePicker v-model:value="draftFromMs" type="date" clearable placeholder="开始日期" />
      <div class="field-label field-label-gap">结束日期</div>
      <NDatePicker
        v-model:value="draftToMs"
        type="date"
        clearable
        :disabled="followToday"
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
  width: 300px;
  padding: var(--ts-space-3);
  border-radius: var(--ts-radius-popover);
  background-color: var(--ts-surface-elevated);
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

.range-trigger-label {
  margin-right: var(--ts-space-1);
}

.shortcut-row {
  display: inline-flex;
  border: 1px solid var(--ts-separator);
  border-radius: var(--ts-radius-control);
  overflow: hidden;
  margin-bottom: var(--ts-space-3);
}
.shortcut-btn {
  width: 34px;
  height: 34px;
  padding: 0;
  font-size: 12px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 0;
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
