<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  darkTheme,
  NAlert,
  NConfigProvider,
  NGlobalStyle,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NMessageProvider,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NSwitch,
 NSpin,
  type GlobalTheme,
} from "naive-ui";
import { useTheme } from "./composables/theme";
import { AGENT_LABEL, type AgentFilter, type Dim, type SourceStatus, type SummaryReport } from "./types";
import SummaryCards from "./components/SummaryCards.vue";
import UsageTable from "./components/UsageTable.vue";
import TrendChart from "./components/TrendChart.vue";

const { mode, toggle } = useTheme();
const theme = computed<GlobalTheme | null>(() => (mode.value === "dark" ? darkTheme : null));

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

watch([by, agent, days], refresh, { immediate: true });
void invoke<SourceStatus[]>("source_status").then((st) => (sourceStatus.value = st));
</script>

<template>
  <NConfigProvider :theme="theme">
    <NGlobalStyle />
    <NMessageProvider>
      <NLayout style="height: 100vh">
        <NLayoutHeader bordered style="padding: 12px 20px; display: flex; align-items: center; gap: 16px">
          <strong style="font-size: 18px">TokenScope</strong>
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
          <NSelect
            v-model:value="days"
            :options="dayOptions"
            size="small"
            style="width: 130px"
          />
          <div style="flex: 1" />
          <span style="font-size: 12px; opacity: 0.65">暗色</span>
          <NSwitch :value="mode === 'dark'" size="small" @update:value="toggle" />
        </NLayoutHeader>
        <NLayoutContent style="padding: 16px 20px">
          <NAlert
            v-for="s in sourceStatus.filter((x) => !x.exists)"
            :key="s.agent"
            type="warning"
            style="margin-bottom: 12px"
          >
            {{ AGENT_LABEL[s.agent] ?? s.agent }} 数据目录不存在（{{ s.dir }}），该来源将没有统计。
          </NAlert>
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
        </NLayoutContent>
      </NLayout>
    </NMessageProvider>
  </NConfigProvider>
</template>
