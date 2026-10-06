<script setup lang="ts" generic="T extends string">
// 通用分段控件（苹果风格）：radiogroup + 方向键导航 + 选中块平移动画。
// 用于页面切换、来源过滤、聚合维度、主题选择等"多选一"场景。
import { ref, watch, onMounted, nextTick } from "vue";

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
  icon?: string; // 可选图标（来源按钮用）
}

const props = defineProps<{
  modelValue: T;
  options: SegmentOption<T>[];
  ariaLabel?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: T];
}>();

const containerRef = ref<HTMLDivElement | null>(null);
const thumbStyle = ref({ width: "0px", transform: "translateX(0px)" });

function updateThumb() {
  if (!containerRef.value) return;
  const activeIndex = props.options.findIndex((o) => o.value === props.modelValue);
  if (activeIndex === -1) return;
  const items = containerRef.value.querySelectorAll(".ts-segmented-item");
  const activeItem = items[activeIndex] as HTMLElement | undefined;
  if (!activeItem) return;
  const containerRect = containerRef.value.getBoundingClientRect();
  const itemRect = activeItem.getBoundingClientRect();
  thumbStyle.value = {
    width: `${itemRect.width}px`,
    transform: `translateX(${itemRect.left - containerRect.left}px)`,
  };
}

watch(
  () => props.modelValue,
  () => void nextTick(updateThumb),
);
onMounted(() => void nextTick(updateThumb));

function select(value: T) {
  emit("update:modelValue", value);
}

function onKeydown(e: KeyboardEvent) {
  const current = props.options.findIndex((o) => o.value === props.modelValue);
  if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
    e.preventDefault();
    const next = (current - 1 + props.options.length) % props.options.length;
    select(props.options[next].value);
  } else if (e.key === "ArrowRight" || e.key === "ArrowDown") {
    e.preventDefault();
    const next = (current + 1) % props.options.length;
    select(props.options[next].value);
  }
}
</script>

<template>
  <div
    ref="containerRef"
    class="ts-segmented"
    role="radiogroup"
    :aria-label="ariaLabel"
    @keydown="onKeydown"
  >
    <span class="ts-segmented-thumb" :style="thumbStyle" />
    <button
      v-for="opt in options"
      :key="opt.value"
      type="button"
      role="radio"
      class="ts-segmented-item"
      :aria-checked="modelValue === opt.value"
      :tabindex="modelValue === opt.value ? 0 : -1"
      @click="select(opt.value)"
    >
      <span v-if="opt.icon" class="seg-icon" :aria-hidden="true">{{ opt.icon }}</span>
      <span>{{ opt.label }}</span>
    </button>
  </div>
</template>

<style scoped>
.seg-icon {
  font-size: 16px;
  line-height: 1;
}
</style>
