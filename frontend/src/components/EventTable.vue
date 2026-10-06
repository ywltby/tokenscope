<script setup lang="ts">
import { computed, h } from "vue";
import { NButton, NDataTable, NTag, NTooltip, type DataTableColumn } from "naive-ui";
import { AGENT_LABEL, fmtNum, projectLabel, type EventList } from "../types";
import { formatCostBreakdown } from "../lib/costBreakdown";

const props = defineProps<{
  list: EventList;
  filterLabel: string;
  /** D1 游标分页：还有未加载的行 */
  more?: boolean;
  moreLoading?: boolean;
}>();
const emit = defineEmits<{ (e: "load-more"): void }>();
const remaining = () => props.list.total - props.list.rows.length;

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
    // 设计系统 Task 5：模型名可聚焦获取完整值（不只 hover）
    render: (r) => h("span", { tabindex: 0, class: "ts-focusable" }, asRow(r).model),
  },
  {
    title: "项目",
    key: "project",
    minWidth: 100,
    // C2：明细项目显示末段（身份是完整路径），悬浮可见完整值。
    render: (r) => {
      const p = asRow(r).project;
      const label = projectLabel(p);
      if (label === p) return label;
      return h(NTooltip, null, {
        trigger: () =>
          h("span", { style: "cursor: help", tabindex: 0, class: "ts-focusable" }, label),
        default: () => p,
      });
    },
  },
  {
    title: "输入",
    key: "input",
    align: "right",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).input),
  },
  {
    title: "输出",
    key: "output",
    align: "right",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).output),
  },
  {
    title: "缓存写",
    key: "cache_write",
    align: "right",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).cache_write),
  },
  {
    title: "缓存读",
    key: "cache_read",
    align: "right",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).cache_read),
  },
  {
    title: "费用$(估算)",
    key: "cost_usd",
    align: "right",
    className: "ts-num",
    render: (r) => {
      const row = asRow(r);
      const c = row.cost_usd;
      const bd = row.cost_breakdown;
      const trigger =
        c == null
          ? () =>
              h(
                NTag,
                { size: "small", bordered: false, type: "warning" },
                { default: () => "未知" },
              )
          : () => h("span", { style: "cursor: help" }, fmtPrice(c));
      // Task 7：breakdown 存在时悬浮展示计算明细；旧/异常响应（有价无明细）
      // 保持原样不崩溃。
      if (!bd) return trigger();
      return h(
        NTooltip,
        { style: "max-width: 460px", placement: "left" },
        {
          trigger,
          default: () =>
            h(
              "div",
              { style: "font-size: 12px; line-height: 1.7; text-align: left" },
              formatCostBreakdown(bd).map((l) =>
                h(
                  "div",
                  {
                    style: l.unknown
                      ? "color: #f0a020; white-space: normal"
                      : "white-space: normal",
                  },
                  l.text,
                ),
              ),
            ),
        },
      );
    },
  },
]);

// 设计系统 Task 5：暴露列定义供组件测试断言（无行渲染环境）。
defineExpose({ columns });

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
      :bordered="false"
      :max-height="380"
      virtual-scroll
    />
    <div style="font-size: 12px; opacity: 0.6; margin-top: 4px">
      共 {{ fmtNum(props.list.total) }} 条（时间倒序）· 已加载
      {{ fmtNum(props.list.rows.length) }} 条
      <template v-if="props.filterLabel">· 筛选：{{ props.filterLabel }}</template>
    </div>
    <div v-if="props.more" style="text-align: center; margin-top: 6px">
      <NButton size="tiny" :loading="props.moreLoading" @click="emit('load-more')">
        加载更多（还剩 {{ fmtNum(remaining()) }} 条）
      </NButton>
    </div>
  </div>
</template>
