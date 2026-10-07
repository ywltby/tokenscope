<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
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
import { SNAPSHOT_VERSION, enqueueSnapshotSave } from "../lib/viewSnapshot";
import { onUnmounted } from "vue";
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
// F03：共享刷新批次——主筛选/refreshKey/手动刷新同步生成单一 epoch，两条
// 查询携带同一 epoch；请求代次只负责丢弃旧响应，保存门槛用 epoch 判定
// 「两侧均成功且属于当前批次」，杜绝新汇总 + 旧明细的同筛选拼接。
let refreshEpoch = 0;
let summaryBatchEpoch = -1;
let eventsBatchEpoch = -1;
// F03：用户操作版本——启动缓存读取前捕获；任何筛选/下钻/手动刷新立即
// 递增并撤销晚到缓存的恢复资格（即使新查询尚未返回）。
let interactionEpoch = 0;
// F03：恢复应用期间 watcher 跳过查询（恢复后恰好启动一批刷新，不重复触发）。
let applyingRestore = false;
// R04（Task 4）：实例 disposal——App 用 v-if 切页，卸载后晚到的响应
// 不得更新状态或落盘快照（新实例的查询/保存才是权威）。
let disposed = false;
// R04：freshArrived 守卫——任何新查询结果落地后，晚到的视图缓存不再
// 接管（并发初始化 + 守卫；缓存恢复改筛选触发的后台刷新由 watcher
// 自然完成）。
let freshArrived = false;
// 失败必须可见并可重试（计划 ipc_error_visible），不再无声吞异常。
const summaryError = ref<string | null>(null);
const eventsError = ref<string | null>(null);
// C4（F08）：快照必须携带查询身份——修复前 report 与 events 来自各自
// 的请求，保存时可能混搭不同筛选的两代数据，恢复时筛选已重置而数据
// 还是旧口径。eventsKey 记录明细当前所属的筛选上下文，两者一致才落盘。
type SnapshotFilters = {
  by: Dim;
  agent: AgentFilter;
  /** v4：统计时区日历字符串 */
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
// SF04：查询会话句柄（query_begin 返回）——同一批次的汇总/明细/分页
// 都绑定同一 query_id；价格修订随会话冻结。
type QueryHandle = {
  queryId: string;
  generation: number;
  pricingRevision: string;
  timezone: string;
  asOf: string;
};
const currentQuery = ref<QueryHandle | null>(null);
// SF05/SF04：每批次共享一次 begin_query Promise，汇总/明细并发读取同一
// 会话；跨午夜/手动刷新的批次重建会话。
let beginPromise: Promise<QueryHandle | null> | null = null;
let beginEpoch = -1;
// R04：汇总/明细各自的**请求时捕获键**（不可变）——响应返回时不再读
// currentFilters() 冒充请求身份（混代数据保存的根因）。
let summaryKey: SnapshotFilters | null = null;
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
  // R04 状态机：stale + 失败 = 旧数据/刷新失败（不能谎称"后台刷新中"）；
  // stale + 刷新中才显示后台刷新。
  if (stale.value && summaryError.value)
    return { cls: "ts-pill-warning", text: "缓存数据 · 刷新失败" };
  if (stale.value && loading.value)
    return { cls: "ts-pill-warning", text: "缓存数据 · 后台刷新中" };
  if (loading.value || !report.value) return { cls: "ts-pill-info", text: "刷新中…" };
  if (summaryError.value) return { cls: "ts-pill-warning", text: "刷新失败 · 显示上次数据" };
  return { cls: "ts-pill-success", text: "已更新" };
});

