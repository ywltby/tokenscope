<script setup lang="ts">
// U04（UI 调整计划 2026-10-13）：汇总页的状态与查询编排已集中到
// `composables/useUsageQuery.ts`——本组件只保留模板渲染、UI 局部状态
// （合并通知的展开态）与可见性转达。
//
// **顶层绑定名保持与重构前一致**（report / events / loading / …）：既有
// `Dashboard.test.ts` 约 45 条断言按 `state(w)["report"]` 的方式读取它们，
// 因此这里用解构 + 同名暴露（`<script setup>` 的顶层绑定会挂到 vm 上）。
import { computed, onActivated, onDeactivated, ref } from "vue";
import { NButton, NSpin } from "naive-ui";
import type { AgentFilter, Dim } from "../types";
import SummaryCards from "../components/SummaryCards.vue";
import UsageTable from "../components/UsageTable.vue";
import TrendChart from "../components/TrendChart.vue";
import EventTable from "../components/EventTable.vue";
import SegmentedControl from "../components/SegmentedControl.vue";
import DateRangeSelect from "../components/DateRangeSelect.vue";
import { useUsageQuery, type NoticeAction, type NoticeItem } from "../composables/useUsageQuery";
import { useCollectionStatus, type CollectionPillView } from "../composables/collectionStatus";
import { useTimezone } from "../composables/timezone";
import { realGroups } from "../lib/chartData";

const props = defineProps<{ refreshKey: number }>();
/// 任务 3：来源异常通知的「去设置 ›」动作——由应用壳切换到设置页。
const emit = defineEmits<{ (e: "go-settings"): void }>();

/// PR2：读取侧状态桥——collectionStatus 写入这两个 ref，useUsageQuery 在
/// 状态胶囊与合并通知里消费（先建查询组合式，再把它的可见性与刷新入口
/// 交给读取侧组合式，避免循环依赖）。
const collectionPill = ref<CollectionPillView | null>(null);
const collectionNotice = ref<NoticeItem | null>(null);

const {
  // 筛选偏好（来源 / 维度 / 时间预设 / 日粒度参与方式 / 下钻）
  by,
  agent,
  preset,
  drill,
  rollupsMode,
  batchRange,
  // 数据与状态
  report,
  events,
  loading,
  eventsLoading,
  moreLoading,
  hasMore,
  stale,
  cachedAt,
  summaryError,
  eventsError,
  sessionReady,
  liveFirstPage,
  currentQuery,
  // 来源状态
  sourceStatus,
  sourceError,
  sourcesLoading,
  problemSources,
  // 派生
  statusPill,
  collectionDiagnostics,
  instantRangeHint,
  notices,
  noticeHeadline,
  moreBlockedHint,
  drillLabel,
  // 动作
  refresh,
  manualRefresh,
  startRefreshBatch,
  retryAfterFailure,
  loadEvents,
  loadMoreEvents,
  setRollupsMode,
  onSummaryRowClick,
  onTotalRowClick,
  clearDrill,
  loadSources,
  saveSnapshot,
  // U06 接入点：可见性、待刷新标记、自动刷新入口与让位提示
  visible,
  pendingRefresh,
  latestGeneration,
  autoRefreshHint,
  setVisible,
  requestAutoRefresh,
  notifyUsageUpdated,
} = useUsageQuery({
  refreshKey: () => props.refreshKey,
  goSettings: () => emit("go-settings"),
  collectionPill,
  collectionNotice,
});

/// PR2：真实采集状态 + 最短 1 秒显示期。更高的 latestGeneration 经
/// notifyUsageUpdated（既有的"有新数据"自动刷新合并入口）转发；综合可见性
/// 直接复用查询侧的 visible（页面 KeepAlive + 宿主窗口两个维度都已合并）。
const collectionApi = useCollectionStatus({
  visible,
  onGeneration: notifyUsageUpdated,
  onManualRefresh: manualRefresh,
  pillOut: collectionPill,
  noticeOut: collectionNotice,
});

