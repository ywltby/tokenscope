<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  NAlert,
  NButton,
  NCard,
  NDatePicker,
  NRadioButton,
  NRadioGroup,
  NSpin,
  NTag,
  NTooltip,
} from "naive-ui";
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

const dimOptions: { label: string; value: Dim }[] = [
  { label: "按日", value: "day" },
  { label: "按模型", value: "model" },
  { label: "按项目", value: "project" },
  { label: "按应用", value: "agent" },
];

const agentOptions: { label: string; value: AgentFilter; icon: string }[] = [
  { label: "全部", value: "all", icon: "all" },
  { label: "Claude Code", value: "claude", icon: "claude" },
  { label: "Codex", value: "codex", icon: "openai" },
];

// 区间快捷项（参考 cc-switch）；null = 全部时间
const rangeShortcuts: Record<string, () => [number, number]> = {
  近7天: () => [Date.now() - 7 * 86400e3, Date.now()],
  近30天: () => [Date.now() - 30 * 86400e3, Date.now()],
  近90天: () => [Date.now() - 90 * 86400e3, Date.now()],
};

/// 区间毫秒 → 解析时区下的 YYYY-MM-DD（避免前端再引时区库：local 用本地
/// 格式化，固定偏移时区按偏移折算；Asia/Shanghai +8 无夏令时）
function fmtDate(ms: number): string {
  if (tz.value === "local") return new Date(ms).toLocaleDateString("sv-SE");
  const offsetH = tz.value === "UTC" ? 0 : 8;
  return new Date(ms + offsetH * 3600e3).toISOString().slice(0, 10);
}

async function refresh(): Promise<void> {
  loading.value = true;
  try {
    report.value = await invoke<SummaryReport>("summarize", {
      by: by.value,
      agent: agent.value,
      days: null,
      from: range.value ? fmtDate(range.value[0]) : null,
      to: range.value ? fmtDate(range.value[1]) : null,
      tz: tz.value,
    });
  } finally {
    loading.value = false;
  }
}

async function loadSources(): Promise<void> {
  sourceStatus.value = await invoke<SourceStatus[]>("source_status");
}

async function loadEvents(): Promise<void> {
  eventsLoading.value = true;
  try {
    events.value = await invoke<EventList>("list_events", {
      agent: agent.value,
      from: range.value ? fmtDate(range.value[0]) : null,
      to: range.value ? fmtDate(range.value[1]) : null,
      model: drill.value?.type === "model" ? drill.value.key : null,
      project: drill.value?.type === "project" ? drill.value.key : null,
      day: drill.value?.type === "day" ? drill.value.key : null,
      limit: 200,
      tz: tz.value,
    });
  } finally {
    eventsLoading.value = false;
  }
}

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
        <NDatePicker
          v-model:value="range"
          type="daterange"
          clearable
          size="small"
          :shortcuts="rangeShortcuts"
          style="width: 260px"
        />
      </div>
    </div>
    <NSpin :show="loading">
      <!-- 最小高度保证加载转圈居中于可视区，避免空内容时贴顶被遮挡 -->
      <div style="min-height: 380px">
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
