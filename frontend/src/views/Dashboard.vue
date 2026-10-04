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
// 启动提速（用户 2026-10-04）：先渲染上次视图快照，再后台刷新替换
const stale = ref(false);
const cachedAt = ref<string | null>(null);
let runSeq = 0;

const dimOptions: { label: string; value: Dim }[] = [
  { label: "按日", value: "day" },
  { label: "按模型", value: "model" },
  { label: "按项目", value: "project" },
  { label: "按应用", value: "agent" },
];

/// 区间毫秒 → 解析时区下的 YYYY-MM-DD（local 用本地格式化；固定偏移时区
/// 按偏移折算，Asia/Shanghai +8 无夏令时）
function fmtDate(ms: number): string {
  if (tz.value === "local") return new Date(ms).toLocaleDateString("sv-SE");
  const offsetH = tz.value === "UTC" ? 0 : 8;
  return new Date(ms + offsetH * 3600e3).toISOString().slice(0, 10);
}

const agentOptions: { label: string; value: AgentFilter; icon: string }[] = [
  { label: "全部", value: "all", icon: "all" },
  { label: "Claude Code", value: "claude", icon: "claude" },
  { label: "Codex", value: "codex", icon: "openai" },
];

async function refresh(): Promise<void> {
  const seq = ++runSeq;
  loading.value = true;
  try {
    const fresh = await invoke<SummaryReport>("summarize", {
      by: by.value,
      agent: agent.value,
      days: null,
      from: range.value ? fmtDate(range.value[0]) : null,
      to: range.value ? fmtDate(range.value[1]) : null,
      tz: tz.value,
    });
    if (seq !== runSeq) return; // 已有更新的查询，丢弃旧响应
    report.value = fresh;
    stale.value = false;
    void invoke("view_cache_save", {
      value: { report: fresh, events: events.value, saved_at: new Date().toISOString() },
    });
  } finally {
    if (seq === runSeq) loading.value = false;
  }
}

async function loadSources(): Promise<void> {
  sourceStatus.value = await invoke<SourceStatus[]>("source_status");
}

async function loadEvents(): Promise<void> {
  const seq = ++runSeq;
  eventsLoading.value = true;
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
    if (seq !== runSeq) return;
    events.value = list;
    void invoke("view_cache_save", {
      value: { report: report.value, events: list, saved_at: new Date().toISOString() },
    });
  } finally {
    if (seq === runSeq) eventsLoading.value = false;
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
