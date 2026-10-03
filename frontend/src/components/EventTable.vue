<script setup lang="ts">
import { computed, h } from "vue";
import { NDataTable, NTag, type DataTableColumn } from "naive-ui";
import { AGENT_LABEL, fmtNum, type EventList } from "../types";

const props = defineProps<{ list: EventList; filterLabel: string }>();

function fmtPrice(v: number): string {
  if (v === 0) return "0";
  if (v < 0.001) return v.toFixed(6);
  if (v < 1) return v.toFixed(4);
  return v.toFixed(2);
}

const columns = computed<DataTableColumn[]>(() => [
  { title: "时间", key: "ts", minWidth: 150 },
  {
    title: "Agent",
    key: "agent",
    render: (r) => AGENT_LABEL[asRow(r).agent] ?? asRow(r).agent,
  },
  {
    title: "模型",
    key: "model",
    minWidth: 180,
    ellipsis: { tooltip: true },
  },
  { title: "项目", key: "project", minWidth: 100, ellipsis: { tooltip: true } },
  { title: "输入", key: "input", align: "right", render: (r) => fmtNum(asRow(r).input) },
  { title: "输出", key: "output", align: "right", render: (r) => fmtNum(asRow(r).output) },
  {
    title: "缓存写",
    key: "cache_write",
    align: "right",
    render: (r) => fmtNum(asRow(r).cache_write),
  },
  {
    title: "缓存读",
    key: "cache_read",
    align: "right",
    render: (r) => fmtNum(asRow(r).cache_read),
  },
  {
    title: "费用$",
    key: "cost_usd",
    align: "right",
    render: (r) => {
      const c = asRow(r).cost_usd;
      if (c == null) {
        return h(
          NTag,
          { size: "small", bordered: false, type: "warning" },
          { default: () => "未知" },
        );
      }
      return fmtPrice(c);
    },
  },
]);

type EventRowT = EventList["rows"][number];
const asRow = (r: object): EventRowT => r as unknown as EventRowT;

const rowKey = (r: object): string => {
  const e = asRow(r);
  return `${e.ts}|${e.agent}|${e.model}|${e.session_id}`;
};
</script>

<template>
  <div>
    <NDataTable
      :columns="columns"
      :data="props.list.rows"
      :row-key="rowKey"
      size="small"
      :bordered="true"
      :single-line="false"
      :max-height="380"
      virtual-scroll
    />
    <div style="font-size: 12px; opacity: 0.6; margin-top: 4px">
      共 {{ fmtNum(props.list.total) }} 条（时间倒序）· 显示前
      {{ fmtNum(props.list.rows.length) }} 条
      <template v-if="props.filterLabel">· 筛选：{{ props.filterLabel }}</template>
    </div>
  </div>
</template>
