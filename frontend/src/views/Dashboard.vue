<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  NAlert,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NSpin,
} from "naive-ui";
import {
  AGENT_LABEL,
  type AgentFilter,
  type Dim,
  type SourceStatus,
  type SummaryReport,
} from "../types";
import SummaryCards from "../components/SummaryCards.vue";
import UsageTable from "../components/UsageTable.vue";
import TrendChart from "../components/TrendChart.vue";

const props = defineProps<{ refreshKey: number }>();

const by = ref<Dim>("day");
const agent = ref<AgentFilter>("all");
const days = ref<number>(0);
const report = ref<SummaryReport | null>(null);
const loading = ref(false);
const sourceStatus = ref<SourceStatus[]>([]);

const dimOptions: { label: string; value: Dim }[] = [
  { label: "按日", value: "day" },
  { label: "按模型", value: "model" },
  { label: "按项目", value: "project" },
  { label: "按 Agent", value: "agent" },
];

const agentOptions: { label: string; value: AgentFilter }[] = [
  { label: "全部 Agent", value: "all" },
  { label: "Claude Code", value: "claude" },
  { label: "Codex", value: "codex" },
];

const dayOptions: { label: string; value: number }[] = [
  { label: "全部时间", value: 0 },
  { label: "近 7 天", value: 7 },
  { label: "近 30 天", value: 30 },
  { label: "近 90 天", value: 90 },
];

async function refresh(): Promise<void> {
  loading.value = true;
  try {
    report.value = await invoke<SummaryReport>("summarize", {
      by: by.value,
      agent: agent.value,
      days: days.value === 0 ? null : days.value,
    });
  } finally {
    loading.value = false;
  }
}

async function loadSources(): Promise<void> {
  sourceStatus.value = await invoke<SourceStatus[]>("source_status");
}

watch([by, agent, days, () => props.refreshKey], refresh, { immediate: true });
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
    <div style="display: flex; align-items: center; gap: 16px; margin-bottom: 12px">
      <NRadioGroup v-model:value="agent" size="small">
        <NRadioButton
          v-for="o in agentOptions"
          :key="o.value"
          :value="o.value"
          :label="o.label"
        />
      </NRadioGroup>
      <NRadioGroup v-model:value="by" size="small">
        <NRadioButton
          v-for="o in dimOptions"
          :key="o.value"
          :value="o.value"
          :label="o.label"
        />
      </NRadioGroup>
      <NSelect v-model:value="days" :options="dayOptions" size="small" style="width: 130px" />
    </div>
    <NSpin :show="loading">
      <template v-if="report">
        <SummaryCards :totals="report.totals" />
        <TrendChart
          v-if="report.by === 'day' || report.groups.length > 2"
          :groups="report.groups.filter((g) => g.key !== '合计')"
          :by="report.by"
          style="margin-top: 12px"
        />
        <UsageTable :report="report" style="margin-top: 12px" />
      </template>
    </NSpin>
  </div>
</template>