/// PR2：读取侧展示态（原生 E2E 以低频轮询取证，不用截图间隔推断毫秒）。
const collectionState = computed(() => {
  if (collectionApi.statusUnavailable.value) return "unavailable";
  if (!collectionApi.latestStatus.value) return "confirming";
  if (collectionApi.reading.value) return "reading";
  if (collectionApi.latestStatus.value.phase === "failed") return "failed";
  return "confirmed";
});
const collectionRevision = computed(() =>
  collectionApi.latestStatus.value ? String(collectionApi.latestStatus.value.revision) : "",
);
const collectionRunId = computed(() =>
  collectionApi.latestStatus.value ? String(collectionApi.latestStatus.value.runId) : "",
);
const collectionGeneration = computed(() => {
  const gen = collectionApi.latestStatus.value?.latestGeneration;
  return typeof gen === "number" ? String(gen) : "";
});

/// U01：日期按钮的悬停提示与可访问名称按统计时区渲染（时区本身在设置页改）。
const { tz } = useTimezone();

/// U04：合并通知的展开态。`showSourceDetails` 是历史绑定名（既有回归测试按
/// 它读取/写入展开态），与 `showNoticeDetails` 指向同一状态。
const showNoticeDetails = ref(false);
const showSourceDetails = showNoticeDetails;

type NoticeRow = { key: string; text: string; actions: NoticeAction[] };

/// 展开后的逐条内容：多条通知时每条一行（带自己的动作），单条时列出它的细节。
const noticeDetailRows = computed<NoticeRow[]>(() => {
  if (!showNoticeDetails.value) return [];
  const rows: NoticeRow[] = [];
  for (const n of notices.value) {
    rows.push({ key: n.id, text: n.text, actions: n.actions });
    (n.details ?? []).forEach((d, i) => rows.push({ key: `${n.id}-d${i}`, text: d, actions: [] }));
  }
  return rows;
});

const dimOptions: { label: string; value: Dim }[] = [
  { label: "按时间", value: "day" },
  { label: "按模型", value: "model" },
  { label: "按项目", value: "project" },
  { label: "按应用", value: "agent" },
];

const agentOptions: { label: string; value: AgentFilter; icon: string }[] = [
  { label: "全部", value: "all", icon: "all" },
  { label: "Claude Code", value: "claude", icon: "claude" },
  { label: "Codex", value: "codex", icon: "openai" },
];

// UX05：真实可绘制类别（排除"合计"行）——用 computed 缓存，避免在模板里
// 每次渲染都 filter 生成新数组；新数组会改变 TrendChart 的 groups 身份，
// 触发图表不必要的 dispose/init。
const chartGroups = computed(() => realGroups(report.value?.groups ?? []));

// U04：切页保留——汇总页由 KeepAlive 保留，隐藏期间 watcher 仍会运行，
// 必须显式把可见性交给查询状态机（隐藏时只置"待刷新"标记）。
// 选择 onActivated/onDeactivated 的理由见 useUsageQuery.setVisible 的注释。
onActivated(() => setVisible(true));
onDeactivated(() => setVisible(false));

// U04 契约：这些绑定名与重构前保持一致（既有 `Dashboard.test.ts` 按
// `state(w)["report"]` / `state(w)["stale"]` 之类的写法读取；U06 的
// `pendingRefresh` / `autoRefreshHint` / `requestAutoRefresh` /
// `notifyUsageUpdated` 同样经这里暴露，供模板 ref 程序化调用）。
// 模板不直接使用其中一部分，这里显式聚合暴露一次：既避免被 noUnusedLocals
// 判成死绑定，也给父组件（模板 ref）一个明确的程序化入口。
defineExpose({
  by,
  agent,
  preset,
  drill,
  rollupsMode,
  batchRange,
  report,
  events,
  loading,
  eventsLoading,
  moreLoading,
  hasMore,
  stale,
  cachedAt,
  summaryError,
  eventsError,
  sessionReady,
  liveFirstPage,
  currentQuery,
  sourceStatus,
  sourceError,
  sourcesLoading,
  problemSources,
  statusPill,
  collectionDiagnostics,
  instantRangeHint,
  notices,
  noticeHeadline,
  moreBlockedHint,
  drillLabel,
  refresh,
  manualRefresh,
  startRefreshBatch,
  retryAfterFailure,
  loadEvents,
  loadMoreEvents,
  setRollupsMode,
  onSummaryRowClick,
  onTotalRowClick,
  clearDrill,
  loadSources,
  saveSnapshot,
  visible,
  pendingRefresh,
  latestGeneration,
  autoRefreshHint,
  setVisible,
  requestAutoRefresh,
  notifyUsageUpdated,
});
</script>

