<script setup lang="ts" generic="T extends string">
// 通用分段控件（苹果风格）：radiogroup + 方向键导航 + 选中块平移动画。
// 用于页面切换、来源过滤、聚合维度、主题选择等"多选一"场景。
//
// UX02（界面审查修复）：
//  - 方向键真正移动 DOM 焦点（修复前只 emit、选中与 tabindex 更新而焦点不动）；
//  - 每个选项可给独立 ariaLabel（真实可访问名称），主题用通用 SVG 插槽渲染；
//  - 底槽用局部 border-box 达到外高 32px（修复前 32 + 上下 padding = 36）；
//  - 观察容器/选项尺寸变化与选项增删，批量 nextTick 重定位 thumb，卸载断开；
//  - 外部改变 modelValue 只重定位 thumb，不抢焦点。
import { nextTick, onBeforeUnmount, onMounted, ref, useSlots, watch } from "vue";
import AgentIcon from "./AgentIcon.vue";

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
  /** 真实可访问名称（含当前值语义）；缺省回退 label */
  ariaLabel?: string;
  /** 可选图标：AgentIcon 品牌图标名（来源筛选"图标 + 文字"用） */
  icon?: string;
}

const props = defineProps<{
  modelValue: T;
  options: SegmentOption<T>[];
  ariaLabel?: string;
  iconOnly?: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: T];
}>();

const slots = useSlots();
// 通用图标插槽：option/index 供父级按需渲染 SVG（主题选择用）。
defineSlots<{
  icon?: (props: { option: SegmentOption<T>; index: number }) => unknown;
}>();
const containerRef = ref<HTMLDivElement | null>(null);
const thumbStyle = ref({ width: "0px", transform: "translateX(0px)" });

function items(): HTMLButtonElement[] {
  const c = containerRef.value;
  return c ? Array.from(c.querySelectorAll<HTMLButtonElement>(".ts-segmented-item")) : [];
}

function updateThumb(): void {
  const container = containerRef.value;
  if (!container) return;
  const activeIndex = props.options.findIndex((o) => o.value === props.modelValue);
  const activeItem = activeIndex === -1 ? undefined : items()[activeIndex];
  if (!activeItem) {
    thumbStyle.value = { width: "0px", transform: "translateX(0px)" };
    return;
  }
  const containerRect = container.getBoundingClientRect();
  const itemRect = activeItem.getBoundingClientRect();
  thumbStyle.value = {
    width: `${itemRect.width}px`,
    transform: `translateX(${itemRect.left - containerRect.left}px)`,
  };
}

/// 方向键更新选中值后聚焦对应按钮（UX02：焦点必须真实移动）。
function selectAndFocus(value: T, index: number): void {
  emit("update:modelValue", value);
  void nextTick(() => {
    updateThumb();
    items()[index]?.focus();
  });
}

function select(value: T): void {
  emit("update:modelValue", value);
  void nextTick(updateThumb);
}

function onKeydown(e: KeyboardEvent): void {
  const len = props.options.length;
  if (len === 0) return; // 空 options 安全无动作
  const current = props.options.findIndex((o) => o.value === props.modelValue);
  const base = current === -1 ? 0 : current;
  if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
    e.preventDefault();
    const next = (base - 1 + len) % len;
    selectAndFocus(props.options[next].value, next);
  } else if (e.key === "ArrowRight" || e.key === "ArrowDown") {
    e.preventDefault();
    const next = (base + 1) % len;
    selectAndFocus(props.options[next].value, next);
  }
}

// 外部改变选中值：只重定位 thumb，不抢焦点（区别于方向键路径）。
watch(
  () => props.modelValue,
  () => void nextTick(updateThumb),
);
// 选项增删/文案变化：下一帧重定位（批处理，避免逐项重排）。
watch(
  () => props.options,
  () => void nextTick(updateThumb),
  { deep: false },
);

let observer: ResizeObserver | null = null;
onMounted(() => {
  void nextTick(updateThumb);
  // 观察容器与每个选项：选中项尺寸/选项增删都会重定位 thumb。
  observer = new ResizeObserver(() => void nextTick(updateThumb));
  if (containerRef.value) {
    observer.observe(containerRef.value);
    for (const item of items()) observer.observe(item);
  }
});

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
});
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
      v-for="(opt, i) in options"
      :key="opt.value"
      type="button"
      role="radio"
      class="ts-segmented-item"
      :class="{ 'is-icon-only': iconOnly }"
      :title="iconOnly ? (opt.ariaLabel ?? opt.label) : undefined"
      :aria-checked="modelValue === opt.value"
      :aria-label="opt.ariaLabel ?? opt.label"
      :tabindex="modelValue === opt.value ? 0 : -1"
      @click="select(opt.value)"
    >
      <!-- 通用 SVG 渲染插槽：主题等需要语义化图形时使用；装饰图形必须
           aria-hidden，真实名称由按钮 aria-label 承载。 -->
      <span v-if="slots.icon" class="seg-icon" aria-hidden="true">
        <slot name="icon" :option="opt" :index="i" />
      </span>
      <span v-else-if="opt.icon" class="seg-icon" aria-hidden="true">
        <AgentIcon :name="opt.icon" :size="16" />
      </span>
      <span v-if="!iconOnly">{{ opt.label }}</span>
    </button>
  </div>
</template>

<style scoped>
.ts-segmented-item.is-icon-only {
  display: inline-flex;
  align-items: center;
  width: 32px;
  padding: 0;
  justify-content: center;
}

.seg-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 16px;
  line-height: 1;
}
</style>
