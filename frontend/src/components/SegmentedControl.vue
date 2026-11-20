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
import {
  nextTick,
  onActivated,
  onBeforeUnmount,
  onDeactivated,
  onMounted,
  ref,
  useSlots,
  watch,
} from "vue";
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
const thumbStyle = ref({ width: "0px", height: "0px", top: "0px", transform: "translateX(0px)" });
const thumbReady = ref(false);
const thumbAnimating = ref(false);
let measuredValue: T | null = null;

function releaseThumb(): void {
  thumbReady.value = false;
  thumbAnimating.value = false;
}

function items(): HTMLButtonElement[] {
  const c = containerRef.value;
  return c ? Array.from(c.querySelectorAll<HTMLButtonElement>(".ts-segmented-item")) : [];
}

function updateThumb(animateSelection = false): void {
  const container = containerRef.value;
  if (!container) return;
  const activeIndex = props.options.findIndex((o) => o.value === props.modelValue);
  const activeItem = activeIndex === -1 ? undefined : items()[activeIndex];
  if (!activeItem) {
    releaseThumb();
    return;
  }
  const containerRect = container.getBoundingClientRect();
  const itemRect = activeItem.getBoundingClientRect();
  if (
    containerRect.width <= 0 ||
    containerRect.height <= 0 ||
    itemRect.width <= 0 ||
    itemRect.height <= 0 ||
    ![
      containerRect.left,
      containerRect.top,
      containerRect.width,
      containerRect.height,
      itemRect.left,
      itemRect.top,
      itemRect.width,
      itemRect.height,
    ].every(Number.isFinite)
  ) {
    releaseThumb();
    return;
  }
  // absolute 原点在容器描边内侧；不能把 clientLeft/clientTop 重复计入位移。
  const next = {
    width: `${itemRect.width}px`,
    height: `${itemRect.height}px`,
    top: `${itemRect.top - containerRect.top - container.clientTop + container.scrollTop}px`,
    transform: `translateX(${itemRect.left - containerRect.left - container.clientLeft + container.scrollLeft}px)`,
  };
  const changedValue = measuredValue !== props.modelValue;
  if (
    thumbReady.value &&
    !changedValue &&
    Object.entries(next).every(([k, v]) => thumbStyle.value[k as keyof typeof next] === v)
  )
    return;
  // 相同尺寸的 observer 回调不打断正在发生的真实选择动画；有效布局变化
  // 则立即对齐。首次/隐藏恢复时由按钮材质切到已对齐 thumb，绝不从 0px 展开。
  thumbAnimating.value = animateSelection && thumbReady.value && changedValue;
  thumbStyle.value = next;
  measuredValue = props.modelValue;
  thumbReady.value = true;
}

/// 方向键更新选中值后聚焦对应按钮（UX02：焦点必须真实移动）。
function selectAndFocus(value: T, index: number): void {
  emit("update:modelValue", value);
  void nextTick(() => {
    items()[index]?.focus();
  });
}

function select(value: T): void {
  emit("update:modelValue", value);
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
  () => updateThumb(true),
  { flush: "post" },
);
// 选项增删/文案变化：下一帧重定位（批处理，避免逐项重排）。
watch(
  () => props.options,
  () => {
    observeItems();
    updateThumb();
  },
  { deep: true, flush: "post" },
);

let observer: ResizeObserver | null = null;
const observedItems = new Set<HTMLButtonElement>();
function observeItems(): void {
  if (!observer) return;
  const current = new Set(items());
  for (const item of observedItems)
    if (!current.has(item)) {
      observer.unobserve(item);
      observedItems.delete(item);
    }
  for (const item of current)
    if (!observedItems.has(item)) {
      observer.observe(item);
      observedItems.add(item);
    }
}
onMounted(() => {
  void nextTick(() => updateThumb());
  // 观察容器与每个选项：选中项尺寸/选项增删都会重定位 thumb。
  observer = new ResizeObserver(() => void nextTick(() => updateThumb()));
  if (containerRef.value) {
    observer.observe(containerRef.value);
    observeItems();
  }
});

onDeactivated(releaseThumb);
onActivated(() => void nextTick(() => updateThumb()));

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
  observedItems.clear();
});
</script>

<template>
  <div
    ref="containerRef"
    class="ts-segmented"
    :class="{ 'is-thumb-ready': thumbReady, 'is-thumb-animating': thumbAnimating }"
    role="radiogroup"
    :aria-label="ariaLabel"
    @keydown="onKeydown"
  >
    <span class="ts-segmented-thumb" :style="thumbStyle" aria-hidden="true" />
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