<template>
  <div
    :data-summary-query="report?.query_id"
    :data-events-query="events?.query_id"
    :data-requests="report?.totals.requests"
  >
    <!-- 页面标题行：先回答"覆盖什么时间"，右侧状态胶囊 + 刷新动作 -->
    <div class="page-head">
      <div class="head-left">
        <h1 class="page-title">用量汇总</h1>
        <div v-if="drill" class="page-sub">已筛选 {{ drillLabel(drill) }}</div>
      </div>
      <div class="head-right">
        <!-- U06/PR2：正常态显示静态"已更新"（自带状态圆点，无常驻更新时间，
             悬停提示写自动读取间隔与"无新用量时保持统计不变"）；读取侧状态由
             collectionStatus 驱动——正在读取（旋转图标，最短 1 秒）、读取失败、
             状态不可用。data-collection-* 供原生 E2E 低频轮询取证。 -->
        <span
          class="ts-pill ts-status-pill"
          :class="[statusPill.cls, { 'is-reading': statusPill.spin }]"
          :title="statusPill.title"
          role="status"
          :data-collection-state="collectionState"
          :data-collection-revision="collectionRevision"
          :data-collection-run-id="collectionRunId"
          :data-collection-generation="collectionGeneration"
          ><svg
            v-if="statusPill.spin"
            class="ts-pill-spinner"
            viewBox="0 0 16 16"
            width="14"
            height="14"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            stroke-linecap="round"
            aria-hidden="true"
          >
            <path d="M8 2.2a5.8 5.8 0 1 1-5.8 5.8" />
          </svg>
          <span class="ts-pill-text">{{ statusPill.text }}</span></span
        >
        <!-- U06 翻页保护：已加载更多时自动刷新不执行，改为提示；点击等同手动刷新 -->
        <button
          v-if="autoRefreshHint"
          type="button"
          class="ts-notice-action ts-focusable ts-refresh-hint"
          @click="manualRefresh"
        >
          {{ autoRefreshHint }}
        </button>
        <NButton
          size="small"
          :loading="loading || eventsLoading"
          :disabled="loading || eventsLoading"
          @click="manualRefresh"
        >
          <template #icon>
            <!-- 装饰性刷新图标：可访问名称由按钮文字「刷新」承载 -->
            <svg
              viewBox="0 0 24 24"
              width="14"
              height="14"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path d="M20 12a8 8 0 1 1-2.4-5.7" />
              <path d="M20 4.2V9h-4.8" />
            </svg>
          </template>
          刷新
        </NButton>
      </div>
    </div>
    <!-- 固定两行：来源；日期 / 聚合维度。时区在设置页调整。 -->
    <div class="filter-row filter-controls">
      <div class="filter-line">
        <SegmentedControl v-model="agent" :options="agentOptions" aria-label="数据来源" icon-only />
      </div>
      <div class="filter-line">
        <DateRangeSelect v-model:preset="preset" :tz="tz" :range="batchRange" />
        <SegmentedControl v-model="by" :options="dimOptions" aria-label="聚合维度" />
      </div>
    </div>
    <!-- U04：合并通知——页面上只显示一条（原来最多七种通知条并存），展示最高
         严重级别与条数；展开后逐条列出，动作保留在各自条目上。
         条目来源：来源检测失败（重试）、来源异常（去设置）、汇总/明细失败（重试）、
         采集诊断（详情）、时刻范围提示、时区不匹配（只看请求明细）、
         仅明细模式（恢复自动参与）。时区的取值见 useUsageQuery。 -->
    <div
      v-if="noticeHeadline"
      class="ts-notice source-notice notice-merged"
      :data-expanded="showSourceDetails ? 'true' : 'false'"
      :role="noticeHeadline.severity === 'error' ? 'alert' : 'status'"
    >
      <svg
        v-if="noticeHeadline.severity === 'error'"
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
      <svg
        v-else
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
      <span class="ts-notice-content">{{ noticeHeadline.text }}</span>
      <button
        v-if="noticeHeadline.expandable"
        type="button"
        class="ts-notice-action ts-focusable"
        :aria-expanded="showNoticeDetails"
        @click="showNoticeDetails = !showNoticeDetails"
      >
        {{ showNoticeDetails ? "收起" : "详情" }}
      </button>
      <button
        v-for="a in noticeHeadline.actions"
        :key="a.label"
        type="button"
        class="ts-notice-action ts-focusable"
        :disabled="a.disabled"
        @click="a.run()"
      >
        {{ a.label }}
      </button>
    </div>
    <div v-if="noticeDetailRows.length > 0" class="source-details">
      <div v-for="row in noticeDetailRows" :key="row.key" class="source-detail-line">
        <span class="source-detail-text">{{ row.text }}</span>
        <button
          v-for="a in row.actions"
          :key="a.label"
          type="button"
          class="ts-notice-action ts-focusable"
          :disabled="a.disabled"
          @click="a.run()"
        >
          {{ a.label }}
        </button>
      </div>
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
          <!-- UX05：以真实类别数判断——≥1 绘制图表；0 给明确空状态，不画无内容图 -->
          <TrendChart v-if="chartGroups.length >= 1" :groups="chartGroups" :by="report.by" />
          <div v-else class="chart-empty" role="status">
            <span class="chart-empty-text">暂无可绘制的类别（仅有合计行）。</span>
          </div>
          <UsageTable
            :report="report"
            @row-click="onSummaryRowClick"
            @total-row="onTotalRowClick"
          />
          <EventTable
            v-if="events"
            :list="events"
            :filter-label="drill ? drillLabel(drill) : ''"
            :filter-closable="!!drill"
            :more="hasMore"
            :more-loading="moreLoading"
            :more-blocked-hint="moreBlockedHint"
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