// R08（Task 8）：采集诊断——空结果/部分结果都必须可见，不依赖表格挂载。
// 分开命名：warnings（重叠/缓存降级等）、坏行、IO 错误——不声称所有
// warning 都表示丢数据；干净空区间不误报。
const collectionDiagnostics = computed(() => {
  const warns = report.value?.warnings ?? [];
  const statsList = report.value?.sources.map((s) => s.stats) ?? [];
  const ioErrors = statsList.reduce((a, st) => a + (st.io_errors ?? 0), 0);
  const badLines = statsList.reduce((a, st) => a + (st.bad_lines ?? 0), 0);
  const clean = warns.length === 0 && ioErrors === 0 && badLines === 0;
  const summaryText = clean
    ? null
    : [
        warns.length > 0 ? `${warns.length} 条采集警告` : null,
        ioErrors > 0 ? `${ioErrors} 个文件读取失败` : null,
        badLines > 0 ? `${badLines} 行解析失败（已跳过）` : null,
      ]
        .filter(Boolean)
        .join("，");
  return { clean, summaryText, warns, ioErrors, badLines };
});
const showCollectionDetails = ref(false);

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
// SF04：创建（或复用）当前批次的查询会话。同一 epoch 只发起一次
// begin_query，refresh/loadEvents 共享该 Promise；晚到的旧 begin 不得
// 接管新批次（epoch/queryId 双重守卫）。
function beginQueryForCurrentEpoch(): Promise<QueryHandle | null> {
  const epoch = refreshEpoch;
  if (beginPromise && beginEpoch === epoch) return beginPromise;
  beginEpoch = epoch;
  const captured = {
    by: by.value,
    agent: agent.value,
    from: range.value?.[0] ?? null,
    to: range.value?.[1] ?? null,
    tz: tz.value,
  };
  beginPromise = (async () => {
    try {
      const h = await invoke<QueryHandle>("query_begin", {
        by: captured.by,
        agent: captured.agent,
        days: null,
        from: captured.from,
        to: captured.to,
        tz: captured.tz,
      });
      if (epoch !== refreshEpoch || disposed) return null; // 旧 begin 晚到不接管
      currentQuery.value = h;
      return h;
    } catch (e) {
      // 会话建立失败：保留旧视图（不伪装已更新），错误在汇总侧可见。
      if (epoch === refreshEpoch && !disposed)
        summaryError.value = e instanceof Error ? e.message : String(e);
      return null;
    }
  })();
  return beginPromise;
}

async function refresh(): Promise<void> {
  const seq = ++summarySeq;
  const epoch = refreshEpoch;
  // R04：请求发起时捕获不可变查询身份（批次 = epoch）。
  const captured: SnapshotFilters = {
    by: by.value,
    agent: agent.value,
    range: range.value ? [...range.value] : null,
    drill: null,
    tz: tz.value,
  };
  loading.value = true;
  summaryError.value = null;
  try {
    const h = await beginQueryForCurrentEpoch();
    if (seq !== summarySeq || disposed || epoch !== refreshEpoch) return;
    if (!h) return; // begin 失败已写入 summaryError；保留旧视图
    const fresh = await invoke<SummaryReport>("query_summary", { queryId: h.queryId });
    if (seq !== summarySeq || disposed || epoch !== refreshEpoch) return;
    if (h.queryId !== currentQuery.value?.queryId) return; // 会话已被新批次替换
    report.value = fresh;
    stale.value = false;
    freshArrived = true;
    summaryBatchEpoch = epoch;
    summaryKey = captured;
    saveSnapshot();
  } catch (e) {
    if (seq === summarySeq && !disposed)
      summaryError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (seq === summarySeq && !disposed) loading.value = false;
  }
}

function sameMainIdentity(a: SnapshotFilters, b: SnapshotFilters): boolean {
  return (
    a.by === b.by &&
    a.agent === b.agent &&
    a.tz === b.tz &&
    a.range?.[0] === b.range?.[0] &&
    a.range?.[1] === b.range?.[1]
  );
}

