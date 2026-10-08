<script setup lang="ts" generic="T">
import { computed, nextTick, ref, watch } from "vue";

const props = defineProps<{
  rows: T[];
  total?: number;
  more?: boolean;
  loading?: boolean;
  blocked?: string;
  label: string;
}>();
const emit = defineEmits<{ (e: "load-more"): void }>();
const viewport = ref<HTMLElement | null>(null);
const limit = ref(200);
const visibleRows = computed(() => props.rows.slice(0, limit.value));
const hasMore = computed(() => limit.value < props.rows.length || !!props.more);
let requestedLength = -1;
let revealing = false;
let engaged = false;

function onScroll(): void {
  if ((viewport.value?.scrollTop ?? 0) > 0) engaged = true;
  if (engaged) void checkBottom();
}
function onScrollIntent(event: WheelEvent | KeyboardEvent): void {
  if ("deltaY" in event ? event.deltaY <= 0 : !["End", "PageDown", "ArrowDown"].includes(event.key))
    return;
  engaged = true;
  void nextTick(checkBottom);
}

async function checkBottom(): Promise<void> {
  const el = viewport.value;
  if (!el || el.clientHeight === 0 || el.scrollHeight - el.scrollTop - el.clientHeight > 160)
    return;
  if (revealing || props.loading || props.blocked) return;
  if (limit.value < props.rows.length) {
    revealing = true;
    limit.value += 200;
    await nextTick();
    revealing = false;
  } else if (props.more && requestedLength !== props.rows.length) {
    requestedLength = props.rows.length;
    emit("load-more");
  }
}

// 追加保留首行对象；刷新/换筛选替换首行，恢复首批并清除旧请求资格。
watch(
  () => props.rows[0],
  () => {
    limit.value = 200;
    requestedLength = -1;
    engaged = false;
    if (viewport.value) viewport.value.scrollTop = 0;
  },
);
watch(
  () => [props.rows.length, props.loading, props.blocked],
  async () => {
    await nextTick();
    if (engaged) void checkBottom();
  },
);
</script>

<template>
  <div
    ref="viewport"
    class="scroll-list"
    :class="{ 'is-progressive': (total ?? rows.length) > 200 }"
    role="region"
    :aria-label="label"
    tabindex="0"
    @scroll.passive="onScroll"
    @wheel.passive="onScrollIntent"
    @keydown="onScrollIntent"
  >
    <slot :rows="visibleRows" />
    <div v-if="hasMore || blocked" class="scroll-list-status" role="status">
      {{ blocked || (loading ? "正在加载…" : "向下滚动自动加载更多") }}
    </div>
  </div>
</template>

<style scoped>
.scroll-list {
  overflow: auto;
  max-height: 420px;
}
.scroll-list:focus-visible {
  outline: 2px solid var(--ts-accent);
  outline-offset: 2px;
}
.scroll-list-status {
  padding: 10px;
  text-align: center;
  color: var(--ts-text-secondary);
  font-size: 12px;
}
</style>
