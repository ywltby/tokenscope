<script setup lang="ts">
import { computed, h } from "vue";
import { NCollapse, NCollapseItem, NDataTable, type DataTableColumn } from "naive-ui";
import {
  AGENT_LABEL,
  STAT_KEYS,
  fmtCost,
  fmtNum,
  type Group,
  type SummaryReport,
} from "../types";

const props = defineProps<{ report: SummaryReport }>();
const emit = defineEmits<{ (e: "row-click", key: string): void }>();

/// Naive UI 表格行类型是 Record<string, unknown>，统一经 unknown 转 Group。
const asGroup = (row: object): Group => row as unknown as Group;

const DIM_LABEL: Record<string, string> = {
  day: "日期",
  model: "模型",
  project: "项目",
  agent: "Agent",
};

const columns = computed<DataTableColumn[]>(() => {
  const first: DataTableColumn = {
    title: DIM_LABEL[props.report.by] ?? "维度",
    key: "key",
    minWidth: 140,
    ellipsis: { tooltip: true },
  };
  const num = (title: string, key: string): DataTableColumn => ({
    title,
    key,
    align: "right",
    render: (row) => fmtNum(Number((row as Record<string, unknown>)[key] ?? 0)),
  });
  const token = (title: string, path: "input" | "output" | "cache_write" | "cache_read"): DataTableColumn => ({
    title,
    key: `tokens.${path}`,
    align: "right",
    render: (row) => fmtNum(asGroup(row).tokens[path]),
  });
  return [
    first,
    num("请求", "requests"),
    token("输入", "input"),
    token("输出", "output"),
    token("缓存写", "cache_write"),
    token("缓存读", "cache_read"),
    {
      title: "合计",
      key: "total",
      align: "right",
      render: (row) =>
        fmtNum(
          asGroup(row).tokens.input +
            asGroup(row).tokens.output +
            asGroup(row).tokens.cache_write +
            asGroup(row).tokens.cache_read,
        ),
    },
    {
      title: "费用$",
      key: "cost_usd",
      align: "right",
      render: (row) => {
        const g = asGroup(row);
        const text = fmtCost(g.cost_usd) + (g.unknown_pricing ? "†" : "");
        return h("span", { style: g.unknown_pricing ? "opacity: 0.75" : "" }, text);
      },
    },
  ];
});

const rows = computed<Group[]>(() => props.report.groups);
const rowKey = (row: object): string => asGroup(row).key;
const rowClass = (row: object): string =>
  asGroup(row).key === "合计" ? "total-row" : "";
const rowProps = (row: object) => ({
  style: "cursor: pointer",
  onclick: () => emit("row-click", asGroup(row).key),
});

const sourceLines = computed(() =>
  props.report.sources.map((s) => ({
    agent: AGENT_LABEL[s.agent] ?? s.agent,
    parts: STAT_KEYS.filter((k) => (s.stats[k.key] ?? 0) > 0 || k.key === "files_scanned" || k.key === "events").map(
      (k) => `${k.label} ${fmtNum(s.stats[k.key] ?? 0)}`,
    ),
  })),
);
</script>

<template>
  <div>
    <NDataTable
      :columns="columns"
      :data="rows"
      :row-key="rowKey"
      :row-class-name="rowClass"
      :row-props="rowProps"
      size="small"
      :bordered="true"
      :single-line="false"
    />
    <div style="font-size: 12px; opacity: 0.6; margin-top: 4px">点击行可下钻到请求明细。</div>
    <div v-if="report.totals.unknown_pricing" style="font-size: 12px; opacity: 0.7; margin-top: 6px">
      † 部分用量来自无价格模型，费用仅含已计价部分（unknown 用量见合计行明细）。
    </div>
    <NCollapse style="margin-top: 8px">
      <NCollapseItem title="来源采集统计" name="sources">
        <div v-for="s in sourceLines" :key="s.agent" style="font-size: 12px; opacity: 0.8; line-height: 1.9">
          <strong>{{ s.agent }}</strong>：{{ s.parts.join(" · ") }}
        </div>
        <div v-if="report.warnings.length" style="font-size: 12px; color: #d97706">
          {{ report.warnings.join("；") }}
        </div>
      </NCollapseItem>
    </NCollapse>
  </div>
</template>

<style>
.total-row strong {
  font-weight: 700;
}
</style>
