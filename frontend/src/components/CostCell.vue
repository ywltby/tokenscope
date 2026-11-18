<script setup lang="ts">
// v0.9.2（2026-11-15，用户产品决策）：费用列**只显示一个金额**——
//
// 主读数 = `total_cost_usd`（后端合并：请求估算 + 可估算日汇总 + 采用汇总
// 的来源记录费用，互不重叠贡献未舍入相加，前端只格式化一次）。
// 旧常驻元素全部移除：来源胶囊、第二行来源金额、大段来源/账单说明。
// v0.9.3：备注符号 `†` 与独立未知列移除——全部/部分未定价由金额色态表达，
// 缺项与依据进悬浮详情。
//
// 悬浮详情（GroupCostBreakdownTooltip，复用请求级浮层配方与 RC06 交互）：
// hover / focus / click 固定三态 + Escape + 外点关闭，键盘可达。
import { computed, ref } from "vue";
import type { Group } from "../types";
import { costDisplayText } from "../lib/costDisplay";
import { useT } from "../lib/i18n";
import GroupCostBreakdownTooltip from "./GroupCostBreakdownTooltip.vue";

const t = useT();

const props = defineProps<{ group: Group }>();

/// 完全没有费用依据 → 「未知」（缺价 ≠ 免费）；单一金额走共享入口。
const unknownOnly = computed(() => props.group.total_cost_usd == null);
const displayText = computed(() => costDisplayText(props.group, t("dashboard.costUnknown")));

// RC06 三态：hover / focus / pinned（固定态外点关闭）；Escape 关闭。
const hoverOpen = ref(false);
const focusOpen = ref(false);
const pinned = ref(false);
const tipOpen = computed(() => hoverOpen.value || focusOpen.value || pinned.value);
function closeTip(): void {
  hoverOpen.value = false;
  focusOpen.value = false;
  pinned.value = false;
}
function togglePin(): void {
  pinned.value = !pinned.value;
}
</script>

<template>
  <GroupCostBreakdownTooltip
    :group="props.group"
    :display-text="displayText"
    :unknown-only="unknownOnly"
    :open="tipOpen"
    :pinned="pinned"
    @hover-enter="hoverOpen = true"
    @hover-leave="hoverOpen = false"
    @focus-enter="focusOpen = true"
    @focus-leave="focusOpen = false"
    @toggle="togglePin"
    @escape="closeTip"
  />
</template>
