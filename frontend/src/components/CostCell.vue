<script setup lang="ts">
// v0.9.2（2026-11-15，用户产品决策）：费用列**只显示一个金额**——
//
// 主读数 = `total_cost_usd`（后端合并：请求估算 + 可估算日汇总 + 采用汇总
// 的来源记录费用，互不重叠贡献未舍入相加，前端只格式化一次）。
// 旧常驻元素全部移除：来源胶囊、第二行来源金额、大段来源/账单说明。
// 真实费用缺失只保留短标记 `†`（cost_partially_unknown），缺项进悬浮详情。
//
// 悬浮详情（GroupCostBreakdownTooltip，复用请求级浮层配方与 RC06 交互）：
// hover / focus / click 固定三态 + Escape + 外点关闭，键盘可达。
import { computed, ref } from "vue";
import type { Group } from "../types";
import { formatMoney } from "../lib/formatMoney";
import { useT } from "../lib/i18n";
import GroupCostBreakdownTooltip from "./GroupCostBreakdownTooltip.vue";

const t = useT();

const props = defineProps<{ group: Group }>();

/// 单一显示金额：后端合并值；None = 组内完全没有费用依据 → 未知。
const hasTotal = computed(() => props.group.total_cost_usd != null);
const totalText = computed(() =>
  hasTotal.value ? formatMoney(props.group.total_cost_usd as number, "summary") : "",
);
/// 完全没有费用依据 → 未知†（缺价 ≠ 免费，G-P3-1）。
const unknownOnly = computed(() => !hasTotal.value);
const displayText = computed(() =>
  unknownOnly.value ? t("dashboard.costUnknown") : totalText.value,
);

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

<style scoped>
/* v0.9.2：费用列不再有本地排版——单金额 + 悬浮由 GroupCostBreakdownTooltip
   承载。本组件保留 scoped 样式块以维持 SFC scope 标记（缺陷 B 的回归锚点：
   UsageTable render 回调创建的节点不带 scope，scoped CSS 永不命中导致粘行；
   独立 SFC 的 scope 属性是防回归断言的载体），规则作用于自身模板根节点。 */
:where(section, span) {
  /* 无实际样式——仅维持 scope id 生成。 */
}
</style>
