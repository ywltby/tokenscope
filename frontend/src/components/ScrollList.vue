<script setup lang="ts" generic="T">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { t } from "../lib/translate";

interface AnchorCandidate {
  key: string;
  topOffset: number;
}

interface ScrollAnchor {
  scrollTop: number;
  candidates: AnchorCandidate[];
}

const props = defineProps<{
  rows: T[];
  total?: number;
  more?: boolean;
  loading?: boolean;
  blocked?: string;
  label: string;
  /**
   * 筛选/内容重置身份：
   * - 提供时仅在该身份变化时恢复首批并回到顶部；同身份下的刷新（替换行对象、
   *   顶部新增行、行重排）保留已展开批次，并尽量将当前正在看的那一行留在原位置。
   * - 未提供时保持既有「首行对象引用变化即重置」兼容语义。
   */
  resetKey?: string | null;
  /**
   * 行稳定身份键（如聚合表 group.key、明细表稳定 row_id、价格表 source|prefix）。
   * 提供后，同筛选刷新时按可见锚点行重新对齐 scrollTop，即使上方新增/重排行也能把
   * 正在看的那一行留在视野原位置。
   */
  rowKey?: (row: T) => string;
}>();
const emit = defineEmits<{ (e: "load-more"): void }>();
const viewport = ref<HTMLElement | null>(null);
const limit = ref(200);
const visibleRows = computed(() => props.rows.slice(0, limit.value));
const hasMore = computed(() => limit.value < props.rows.length || !!props.more);
let requestedLength = -1;
let revealing = false;
let engaged = false;
let restoringScroll = false;

let renderedKeys: string[] = [];
let pendingAnchor: ScrollAnchor | null = null;
let resetTriggered = false;
let reconcileSeq = 0;
let lastRowsRef: T[] = props.rows;
let lastRowCount = props.rows.length;
let lastFirstKey: string | null = extractKey(props.rows[0]);

function extractKey(row: T | undefined): string | null {
  return row !== undefined && props.rowKey ? props.rowKey(row) : null;
}

function keysOf(rows: T[]): string[] {
  return props.rowKey ? rows.map((r) => props.rowKey!(r)) : [];
}

function queryRowElements(el: HTMLElement): HTMLElement[] {
  const naive = el.querySelectorAll<HTMLElement>(".n-data-table-tbody > tr.n-data-table-tr");
  if (naive.length > 0) return Array.from(naive);
  return Array.from(el.querySelectorAll<HTMLElement>("tbody > tr"));
}

function captureAnchor(el: HTMLElement | null, keys: string[]): ScrollAnchor | null {
  if (!el) return null;
  const scrollTop = el.scrollTop;
  if (scrollTop <= 0) return { scrollTop: 0, candidates: [] };
  if (keys.length === 0) return { scrollTop, candidates: [] };
  const rowEls = queryRowElements(el);
  if (rowEls.length !== keys.length) return { scrollTop, candidates: [] };
  const vpRect = el.getBoundingClientRect();
  let hasLayout = false;
  const rects: DOMRect[] = [];
  for (const tr of rowEls) {
    const r = tr.getBoundingClientRect();
    if (r.height > 0) hasLayout = true;
    rects.push(r);
  }
  if (!hasLayout) return { scrollTop, candidates: [] };
  let firstVisible = -1;
  for (let i = 0; i < rects.length; i++) {
    if (rects[i].bottom > vpRect.top + 1) {
      firstVisible = i;
      break;
    }
  }
  if (firstVisible < 0) firstVisible = rects.length - 1;
  const candidates: AnchorCandidate[] = [];
  const pushAt = (idx: number) => {
    if (idx < 0 || idx >= keys.length) return;
    candidates.push({
      key: keys[idx],
      topOffset: rects[idx].top - vpRect.top,
    });
  };
  // 先向下取可视区内的行作为主/备选锚点，再补紧邻上方行防止主锚点被移除。
  for (let i = firstVisible; i < Math.min(keys.length, firstVisible + 10); i++) {
    pushAt(i);
  }
  for (let i = firstVisible - 1; i >= Math.max(0, firstVisible - 4); i--) {
    pushAt(i);
  }
  return { scrollTop, candidates };
}

