<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { NAlert, NButton, NCard, NRadioButton, NRadioGroup, NSpin, NTag, NTooltip } from "naive-ui";
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
import AgentIcon from "../components/AgentIcon.vue";
import DateRangeSelect from "../components/DateRangeSelect.vue";
import { useTimezone } from "../composables/timezone";
import { tzDate } from "../lib/dates";

const props = defineProps<{ refreshKey: number }>();

const by = ref<Dim>("day");
const agent = ref<AgentFilter>("all");
const range = ref<[number, number] | null>(null);
const { tz } = useTimezone();
const report = ref<SummaryReport | null>(null);
const loading = ref(false);
const sourceStatus = ref<SourceStatus[]>([]);
const drill = ref<EventDrill | null>(null);
const events = ref<EventList | null>(null);
const eventsLoading = ref(false);
// F01（计划 A2）：汇总与明细各自持有请求代次——曾共用一个 runSeq，两个
// immediate watcher 依次启动使后者作废前者，首屏永远转圈或停在旧快照。
let summarySeq = 0;
let eventsSeq = 0;
// 失败必须可见并可重试（计划 ipc_error_visible），不再无声吞异常。
const summaryError = ref<string | null>(null);
const eventsError = ref<string | null>(null);
// 启动提速（用户 2026-10-04）：先渲染上次视图快照，再后台刷新替换
const stale = ref(false);
const cachedAt = ref<string | null>(null);

const dimOptions: { label: string; value: Dim }[] = [
  { label: "按日", value: "day" },
  { label: "按模型", value: "model" },
  { label: "按项目", value: "project" },
  { label: "按应用", value: "agent" },
];

/// 区间毫秒 → 解析时区下的 YYYY-MM-DD（C3：Intl 按所选时区当日实际偏移，
/// DST 正确；修复前非 UTC 一律按 +8 折算）。
function fmtDate(ms: number): string {
  return tzDate(ms, tz.value);
}

const agentOptions: { label: string; value: AgentFilter; icon: string }[] = [
  { label: "全部", value: "all", icon: "all" },
  { label: "Claude Code", value: "claude", icon: "claude" },
  { label: "Codex", value: "codex", icon: "openai" },
];

async function refresh(): Promise<void> {
  const seq = ++summarySeq;
  loading.value = true;
  summaryError.value = null;
  try {
    const fresh = await invoke<SummaryReport>("summarize", {
      by: by.value,
      agent: agent.value,
      days: null,
      from: range.value ? fmtDate(range.value[0]) : null,
      to: range.value ? fmtDate(range.value[1]) : null,
      tz: tz.value,
    });
    if (seq !== summarySeq) return; // 已有更新的查询，丢弃旧响应
    report.value = fresh;
    stale.value = false;
    void invoke("view_cache_save", {
      value: { report: fresh, events: events.value, saved_at: new Date().toISOString() },
    });
  } catch (e) {
    if (seq === summarySeq) summaryError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (seq === summarySeq) loading.value = false;
  }
}

async function loadSources(): Promise<void> {
  sourceStatus.value = await invoke<SourceStatus[]>("source_status");
}

async function loadEvents(): Promise<void> {
  const seq = ++eventsSeq;
  eventsLoading.value = true;
  eventsError.value = null;
  try {
    const list = await invoke<EventList>("list_events", {
      agent: agent.value,
      from: range.value ? fmtDate(range.value[0]) : null,
      to: range.value ? fmtDate(range.value[1]) : null,
      model: drill.value?.type === "model" ? drill.value.key : null,
      project: drill.value?.type === "project" ? drill.value.key : null,
      day: drill.value?.type === "day" ? drill.value.key : null,
      limit: 200,
      tz: tz.value,
    });
    if (seq !== eventsSeq) return;
    events.value = list;
    void invoke("view_cache_save", {
      value: { report: report.value, events: list, saved_at: new Date().toISOString() },
    });
  } catch (e) {
    if (seq === eventsSeq) eventsError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (seq === eventsSeq) eventsLoading.value = false;
  }
}

