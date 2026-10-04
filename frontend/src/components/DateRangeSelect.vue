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

const shortcuts: { label: string; days: number }[] = [
  { label: "当天", days: 1 },
  { label: "7d", days: 7 },
  { label: "14d", days: 14 },
  { label: "30d", days: 30 },
];

function applyShortcut(days: number): void {
  draftFrom.value = todayStart() - (days - 1) * DAY;
  draftTo.value = todayStart() + DAY - 1;
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

const panelStyle = { width: "420px", padding: "12px" };
const shortcutActive = (days: number): boolean => {
  if (draftFrom.value == null || draftTo.value == null) return false;
  return draftTo.value - draftFrom.value + 1 === days * DAY;
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
      <div style="display: flex; gap: 6px; margin-bottom: 12px">
        <NButton
          v-for="sc in shortcuts"
          :key="sc.label"
          size="tiny"
          :type="shortcutActive(sc.days) ? 'primary' : 'default'"
          @click="applyShortcut(sc.days)"
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
