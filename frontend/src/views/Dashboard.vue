<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { NButton, NSelect, NSpin } from "naive-ui";
import {
  AGENT_LABEL,
  type AgentFilter,
  type Dim,
  type EventDrill,
  type EventList,
  type SourceStatus,
  type SummaryReport,
} from "../types";
import SummaryCards from "../components/SummaryCards.vue";
import UsageTable from "../components/UsageTable.vue";
import TrendChart from "../components/TrendChart.vue";
import EventTable from "../components/EventTable.vue";
import SegmentedControl from "../components/SegmentedControl.vue";
import DateRangeSelect from "../components/DateRangeSelect.vue";
import { TZ_OPTIONS, useTimezone } from "../composables/timezone";
import { todayInTz } from "../lib/dates";

const props = defineProps<{ refreshKey: number }>();
/// 任务 3：来源异常通知的「去设置 ›」动作——由应用壳切换到设置页。
const emit = defineEmits<{ (e: "go-settings"): void }>();

const by = ref<Dim>("day");
const agent = ref<AgentFilter>("all");
const range = ref<[string, string] | null>(null);
const { tz } = useTimezone();
// 设计系统 Task 2：标题行的日期/时区摘要（随 tz 响应式更新）。
const todayLabel = computed(() => todayInTz(tz.value));
const tzLabel = computed(() => TZ_OPTIONS.find((o) => o.value === tz.value)?.label ?? tz.value);
const report = ref<SummaryReport | null>(null);
const loading = ref(false);
const sourceStatus = ref<SourceStatus[]>([]);
const drill = ref<EventDrill | null>(null);
const events = ref<EventList | null>(null);
const eventsLoading = ref(false);
// D1 游标分页：「加载更多」状态
const moreLoading = ref(false);
const hasMore = computed(() => !!events.value && events.value.rows.length < events.value.total);
// F01（计划 A2）：汇总与明细各自持有请求代次——曾共用一个 runSeq，两个
// immediate watcher 依次启动使后者作废前者，首屏永远转圈或停在旧快照。
let summarySeq = 0;
let eventsSeq = 0;
// 失败必须可见并可重试（计划 ipc_error_visible），不再无声吞异常。
const summaryError = ref<string | null>(null);
const eventsError = ref<string | null>(null);
// C4（F08）：快照必须携带查询身份——修复前 report 与 events 来自各自
// 的请求，保存时可能混搭不同筛选的两代数据，恢复时筛选已重置而数据
// 还是旧口径。eventsKey 记录明细当前所属的筛选上下文，两者一致才落盘。
type SnapshotFilters = {
  by: Dim;
  agent: AgentFilter;
  /** v3：统计时区日历字符串 */
  range: [string, string] | null;
  drill: EventDrill | null;
  tz: string;
};
type SnapshotPayload = {
  v: number;
  saved_at: string;
  filters: SnapshotFilters;
  report: SummaryReport;
  events: EventList;
};
const eventsKey = ref<SnapshotFilters | null>(null);
// 启动提速（用户 2026-10-04）：先渲染上次视图快照，再后台刷新替换
const stale = ref(false);
const cachedAt = ref<string | null>(null);

const dimOptions: { label: string; value: Dim }[] = [
  { label: "按日", value: "day" },
  { label: "按模型", value: "model" },
  { label: "按项目", value: "project" },
  { label: "按应用", value: "agent" },
];

// Task 6：日期控件直接传统计时区日历字符串，无需毫秒换算。

const agentOptions: { label: string; value: AgentFilter; icon: string }[] = [
  { label: "全部", value: "all", icon: "all" },
  { label: "Claude Code", value: "claude", icon: "claude" },
  { label: "Codex", value: "codex", icon: "openai" },
];

// 任务 3：标题行状态胶囊——stale / 刷新中 / 刷新失败 / 已更新四态可见。
const statusPill = computed<{ cls: string; text: string }>(() => {
  if (stale.value) return { cls: "ts-pill-warning", text: "缓存数据 · 后台刷新中" };
  if (loading.value || !report.value) return { cls: "ts-pill-info", text: "刷新中…" };
  if (summaryError.value) return { cls: "ts-pill-warning", text: "刷新失败 · 显示上次数据" };
  return { cls: "ts-pill-success", text: "已更新" };
});