function saveSnapshot(): void {
  // F03：保存门槛 = 汇总与明细均成功且属于**当前批次**（epoch 一致），
  // 主键完全一致（含 by），且 report.by 与键一致。任一侧失败/旧批次 →
  // 保留上一份一致快照，不由另一侧成功写新旧拼接；分页追加只延长同
  // 批次明细，drill 是明细侧子查询身份（汇总无 drill 概念）。
  if (!report.value || !events.value || !eventsKey.value || !summaryKey) return;
  if (summaryBatchEpoch !== refreshEpoch || eventsBatchEpoch !== refreshEpoch) return;
  const sk = summaryKey;
  const ek = eventsKey.value;
  if (!sameMainIdentity(sk, ek)) return;
  if (report.value.by !== sk.by) return;
  // SF04：两侧必须来自同一后端会话（query_id/价格修订一致）——
  // 跨会话拼接的 report/events 不落盘。
  if (report.value.query_id !== events.value.query_id) return;
  if (report.value.pricing_revision !== events.value.pricing_revision) return;
  const payload: SnapshotPayload = {
    v: SNAPSHOT_VERSION,
    saved_at: new Date().toISOString(),
    filters: ek,
    report: report.value,
    events: events.value,
  };
  enqueueSnapshotSave(payload);
}

async function loadSources(): Promise<void> {
  sourceStatus.value = await invoke<SourceStatus[]>("source_status");
}

async function loadEvents(append = false): Promise<void> {
  if (append) {
    // F03：追加页仅允许在所属明细首批完成后执行，且锚点必须与当前显示
    // 的明细同主身份（by/agent/range/tz/drill）——旧筛选的游标不得用于
    // 新筛选，新筛选首批也不得被追加响应作废。
    if (eventsLoading.value || !eventsKey.value) return;
    const current: SnapshotFilters = {
      by: by.value,
      agent: agent.value,
      range: range.value ? [...range.value] : null,
      drill: drill.value ? { ...drill.value } : null,
      tz: tz.value,
    };
    if (!sameMainIdentity(eventsKey.value, current)) return;
    if ((eventsKey.value.drill?.type ?? null) !== (current.drill?.type ?? null)) return;
    if ((eventsKey.value.drill?.key ?? null) !== (current.drill?.key ?? null)) return;
  }
  const seq = ++eventsSeq;
  const epoch = refreshEpoch;
  // R04：分页追加只作用于所属明细查询——捕获含游标锚的完整身份。
  const captured: SnapshotFilters = {
    by: by.value,
    agent: agent.value,
    range: range.value ? [...range.value] : null,
    drill: drill.value ? { ...drill.value } : null,
    tz: tz.value,
  };
  const anchor = append ? (events.value?.rows.at(-1)?.cursor ?? null) : null;
  if (!append) eventsLoading.value = true;
  moreLoading.value = append;
  eventsError.value = null;
  try {
    // SF04：共享本批次的会话；下钻/翻页在同一 query_id 上执行。
    const h = await beginQueryForCurrentEpoch();
    if (seq !== eventsSeq || disposed || epoch !== refreshEpoch) return;
    if (!h) {
      // 会话建立失败：保留旧明细（不伪装已更新），错误在汇总侧可见。
      eventsError.value = summaryError.value;
      return;
    }
    const list = await invoke<EventList>("query_events", {
      queryId: h.queryId,
      model: captured.drill?.type === "model" ? captured.drill.key : null,
      project: captured.drill?.type === "project" ? captured.drill.key : null,
      day: captured.drill?.type === "day" ? captured.drill.key : null,
      limit: 200,
      before: anchor,
    });
    if (seq !== eventsSeq || disposed) return;
    if (h.queryId !== currentQuery.value?.queryId) return; // 旧会话响应不落地
    if (append && events.value) {
      events.value = { ...list, rows: [...events.value.rows, ...list.rows] };
    } else {
      events.value = list;
      eventsKey.value = captured;
      eventsBatchEpoch = epoch;
    }
    freshArrived = true;
    saveSnapshot();
  } catch (e) {
    if (seq === eventsSeq && !disposed)
      eventsError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (seq === eventsSeq && !disposed) {
      eventsLoading.value = false;
      moreLoading.value = false;
    }
  }
}

