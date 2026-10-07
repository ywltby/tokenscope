<script setup lang="ts">
// UX03：请求级费用明细浮层（可访问性组件）——从 EventTable 列渲染函数
// 提取为独立组件，使真实浮层路径可被测试直接挂载。
// 可访问性契约：
// - 触发器名称含当前请求金额（UX07 request 格式）与动作说明；未知价格
//   由父层渲染 NTag（不进入本组件）；
// - hover/focus/pinned 三态分离：hover 随指针、focus 随键盘焦点（focus
//   在时鼠标离开不关闭）、click/Enter/Space 固定切换；Escape 一律关闭；
// - aria-describedby 在打开时指向**存在**的内容节点（稳定唯一 id）。
// 状态（互斥的 openKey）由父层持有，本组件按 props.open 显示并把交互
// 事件转发给父层。
import { computed, h, useId } from "vue";
import { NTooltip } from "naive-ui";
import { formatCostBreakdownRows } from "../lib/costBreakdown";
import { formatMoney } from "../lib/formatMoney";
import type { EventCostBreakdown } from "../types";

const props = defineProps<{
  cost: number;
  breakdown: EventCostBreakdown;
  open: boolean;
  /** 内容浮层 style（EventTable 的 elevated 玻璃配方原样透传）。 */
  tooltipStyle?: string;
}>();

const emit = defineEmits<{
  (e: "hover-enter"): void;
  (e: "hover-leave"): void;
  (e: "focus-enter"): void;
  (e: "focus-leave"): void;
  (e: "toggle"): void;
  (e: "escape"): void;
}>();

const ariaLabel = computed(() => `估算费用 ${formatMoney(props.cost, "request")}，查看计算明细`);
const descId = `ts-cost-bd-${useId()}`;

function renderContent(): ReturnType<typeof h> {
  // 事实 → 公式 → 结果 → 来源：divider 分段渲染，公式段（第 2 段）
  // 套 .ts-card-solid 实色衬底保证可读性（DESIGN.md §5 费用明细）。
  const rows = formatCostBreakdownRows(props.breakdown);
  const segments: (typeof rows)[] = [];
  let cur: typeof rows = [];
  for (const r of rows) {
    if (r.divider) {
      segments.push(cur);
      cur = [];
    } else {
      cur.push(r);
    }
  }
  segments.push(cur);
  const renderRow = (row2: (typeof rows)[number]) => {
    if (row2.detail != null) {
      return h(
        "div",
        { class: ["bd-row", row2.unknown ? "bd-unknown" : ""] },
        `${row2.label} ${row2.detail}`,
      );
    }
    return h(
      "div",
      {
        class: ["bd-row", row2.unknown ? "bd-unknown" : "", row2.total ? "bd-total" : ""],
      },
      [
        h("span", { class: "bd-label" }, row2.label),
        h("span", { class: "bd-value ts-num" }, row2.value ?? ""),
      ],
    );
  };
  return h(
    "div",
    { class: "cost-tooltip", id: descId },
    segments.map((seg, si) =>
      h("div", { class: si === 1 ? "bd-formula ts-card-solid" : undefined }, seg.map(renderRow)),
    ),
  );
}
</script>

<template>
  <NTooltip
    trigger="manual"
    placement="left"
    :show="open"
    :style="tooltipStyle"
    @clickoutside="emit('escape')"
  >
    <template #trigger>
      <span
        class="ts-focusable cost-trigger"
        tabindex="0"
        role="button"
        :aria-label="ariaLabel"
        :aria-expanded="open"
        :aria-describedby="open ? descId : undefined"
        @mouseenter="emit('hover-enter')"
        @mouseleave="emit('hover-leave')"
        @focus="emit('focus-enter')"
        @blur="emit('focus-leave')"
        @click="emit('toggle')"
        @keydown.escape="emit('escape')"
      >
        {{ formatMoney(cost, "request") }}
      </span>
    </template>
    <component :is="renderContent" />
  </NTooltip>
</template>

<style scoped>
.cost-trigger {
  cursor: help;
}
</style>