// 任务 7：来源异常通知——单条直接展示，多条合并为一条可展开通知。
const problemSources = computed(() => sourceStatus.value.filter((x) => x.state !== "ready"));
const showSourceDetails = ref(false);
function sourceNoticeText(s: SourceStatus): string {
  if (s.state === "disabled")
    return `${AGENT_LABEL[s.agent] ?? s.agent} 已在设置中停用，不参与统计。`;
  if (s.state === "missing")
    return `${AGENT_LABEL[s.agent] ?? s.agent} 数据目录不存在（${s.dir}）。`;
  return `${AGENT_LABEL[s.agent] ?? s.agent} 目录存在但没有发现会话日志（${s.dir}）。`;
}
async function refresh(): Promise<void> {
  const seq = ++summarySeq;
  loading.value = true;
  summaryError.value = null;
  try {
    const fresh = await invoke<SummaryReport>("summarize", {
      by: by.value,
      agent: agent.value,
      days: null,
      from: range.value?.[0] ?? null,
      to: range.value?.[1] ?? null,
      tz: tz.value,
    });
    if (seq !== summarySeq) return; // 已有更新的查询，丢弃旧响应
    report.value = fresh;
    stale.value = false;
    saveSnapshot();
  } catch (e) {
    if (seq === summarySeq) summaryError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (seq === summarySeq) loading.value = false;
  }
}

function currentFilters(): SnapshotFilters {
  return {
    by: by.value,
    agent: agent.value,
    range: range.value ? [...range.value] : null,
    drill: drill.value ? { ...drill.value } : null,
    tz: tz.value,
  };
}

function saveSnapshot(): void {
  if (!report.value || !events.value || !eventsKey.value) return;
  // 明细与汇总的筛选上下文不一致（例如刚切换 agent、明细还是旧的）：
  // 宁可保留上一份一致的快照，也不落盘混代数据。
  const k = currentFilters();
  const ek = eventsKey.value;
  const coherent =
    ek.agent === k.agent &&
    ek.tz === k.tz &&
    ek.range?.[0] === k.range?.[0] &&
    ek.range?.[1] === k.range?.[1];
  if (!coherent) return;
  const payload: SnapshotPayload = {
    v: 3,
    saved_at: new Date().toISOString(),
    filters: k,
    report: report.value,
    events: events.value,
  };
  void invoke("view_cache_save", { value: payload });
}

async function loadSources(): Promise<void> {
  sourceStatus.value = await invoke<SourceStatus[]>("source_status");
}

async function loadEvents(append = false): Promise<void> {
  const seq = ++eventsSeq;
  if (!append) eventsLoading.value = true;
  moreLoading.value = append;
  eventsError.value = null;
  try {
    // D1 游标：追加载取时以已加载末行为锚（ts|record_id 严格小于语义）。
    const last = append ? events.value?.rows.at(-1) : undefined;
    const list = await invoke<EventList>("list_events", {
      agent: agent.value,
      from: range.value?.[0] ?? null,
      to: range.value?.[1] ?? null,
      model: drill.value?.type === "model" ? drill.value.key : null,
      project: drill.value?.type === "project" ? drill.value.key : null,
      day: drill.value?.type === "day" ? drill.value.key : null,
      limit: 200,
      before: last ? last.cursor : null,
      tz: tz.value,
    });
    if (seq !== eventsSeq) return;
    if (append && events.value) {
      events.value = { ...list, rows: [...events.value.rows, ...list.rows] };
    } else {
      events.value = list;
      eventsKey.value = currentFilters();
    }
    saveSnapshot();
  } catch (e) {
    if (seq === eventsSeq) eventsError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (seq === eventsSeq) {
      eventsLoading.value = false;
      moreLoading.value = false;
    }
  }
}

function loadMoreEvents(): void {
  void loadEvents(true);
}

async function loadViewCache(): Promise<void> {
  try {
    const cached = await invoke<SnapshotPayload | null>("view_cache_load");
    // 只接受 v3 快照（range 为日历字符串）；旧格式/损坏一律走正常加载。
    if (!cached || cached.v !== 3 || !cached.report || !cached.filters) return;
    // 连同筛选一起恢复：数据与筛选必然同口径（保存时已做过一致性检查）。
    by.value = cached.filters.by;
    agent.value = cached.filters.agent;
    range.value = cached.filters.range ? [...cached.filters.range] : null;
    drill.value = cached.filters.drill ? { ...cached.filters.drill } : null;
    tz.value = cached.filters.tz;
    eventsKey.value = cached.filters;
    report.value = cached.report;
    events.value = cached.events;
    cachedAt.value = cached.saved_at;
    stale.value = true;
  } catch {
    // 视图缓存损坏：静默忽略，走正常加载
  }
}
void loadViewCache();