/* UX08：空状态标题对齐既有字阶——卡片级结论 17px/600/1.3/-0.01em
   （修复前 15px/650 既不在字阶内，也用非标字重） */
.empty-title {
  font-size: 17px;
  font-weight: 600;
  line-height: 1.3;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.empty-hint {
  font-size: 12px;
  color: var(--ts-text-muted);
  margin-top: var(--ts-space-2);
}

/* UX05：无可绘制类别时的明确空状态（不渲染无内容图表） */
.chart-empty {
  border: 1px dashed var(--ts-separator-strong);
  border-radius: var(--ts-radius-card);
  background: var(--ts-surface-solid);
  padding: var(--ts-space-6);
  text-align: center;
  margin-bottom: var(--ts-space-5);
}

.chart-empty-text {
  font-size: 12px;
  color: var(--ts-text-muted);
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

/* 合并通知：一条通知条 + 展开后的逐条明细 */
.notice-merged {
  flex-wrap: wrap;
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
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: var(--ts-space-2);
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

/* U06：翻页保护下的自动刷新提示按钮（「有新数据 · 刷新」/「时间范围已更新 · 刷新」）
   与通知条动作同族：强调色文字按钮，材质由 .ts-notice-action 承载。 */
.ts-refresh-hint {
  border: none;
  background: transparent;
  font: inherit;
  font-size: 13px;
  padding: 0;
  white-space: nowrap;
}

.filter-row {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--ts-space-3);
  margin-bottom: var(--ts-space-3);
}

.filter-line {
  display: flex;
  align-items: center;
  gap: var(--ts-space-3);
}

/* 任务 5：卡片区块间距统一 20px（卡片间 20px，DESIGN.md §4） */
:deep(section.ts-card) {
  margin-bottom: var(--ts-space-5);
}
:deep(section.ts-card:last-child) {
  margin-bottom: 0;
}
</style>