function loadMoreEvents(): void {
  void loadEvents(true);
}

async function loadViewCache(): Promise<void> {
  // F03：恢复所有权——读取前捕获用户操作版本；任何筛选/下钻/手动刷新
  // 都会在等待期间递增 interactionEpoch，晚到缓存随即失去接管资格。
  const epochAtRead = interactionEpoch;
  try {
    const cached = await invoke<SnapshotPayload | null>("view_cache_load");
    // R04：晚到的缓存不得覆盖已落地的新结果（新汇总/明细先到 = 缓存
    // 不再接管）；卸载实例同样不恢复。
    if (disposed || freshArrived || report.value || events.value) return;
    if (interactionEpoch !== epochAtRead) return;
    // 只接受当前版快照；旧版本可能含混代数据，一律走正常加载。
    if (!cached || cached.v !== SNAPSHOT_VERSION || !cached.report || !cached.filters) return;
    applyingRestore = true;
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
    await nextTick();
    applyingRestore = false;
    // 恢复后恰好启动一批后台刷新（单一入口；watcher 已跳过，不重复查询）
    startRefreshBatch();
  } catch {
    applyingRestore = false;
    // 视图缓存损坏：静默忽略，走正常加载
  }
}
void loadViewCache();

onUnmounted(() => {
  disposed = true;
});

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

// C4/F03：手动刷新——以当前筛选重跑两条查询；也是用户操作，撤销启动
// 缓存的恢复资格。共享批次入口保证两条查询属于同一 epoch。
function manualRefresh(): void {
  interactionEpoch++;
  startRefreshBatch();
}

// F03：主刷新协调入口——同一 epoch 传给汇总与明细两条查询。
function startRefreshBatch(): void {
  refreshEpoch++;
  void refresh();
  void loadEvents();
}

const drillLabel = (d: EventDrill): string => `${d.type}: ${d.key}`;

// F03：watcher 不再 immediate——启动批次由 setup 末尾显式开启（不属于
// 用户操作，不递增 interactionEpoch）；恢复应用期间 watcher 跳过查询。
// 主筛选变化走 startRefreshBatch（同一 epoch 两条查询），不另设明细
// watcher——否则每次筛选变化发起两次明细查询。
watch([by, agent, range, tz], () => {
  if (applyingRestore) return;
  interactionEpoch++;
  startRefreshBatch();
});
// 下钻是用户操作：撤销启动缓存的恢复资格；明细作为当前主 epoch 的
// 子查询重扫（drill 版本），汇总沿用同批次结果。
watch(drill, () => {
  if (applyingRestore) return;
  interactionEpoch++;
  void loadEvents();
});
// refreshKey 是程序触发（价格同步等）：新批次但不算用户操作。
watch(
  () => props.refreshKey,
  () => {
    if (applyingRestore) return;
    startRefreshBatch();
  },
);
void loadSources();
// 启动首批查询（等价旧 immediate watcher，且在 loadViewCache 捕获
// interactionEpoch 之后执行，不撤销恢复资格）。
startRefreshBatch();
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
        size="medium"
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
    <!-- R08：采集诊断（空结果/部分结果均可见，不依赖表格挂载） -->
    <div v-if="!collectionDiagnostics.clean" class="ts-notice source-notice">
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
        本轮采集存在部分问题（{{ collectionDiagnostics.summaryText }}）；已采集数据仍有效。
      </span>
      <button
        v-if="collectionDiagnostics.warns.length > 0"
        type="button"
        class="ts-notice-action ts-focusable"
        :aria-expanded="showCollectionDetails"
        @click="showCollectionDetails = !showCollectionDetails"
      >
        详情
      </button>
    </div>
    <div v-if="showCollectionDetails" class="source-details">
      <div v-for="(w, i) in collectionDiagnostics.warns" :key="i" class="source-detail-line">
        {{ w }}
      </div>
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