function onSummaryRowClick(key: string): void {
  // 合计行不是真实维度值，不得生成字面“合计”过滤（计划 total_row_clears_drill）。
  if (key === "合计") {
    if (by.value === "agent") agent.value = "all";
    drill.value = null;
    return;
  }
  if (by.value === "agent") {
    // Agent 行：切换顶部 agent 过滤，不进入明细下钻
    agent.value = key === "claude-code" ? "claude" : key === "codex" ? "codex" : "all";
    drill.value = null;
    return;
  }
  drill.value = { type: by.value as "day" | "model" | "project", key };
}

function clearDrill(): void {
  drill.value = null;
}

// C4：手动刷新——以当前筛选重跑两条查询，筛选状态不动。
function manualRefresh(): void {
  void refresh();
  void loadEvents();
}

const drillLabel = (d: EventDrill): string => `${d.type}: ${d.key}`;

watch([by, agent, range, tz, () => props.refreshKey], refresh, { immediate: true });
watch([by, agent, range, tz, drill, () => props.refreshKey], () => void loadEvents(), {
  immediate: true,
});
void loadSources();
</script>

<template>
  <div>
    <!-- 页面标题行：先回答"覆盖什么时间"，右侧状态胶囊 + 刷新动作 -->
    <div class="page-head">
      <div class="head-left">
        <h1 class="page-title">用量汇总</h1>
        <div class="page-sub">
          统计时区 {{ tzLabel }} · 今天 {{ todayLabel
          }}<template v-if="drill"> · 已筛选 {{ drillLabel(drill) }}</template>
        </div>
      </div>
      <div class="head-right">
        <span class="ts-pill ts-status-pill" :class="statusPill.cls" role="status">{{
          statusPill.text
        }}</span>
        <NButton size="small" secondary class="ts-focusable" @click="manualRefresh">刷新</NButton>
      </div>
    </div>
    <!-- 筛选行：来源 → 维度 → 日期 → 时区 → 刷新（窄窗口自动换行） -->
    <div class="filter-row">
      <SegmentedControl v-model="agent" :options="agentOptions" aria-label="数据来源" />
      <SegmentedControl v-model="by" :options="dimOptions" aria-label="聚合维度" />
      <DateRangeSelect v-model:value="range" :tz="tz" />
      <NSelect
        :value="tz"
        :options="TZ_OPTIONS"
        size="small"
        class="tz-select"
        aria-label="统计时区"
        @update:value="(v: string) => (tz = v)"
      />
    </div>
    <!-- 任务 3/7：异常与来源四态 = 内联通知条；多条来源异常合并为一条可展开 -->
    <template v-if="problemSources.length === 1">
      <div class="ts-notice source-notice">
        <svg
          class="ts-notice-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M12 3.5 21 19.5H3z" />
          <path d="M12 10v4" />
          <path d="M12 17h.01" />
        </svg>
        <span class="ts-notice-content">{{ sourceNoticeText(problemSources[0]) }}</span>
        <button
          v-if="problemSources[0].state !== 'disabled'"
          type="button"
          class="ts-notice-action ts-focusable"
          @click="emit('go-settings')"
        >
          去设置 ›
        </button>
      </div>
    </template>
    <template v-else-if="problemSources.length > 1">
      <div class="ts-notice source-notice">
        <svg
          class="ts-notice-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M12 3.5 21 19.5H3z" />
          <path d="M12 10v4" />
          <path d="M12 17h.01" />
        </svg>
        <span class="ts-notice-content">
          {{ problemSources.length }} 个来源异常，统计可能不完整。
        </span>
        <button
          type="button"
          class="ts-notice-action ts-focusable"
          :aria-expanded="showSourceDetails"
          @click="showSourceDetails = !showSourceDetails"
        >
          详情
        </button>
      </div>
      <div v-if="showSourceDetails" class="source-details">
        <div v-for="s in problemSources" :key="s.agent" class="source-detail-line">
          {{ sourceNoticeText(s) }}
        </div>
      </div>
    </template>
    <!-- 失败可见并可重试（计划 A2）：保留已有数据展示，不整体灰罩 -->
    <div v-if="summaryError" class="ts-notice source-notice">
      <svg
        class="ts-notice-icon is-error"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="9" />
        <path d="m9 9 6 6M15 9l-6 6" />
      </svg>
      <span class="ts-notice-content">汇总加载失败：{{ summaryError }}</span>
      <button type="button" class="ts-notice-action ts-focusable" @click="refresh">重试</button>
    </div>
    <div v-if="eventsError" class="ts-notice source-notice">
      <svg
        class="ts-notice-icon is-error"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="9" />
        <path d="m9 9 6 6M15 9l-6 6" />
      </svg>
      <span class="ts-notice-content">明细加载失败：{{ eventsError }}</span>
      <button type="button" class="ts-notice-action ts-focusable" @click="loadEvents()">
        重试
      </button>
    </div>
    <!-- 有数据时不再全屏灰罩：数据原地更新，标题行胶囊提示刷新中 -->
    <NSpin :show="loading && !report">
      <!-- 最小高度保证加载转圈居中于可视区，避免空内容时贴顶被遮挡 -->
      <div style="min-height: 380px">
        <!-- 设计系统 Task 7：空数据状态明确可见，并给出下一步指引 -->
        <div v-if="report && report.groups.length === 0" class="empty-state" role="status">
          <div class="empty-title">暂无数据</div>
          <div class="empty-hint">
            调整时间范围或来源后重试；若刚配置来源，先在设置页确认目录正确。
          </div>
        </div>
        <template v-else-if="report">
          <SummaryCards :totals="report.totals" />
          <TrendChart
            v-if="report.by === 'day' || report.groups.length > 2"
            :groups="report.groups.filter((g) => g.key !== '合计')"
            :by="report.by"
          />
          <UsageTable :report="report" @row-click="onSummaryRowClick" />
          <EventTable
            v-if="events"
            :list="events"
            :filter-label="drill ? drillLabel(drill) : '无（显示最新 200 条）'"
            :filter-closable="!!drill"
            :more="hasMore"
            :more-loading="moreLoading"
            @load-more="loadMoreEvents"
            @clear-filter="clearDrill"
          />
        </template>
      </div>
    </NSpin>
  </div>
