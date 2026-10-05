<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NCheckbox, NDatePicker, NPopover } from "naive-ui";

/**
 * ccs 风格的日期区间选择（M10；C3 修复 F07）：
 * - 弹层受控（:show 双向绑定）——修复前 show 未绑定，取消/确定关不掉、
 *   打开时的草稿同步 watch 永不触发；
 * - 「清除」显式入口（修复前清空起始日后确定被禁用，"全部时间"不可达）；
 * - 快捷条全部按自然日口径命名（原 "24h" 实为昨天+今天两个自然日，更名
 *   "近2天"，不与滚动小时窗口混称）；
 * 日期粒度（存储/过滤均为自然日，换算由 Dashboard 按解析时区完成）。
 */
const props = defineProps<{ value: [number, number] | null }>();
const emit = defineEmits<{ (e: "update:value", v: [number, number] | null): void }>();

const DAY = 86400e3;
const show = ref(false);
const draftFrom = ref<number | null>(null);
const draftTo = ref<number | null>(null);
const followToday = ref(false);

function todayStart(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

const shortcuts: { label: string; range: () => [number, number] }[] = [
  { label: "当天", range: () => [todayStart(), todayStart() + DAY - 1] },
  // 自然日口径：昨天 + 今天两个自然日（后端过滤为自然日闭区间）。
  { label: "近2天", range: () => [todayStart() - DAY, todayStart() + DAY - 1] },
  { label: "近7天", range: () => [todayStart() - 6 * DAY, todayStart() + DAY - 1] },
  { label: "近14天", range: () => [todayStart() - 13 * DAY, todayStart() + DAY - 1] },
  { label: "近30天", range: () => [todayStart() - 29 * DAY, todayStart() + DAY - 1] },
];

function applyShortcut(sc: { label: string; range: () => [number, number] }): void {
  const [from, to] = sc.range();
  draftFrom.value = from;
  draftTo.value = to;
  followToday.value = false;
}

function fmtShort(ms: number): string {
  const d = new Date(ms);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

// 受控开关：打开时草稿同步自当前值；取消/确定经 show 关闭。
watch(show, (open) => {
  if (!open) return;
  if (props.value) {
    draftFrom.value = props.value[0];
    draftTo.value = props.value[1];
    // 结束日为今天视为"跟随今天"勾选态
    followToday.value = draftTo.value >= todayStart();
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
    emit("update:value", [draftFrom.value, todayStart() + DAY - 1]);
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
  if (v[1] >= todayStart() && v[0] < todayStart()) return `${fmtShort(v[0])} ~ 今天`;
  return fmtShort(v[0]) === fmtShort(v[1])
    ? fmtShort(v[0])
    : `${fmtShort(v[0])} ~ ${fmtShort(v[1])}`;
});

const panelStyle = { width: "300px", padding: "12px" };
const shortcutActive = (sc: { range: () => [number, number] }): boolean => {
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
      <NButton size="small" secondary>
        <span style="margin-right: 4px">📅</span>{{ label }}
      </NButton>
    </template>
    <div :style="panelStyle">
      <div class="shortcut-row" style="margin-bottom: 12px">
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
      <div style="font-size: 12px; opacity: 0.65; margin-bottom: 4px">开始日期</div>
      <NDatePicker v-model:value="draftFrom" type="date" clearable placeholder="开始日期" />
      <div style="font-size: 12px; opacity: 0.65; margin: 10px 0 4px">结束日期</div>
      <NDatePicker
        v-model:value="draftTo"
        type="date"
        clearable
        :disabled="followToday"
        placeholder="结束日期"
      />
      <div style="margin-top: 8px">
        <NCheckbox v-model:checked="followToday">结束日跟随今天</NCheckbox>
      </div>
      <div style="display: flex; justify-content: space-between; margin-top: 14px">
        <!-- 清除入口（F07）：一键回"全部时间"，确定时提交 null -->
        <NButton v-if="draftFrom != null || draftTo != null" size="small" quaternary @click="clear">
          清除
        </NButton>
        <span v-else></span>
        <span style="display: inline-flex; gap: 8px">
          <NButton size="small" @click="show = false">取消</NButton>
          <NButton size="small" type="primary" @click="confirm">确定</NButton>
        </span>
      </div>
    </div>
  </NPopover>
</template>

<style scoped>
.shortcut-row {
  display: inline-flex;
  /* 与下方日期输入框同高同浅边框，视觉对齐 */
  border: 1px solid rgba(128, 128, 128, 0.18);
  border-radius: 3px;
  overflow: hidden;
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
  border-left: 1px solid rgba(128, 128, 128, 0.18);
}
</style>