onMounted(() => {
  renderedKeys = keysOf(visibleRows.value);
});

function onScroll(): void {
  if (restoringScroll) return;
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

// 1) 在 DOM 变更前同步抓取当前视口内的锚点行及其相对顶边偏移。
watch(
  () => props.rows,
  (nextRows, prevRows) => {
    if (nextRows !== prevRows && pendingAnchor === null) {
      pendingAnchor = captureAnchor(viewport.value, renderedKeys);
    }
  },
  { flush: "sync" },
);

// 2) 筛选/内容身份变化才恢复首批并回到顶部（pre flush：单次渲染即落地）。
watch(
  () => props.resetKey ?? props.rows[0],
  () => {
    resetTriggered = true;
    pendingAnchor = null;
    reconcileSeq += 1;
    limit.value = 200;
    requestedLength = -1;
    engaged = false;
    if (viewport.value) viewport.value.scrollTop = 0;
  },
);

// 3) 非追加的同筛选刷新替换整批 rows 时，清除旧分页请求资格与自动续页粘滞，
//    避免刷新后因容器贴近底部而自发连环 load-more。
watch(
  () => props.rows,
  (nextRows) => {
    const nextFirstKey = extractKey(nextRows[0]);
    const isAppend =
      nextRows.length > lastRowCount &&
      lastRowCount > 0 &&
      (props.rowKey ? nextFirstKey === lastFirstKey : nextRows[0] === lastRowsRef[0]);
    lastRowsRef = nextRows;
    lastRowCount = nextRows.length;
    lastFirstKey = nextFirstKey;
    if (!isAppend) {
      requestedLength = -1;
      engaged = false;
    }
  },
);

// 4) DOM 更新后：同筛选刷新按锚点行重新对齐 scrollTop，把正在看的那一行留在原位置。
watch(
  () => [props.rows, props.resetKey] as const,
  async () => {
    const seq = ++reconcileSeq;
    const wasReset = resetTriggered;
    resetTriggered = false;
    const anchor = pendingAnchor;
    pendingAnchor = null;
    if (wasReset) {
      renderedKeys = keysOf(visibleRows.value);
      return;
    }
    const el = viewport.value;
    if (anchor && anchor.scrollTop > 0 && el && props.rowKey && anchor.candidates.length > 0) {
      const indexByKey = new Map<string, number>();
      for (let i = 0; i < props.rows.length; i++) {
        const key = props.rowKey(props.rows[i]);
        if (!indexByKey.has(key)) indexByKey.set(key, i);
      }
      let matched: { index: number; topOffset: number } | null = null;
      for (const cand of anchor.candidates) {
        const idx = indexByKey.get(cand.key);
        if (idx !== undefined) {
          matched = { index: idx, topOffset: cand.topOffset };
          break;
        }
      }
      if (matched) {
        if (matched.index >= limit.value) {
          limit.value = Math.min(props.rows.length, Math.max(limit.value, matched.index + 50));
          await nextTick();
          if (seq !== reconcileSeq) return;
        }
        const rowEls = queryRowElements(el);
        const visible = visibleRows.value;
        if (rowEls.length === visible.length && matched.index < rowEls.length) {
          const vpRect = el.getBoundingClientRect();
          const newTopOffset = rowEls[matched.index].getBoundingClientRect().top - vpRect.top;
          const delta = newTopOffset - matched.topOffset;
          if (Math.abs(delta) > 0.5) {
            restoringScroll = true;
            el.scrollTop = Math.max(0, el.scrollTop + delta);
            await nextTick();
            restoringScroll = false;
          }
        }
      }
    }
    if (seq === reconcileSeq) {
      renderedKeys = keysOf(visibleRows.value);
    }
  },
  { flush: "post" },
);

watch(
  visibleRows,
  (rows) => {
    renderedKeys = keysOf(rows);
  },
  { flush: "post" },
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
      {{ blocked || (loading ? t("dashboard.scrollLoading") : t("dashboard.scrollMore")) }}
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