</template>

<style scoped>
.empty-state {
  border: 1px dashed var(--ts-separator-strong);
  border-radius: var(--ts-radius-card);
  background: var(--ts-surface-solid);
  padding: var(--ts-space-8) var(--ts-space-6);
  text-align: center;
}

.empty-title {
  font-size: 15px;
  font-weight: 650;
  color: var(--ts-text);
}

.empty-hint {
  font-size: 12px;
  color: var(--ts-text-muted);
  margin-top: var(--ts-space-2);
}

.page-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--ts-space-4);
  margin-bottom: var(--ts-space-4);
}

.head-right {
  display: flex;
  align-items: center;
  gap: var(--ts-space-2);
  flex-shrink: 0;
}

/* DESIGN.md §3：页面大标题 28px/700，字距 -0.02em */
.page-title {
  font-family: var(--ts-font-display);
  font-size: 28px;
  font-weight: 700;
  line-height: 1.2;
  letter-spacing: -0.02em;
  margin: 0;
  color: var(--ts-text);
}

.page-sub {
  font-size: 12px;
  line-height: 1.4;
  color: var(--ts-text-muted);
  margin-top: var(--ts-space-1);
}

.source-notice {
  margin-bottom: var(--ts-space-3);
}

/* 合并通知展开后的逐条明细 */
.source-details {
  margin: calc(-1 * var(--ts-space-2)) 0 var(--ts-space-3);
  padding: var(--ts-space-2) var(--ts-space-3);
  border-left: 2px solid var(--ts-separator-strong);
  border-radius: var(--ts-radius-control);
  background: var(--ts-surface-solid);
}

.source-detail-line {
  font-size: 12px;
  line-height: 1.8;
  color: var(--ts-text-secondary);
}

.source-notice .ts-notice-icon {
  color: var(--ts-warning);
}

.source-notice .ts-notice-icon.is-error {
  color: var(--ts-error);
}

.source-notice .ts-notice-action {
  border: none;
  background: transparent;
  font: inherit;
  font-size: 13px;
  padding: 0;
}

.filter-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--ts-space-3);
  margin-bottom: var(--ts-space-3);
}

.tz-select {
  width: 160px;
}

/* 任务 5：卡片区块间距统一 20px（卡片间 20px，DESIGN.md §4） */
:deep(section.ts-card) {
  margin-bottom: var(--ts-space-5);
}
:deep(section.ts-card:last-child) {
  margin-bottom: 0;
}
</style>
