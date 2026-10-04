<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NCheckbox, NDatePicker, NPopover } from "naive-ui";

/**
 * ccs 风格的日期区间选择（M10）：
 * 触发器显示当前选择；面板内为快捷条（当天/7d/14d/30d）+ 起止日期
 * + 「结束日跟随今天」+ 取消/确定。日期粒度（存储/过滤均为自然日，
 * 时区换算由 Dashboard 按解析时区完成），故未含 ccs 的时分列。
 */
const props = defineProps<{ value: [number, number] | null }>();
const emit = defineEmits<{ (e: "update:value", v: [number, number] | null): void }>();

const DAY = 86400e3;
const show = ref(false);
const draftFrom = ref<number | null>(null);
const draftTo = ref<number | null>(null);
const followToday = ref(false);
const label = ref("全部时间");

function todayStart(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

const shortcuts: { label: string; range: () => [number, number] }[] = [
  { label: "当天", range: () => [todayStart(), todayStart() + DAY - 1] },
  { label: "24h", range: () => [todayStart() - DAY, todayStart() + DAY - 1] },
  { label: "7d", range: () => [todayStart() - 6 * DAY, todayStart() + DAY - 1] },
  { label: "14d", range: () => [todayStart() - 13 * DAY, todayStart() + DAY - 1] },
  { label: "30d", range: () => [todayStart() - 29 * DAY, todayStart() + DAY - 1] },
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
    label.value = "全部时间";
  } else if (followToday.value) {
    // 结束日跟随今天：自然日粒度下等价于"从起始日至今（含未来）"
    emit("update:value", [draftFrom.value, todayStart() + DAY - 1]);
    label.value = `${fmtShort(draftFrom.value)} ~ 今天`;
  } else if (draftTo.value == null) {
    emit("update:value", [draftFrom.value, draftFrom.value]);
    label.value = fmtShort(draftFrom.value);
  } else {
    emit("update:value", [draftFrom.value, draftTo.value]);
    label.value =
      fmtShort(draftFrom.value) === fmtShort(draftTo.value)
        ? fmtShort(draftFrom.value)
        : `${fmtShort(draftFrom.value)} ~ ${fmtShort(draftTo.value)}`;
  }
  show.value = false;
}

const panelStyle = { width: "300px", padding: "12px" };
const shortcutActive = (sc: { range: () => [number, number] }): boolean => {
  if (draftFrom.value == null || draftTo.value == null) return false;
  const [from, to] = sc.range();
  return draftFrom.value === from && draftTo.value === to;
};
const hasDraft = computed(() => draftFrom.value != null);
</script>

<template>
  <NPopover trigger="click" :show-arrow="false" placement="bottom-start">
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
      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px">
        <NButton size="small" @click="show = false">取消</NButton>
        <NButton size="small" type="primary" :disabled="!hasDraft" @click="confirm">确定</NButton>
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
  height: 34px;
  border: none;
  border-radius: 0;
}
/* 相邻快捷项之间的细分隔线 */
.shortcut-btn + .shortcut-btn {
  border-left: 1px solid rgba(128, 128, 128, 0.18);
}
</style>