async function loadViewCache(): Promise<void> {
  try {
    const cached = await invoke<{
      report: SummaryReport;
      events: EventList;
      saved_at: string;
    } | null>("view_cache_load");
    if (cached?.report && !report.value) {
      report.value = cached.report;
      events.value = cached.events;
      cachedAt.value = cached.saved_at;
      stale.value = true;
    }
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

const drillLabel = (d: EventDrill): string => `${d.type}: ${d.key}`;

watch([by, agent, range, tz, () => props.refreshKey], refresh, { immediate: true });
watch([by, agent, range, tz, drill, () => props.refreshKey], loadEvents, { immediate: true });
void loadSources();
</script>

<template>
  <div>
    <NAlert
      v-for="s in sourceStatus.filter((x) => !x.exists)"
      :key="s.agent"
      type="warning"
      style="margin-bottom: 12px"
    >
      {{ AGENT_LABEL[s.agent] ?? s.agent }} 数据目录不存在（{{ s.dir }}），该来源将没有统计。
    </NAlert>
    <!-- 筛选两行：第一行 agent 工具，第二行筛选条件（时区在设置页） -->
    <div style="display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px">
      <div class="agent-row">
        <NTooltip v-for="o in agentOptions" :key="o.value">
          <template #trigger>
            <NButton
              quaternary
              class="agent-btn"
              :type="agent === o.value ? 'primary' : 'default'"
              :aria-label="o.label"
              @click="agent = o.value"
            >
              <AgentIcon :name="o.icon" :size="20" />
            </NButton>
          </template>
          {{ o.label }}
        </NTooltip>
      </div>
      <div style="display: flex; align-items: center; gap: 16px">
        <NRadioGroup v-model:value="by" size="small">
          <NRadioButton v-for="o in dimOptions" :key="o.value" :value="o.value" :label="o.label" />
        </NRadioGroup>
        <DateRangeSelect v-model:value="range" />
      </div>
    </div>
    <!-- 失败可见并可重试（计划 A2）：保留已有数据展示，不整体灰罩 -->
    <NAlert v-if="summaryError" type="error" style="margin-bottom: 12px">
      汇总加载失败：{{ summaryError }}
      <NButton size="tiny" style="margin-left: 8px" @click="refresh">重试</NButton>
    </NAlert>
    <NAlert v-if="eventsError" type="error" style="margin-bottom: 12px">
      明细加载失败：{{ eventsError }}
      <NButton size="tiny" style="margin-left: 8px" @click="loadEvents">重试</NButton>
    </NAlert>
    <!-- 有数据时不再全屏灰罩：数据原地更新，右上角提示刷新中 -->
    <NSpin :show="loading && !report">
      <!-- 最小高度保证加载转圈居中于可视区，避免空内容时贴顶被遮挡 -->
      <div style="min-height: 380px">
        <div v-if="report" style="display: flex; justify-content: flex-end; margin-bottom: 8px">
          <NTag v-if="stale" size="small" type="warning" :bordered="false">
            缓存数据（{{ cachedAt ?? "" }}）· 后台刷新中
          </NTag>
          <NTag v-else-if="loading" size="small" type="info" :bordered="false">刷新中…</NTag>
        </div>
        <template v-if="report">
          <SummaryCards :totals="report.totals" />
          <TrendChart
            v-if="report.by === 'day' || report.groups.length > 2"
            :groups="report.groups.filter((g) => g.key !== '合计')"
            :by="report.by"
            style="margin-top: 12px"
          />
          <UsageTable :report="report" style="margin-top: 12px" @row-click="onSummaryRowClick" />
          <NCard v-if="events" size="small" style="margin-top: 12px">
            <template #header>
              请求明细
              <NTag
                v-if="drill"
                size="small"
                closable
                type="info"
                style="margin-left: 8px"
                @close="clearDrill"
              >
                {{ drillLabel(drill) }}
              </NTag>
            </template>

            <EventTable
              :list="events"
              :filter-label="drill ? drillLabel(drill) : '无（显示最新 200 条）'"
            />
          </NCard>
        </template>
      </div>
    </NSpin>
  </div>
</template>

<style scoped>
.agent-row {
  display: inline-flex;
  align-self: flex-start; /* 纵向 flex 容器默认 stretch 会把外框拉满整行 */
  align-items: center;
  border: 1px solid rgba(128, 128, 128, 0.3);
  border-radius: 8px;
  overflow: hidden;
}
.agent-btn {
  width: 40px;
  height: 40px;
  padding: 0;
  border: none;
  border-radius: 0;
}
/* 相邻格之间的细分隔线，整体仍是一个元素 */
.agent-btn + .agent-btn {
  border-left: 1px solid rgba(128, 128, 128, 0.3);
}
</style>
