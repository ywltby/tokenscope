<script setup lang="ts">
// UX03：请求级费用明细浮层（可访问性组件）——从 EventTable 列渲染函数
// 提取为独立组件，使真实浮层路径可被测试直接挂载。
// 可访问性契约：
// - 触发器名称含当前请求金额（UX07 request 格式）与动作说明；未知价格
//   由父层渲染 NTag（不进入本组件）；
// - hover/focus/pinned 三态分离：hover 随指针、focus 随键盘焦点（focus
//   在时鼠标离开不关闭）、click/Enter/Space 固定切换；Escape 一律关闭；
// - RC06：**固定**后点击浮层与触发器之外的位置必须关闭（父层清除 pinnedKey）；
// - aria-describedby 在打开时指向**存在**的内容节点（稳定唯一 id）。
// 状态（互斥的 openKey）由父层持有，本组件按 props.open 显示并把交互
// 事件转发给父层。
import { computed, h, onBeforeUnmount, onMounted, ref, useId } from "vue";
import { NTooltip } from "naive-ui";
import { formatCostBreakdownRows } from "../lib/costBreakdown";
import { formatMoney } from "../lib/formatMoney";
import { UNIT_PRICE_DENOMINATOR } from "../lib/tokenDisplay";
import type { EventCostBreakdown } from "../types";

const props = defineProps<{
  cost: number;
  breakdown: EventCostBreakdown;
  open: boolean;
  /** RC06：该行的浮层是否由点击**固定**（只有固定态才响应外部点击）。 */
  pinned: boolean;
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

// RC06：外部点击关闭。NTooltip 的 clickoutside 在 trigger="manual" 下不可靠
//（与 HelpTooltip 同一实测结论），这里显式监听文档点击：只响应**固定态**，
// 点触发器自身或浮层内容不算外部；关闭走 toggle，由父层清除 pinnedKey，
// hover/focus 仍各自独立维护。
const triggerRef = ref<HTMLElement | null>(null);
function onDocumentClick(e: MouseEvent): void {
  if (!props.pinned) return;
  const target = e.target as Node | null;
  if (!target) return;
  if (triggerRef.value?.contains(target)) return;
  const body = document.getElementById(descId);
  if (body?.contains(target)) return;
  emit("toggle");
}
onMounted(() => document.addEventListener("click", onDocumentClick));
onBeforeUnmount(() => document.removeEventListener("click", onDocumentClick));

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
  return h("div", { class: "cost-tooltip", id: descId }, [
    ...segments.map((seg, si) =>
      h("div", { class: si === 1 ? "bd-formula ts-card-solid" : undefined }, seg.map(renderRow)),
    ),
    // RC07：单价量纲在费用浮层内自身说明（不能只存在于列头或注释）。
    h("div", { class: "bd-unit" }, `单价量纲：${UNIT_PRICE_DENOMINATOR}（每百万 token）`),
  ]);
}
</script>

<template>
  <NTooltip trigger="manual" placement="left" :show="open" :style="tooltipStyle">
    <template #trigger>
      <!-- RC06：用原生 `button type=button` 而不是 `span role=button`——
          浏览器内建 Enter/Space 激活会各产生**一次** click，因此不再需要
          手写键盘处理（手写 + 原生会重复 toggle）。局部重置外观，保留
          字号/数值对齐/焦点环。外部点击关闭见 onDocumentClick（NTooltip
          的 clickoutside 在 trigger="manual" 下不可靠）。 -->
      <button
        ref="triggerRef"
        type="button"
        class="ts-focusable cost-trigger"
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
      </button>
    </template>
    <component :is="renderContent" />
  </NTooltip>
</template>

<style scoped>
.cost-trigger {
  cursor: help;
  /* RC06：原生 button 的局部外观重置——不引入第二套排版，数字对齐与
     焦点环沿用全局 .ts-focusable。 */
  appearance: none;
  margin: 0;
  border: none;
  background: transparent;
  color: inherit;
  font: inherit;
  padding: 0;
  text-align: inherit;
  font-variant-numeric: inherit;
}
</style>
