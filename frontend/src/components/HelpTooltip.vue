<script setup lang="ts">
// UX03（设置页说明浮层可访问性）：可复用的说明浮层——hover/focus/click
// 三态分离（R09/F08 同一策略）：hover 随指针、focus 随键盘焦点（focus
// 在时鼠标离开不关闭）、click/Enter/Space 固定切换；Escape 一律关闭
// 且焦点不离开触发器（tooltip_escape_closes_without_losing_trigger）。
// 触发器有语义 aria-label；浮层打开时 aria-describedby 指向**存在**
// 的内容节点（Vue useId 保证稳定唯一）。
import { computed, onBeforeUnmount, onMounted, ref, useId } from "vue";
import { NTooltip } from "naive-ui";

const props = defineProps<{
  /** 触发器的可访问名称（如「输入单价说明」）。 */
  label: string;
  /** 浮层内容承载节点的可访问角色描述（可选）。 */
  contentLabel?: string;
}>();

const hoverOpen = ref(false);
const focusOpen = ref(false);
const pinned = ref(false);
const open = computed(() => hoverOpen.value || focusOpen.value || pinned.value);
function close(): void {
  hoverOpen.value = false;
  focusOpen.value = false;
  pinned.value = false;
}
// 稳定且唯一的描述关联 ID（浮层打开时内容节点必然存在）。
const descId = `ts-help-${useId()}`;

// RC06：外部点击关闭。NTooltip 的 clickoutside 在 trigger="manual" 下不可靠
//（实测点击文档不关闭），这里显式监听文档点击：点击**固定**后移开指针、
// 再点其它位置必须关闭并清除 aria-describedby；点触发器自身或浮层内容不算外部。
const triggerRef = ref<HTMLElement | null>(null);
function onDocumentClick(e: MouseEvent): void {
  if (!pinned.value) return;
  const target = e.target as Node | null;
  if (!target) return;
  if (triggerRef.value?.contains(target)) return;
  const body = document.getElementById(descId);
  if (body?.contains(target)) return;
  close();
}
onMounted(() => document.addEventListener("click", onDocumentClick));
onBeforeUnmount(() => document.removeEventListener("click", onDocumentClick));
</script>

<template>
  <NTooltip placement="top" trigger="manual" :show="open">
    <template #trigger>
      <!-- RC06：原生 `button type=button` 提供浏览器内建 Enter/Space 激活
           （各一次 click）；删除手写 enter/space 处理，避免与原生重复 toggle。 -->
      <button
        ref="triggerRef"
        type="button"
        class="ts-focusable help-trigger"
        :aria-label="props.label"
        :aria-expanded="open"
        :aria-describedby="open ? descId : undefined"
        @mouseenter="hoverOpen = true"
        @mouseleave="hoverOpen = false"
        @focus="focusOpen = true"
        @blur="focusOpen = false"
        @click="pinned = !pinned"
        @keydown.escape="close"
      >
        <slot name="trigger" />
      </button>
    </template>
    <div :id="descId" class="help-tooltip-body" :aria-label="props.contentLabel">
      <slot />
    </div>
  </NTooltip>
</template>

<style scoped>
.help-trigger {
  cursor: help;
  /* RC06：原生 button 的局部外观重置——保留字号/颜色/对齐与焦点环。 */
  appearance: none;
  margin: 0;
  border: none;
  background: transparent;
  color: inherit;
  font: inherit;
  padding: 0;
  text-align: inherit;
}
.help-tooltip-body {
  font-size: 12px;
  line-height: 1.8;
  max-width: 320px;
}
</style>
