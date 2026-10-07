<script setup lang="ts">
// UX03（设置页说明浮层可访问性）：可复用的说明浮层——hover/focus/click
// 三态分离（R09/F08 同一策略）：hover 随指针、focus 随键盘焦点（focus
// 在时鼠标离开不关闭）、click/Enter/Space 固定切换；Escape 一律关闭
// 且焦点不离开触发器（tooltip_escape_closes_without_losing_trigger）。
// 触发器有语义 aria-label；浮层打开时 aria-describedby 指向**存在**
// 的内容节点（Vue useId 保证稳定唯一）。
import { computed, ref, useId } from "vue";
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
</script>

<template>
  <NTooltip placement="top" trigger="manual" :show="open">
    <template #trigger>
      <span
        class="ts-focusable help-trigger"
        tabindex="0"
        role="button"
        :aria-label="props.label"
        :aria-expanded="open"
        :aria-describedby="open ? descId : undefined"
        @mouseenter="hoverOpen = true"
        @mouseleave="hoverOpen = false"
        @focus="focusOpen = true"
        @blur="focusOpen = false"
        @click="pinned = !pinned"
        @keydown.escape="close"
        @keydown.enter.prevent="pinned = !pinned"
        @keydown.space.prevent="pinned = !pinned"
      >
        <slot name="trigger" />
      </span>
    </template>
    <div :id="descId" class="help-tooltip-body" :aria-label="props.contentLabel">
      <slot />
    </div>
  </NTooltip>
</template>

<style scoped>
.help-trigger {
  cursor: help;
}
.help-tooltip-body {
  font-size: 12px;
  line-height: 1.8;
  max-width: 320px;
}
</style>
