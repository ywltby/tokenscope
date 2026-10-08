<script setup lang="ts">
import { computed, h } from "vue";
import { NCollapse, NCollapseItem, NDataTable, type DataTableColumn } from "naive-ui";
import { AGENT_LABEL, fmtNum, type Group, type SummaryReport } from "../types";
import { formatMoney } from "../lib/formatMoney";
import { tokenBucketLabel } from "../lib/tokenDisplay";
import { buildSourceLines } from "../lib/statsView";

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
    // C2：项目身份是完整路径，展示用 label（末段），悬浮可见完整 key。
    // 任务 5：可点击行右侧 › 指示（合计行不可点，不带）。
    render: (row) => {
      const g = asGroup(row);
      const isTotal = g.key === "合计";
      return h("span", { class: "dim-cell" }, [
        g.label ?? g.key,
        ...(isTotal ? [] : [h("span", { class: "drill-arrow", "aria-hidden": "true" }, " ›")]),
      ]);
    },
  };
  // 数字列居中，同时保留 tabular lining 数字。
  const num = (title: string, key: string): DataTableColumn => ({
    title,
    key,
    align: "center",
    className: "ts-num",
    render: (row) => fmtNum(Number((row as Record<string, unknown>)[key] ?? 0)),
  });
  const token = (
    title: string,
    path: "input" | "output" | "cache_write" | "cache_read",
  ): DataTableColumn => ({
    title,
    key: `tokens.${path}`,
    align: "center",
    className: "ts-num",
    render: (row) => fmtNum(asGroup(row).tokens[path]),
  });
  const cols: DataTableColumn[] = [
    first,
    num("请求", "requests"),
    token(tokenBucketLabel("input"), "input"),
    token(tokenBucketLabel("output"), "output"),
    token(tokenBucketLabel("cache_write"), "cache_write"),
    token(tokenBucketLabel("cache_read"), "cache_read"),
    {
      title: "合计",
      key: "total",
      align: "center",
      className: "ts-num",
      render: (row) =>
        fmtNum(
          asGroup(row).tokens.input +
            asGroup(row).tokens.output +
            asGroup(row).tokens.cache_write +
            asGroup(row).tokens.cache_read,
        ),
    },
    {
      // 设计系统 Task 5：金额列使用估算语义表头
      title: "费用$(估算)",
      key: "cost_usd",
      align: "center",
      className: "ts-num",
      render: (row) => {
        const g = asGroup(row);
        const text = formatMoney(g.cost_usd, "summary") + (g.unknown_pricing ? "†" : "");
        // 未知标记用警告色显式呈现，不用 opacity 压低（DESIGN.md §1）
        return h("span", { style: g.unknown_pricing ? "color: var(--ts-warning)" : "" }, text);
      },
    },
  ];
  // C5：未计价 token 单列可追溯（无价格模型 / 部分计价的缺价分项）。
  if (props.report.totals.unknown_pricing) {
    cols.push({
      title: "未知†",
      key: "unknown_tokens",
      align: "center",
      render: (row) => {
        const u = asGroup(row).unknown_tokens;
        const n = u.input + u.output + u.cache_write + u.cache_read;
        return n > 0 ? h("span", { style: "color: var(--ts-warning)" }, fmtNum(n)) : "—";
      },
    });
  }
  return cols;
});

const rows = computed<Group[]>(() => props.report.groups);
const rowKey = (row: object): string => asGroup(row).key;
const rowClass = (row: object): string => (asGroup(row).key === "合计" ? "total-row" : "");
// 设计系统 Task 5：行可聚焦，Enter/Space 与点击等价下钻（键盘路径）；
// ts-focusable 提供 2px 焦点环。
const rowProps = (row: object) => ({
  style: "cursor: pointer",
  class: "ts-focusable",
  tabindex: 0,
  role: "button",
  "aria-label": `查看 ${asGroup(row).key} 的请求明细`,
  onclick: () => emit("row-click", asGroup(row).key),
  onkeydown: (e: KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      emit("row-click", asGroup(row).key);
    }
  },
});

// 设计系统 Task 5：暴露列定义与行 props 供组件测试断言（无行渲染环境）。
defineExpose({ columns, rowProps });

const sourceLines = computed(() =>
  buildSourceLines(props.report.sources).map((s) => ({
    agent: AGENT_LABEL[s.agent] ?? s.agent,
    parts: s.parts,
  })),
);
</script>

<template>
  <section class="ts-card usage-card">
    <div class="card-head">
      <span class="card-title">聚合</span>
    </div>
    <NDataTable
      :columns="columns"
      :data="rows"
      :row-key="rowKey"
      :row-class-name="rowClass"
      :row-props="rowProps"
      size="small"
      :bordered="false"
      table-layout="auto"
      class="ts-auto-table"
    />
    <div class="table-hint">点击行（或聚焦后按 Enter）可下钻到请求明细。</div>
    <div v-if="report.totals.unknown_pricing" class="table-note">
      † 费用为估算，仅含已计价部分：无价格模型的全部用量、或价格快照缺分项价
      （如缓存价未知）时该分项的用量，均不计入费用，其 token 数见"未知†"列。
    </div>
    <NCollapse class="source-collapse">
      <NCollapseItem title="来源采集统计" name="sources">
        <div v-for="s in sourceLines" :key="s.agent" class="source-line">
          <strong>{{ s.agent }}</strong
          >：{{ s.parts.join(" · ") }}
        </div>
        <!-- R08：warnings 已在页面级采集诊断通知展示，此处不再重复 -->
      </NCollapseItem>
    </NCollapse>
  </section>
</template>

<style scoped>
/* 卡头：标题 17px/600（DESIGN.md §5 卡片头部） */
.card-head {
  display: flex;
  align-items: baseline;
  gap: var(--ts-space-3);
  margin-bottom: var(--ts-space-2);
}
.card-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

/* 可点击行右侧 › 指示（辅助信息，不承载唯一信息） */
.drill-arrow {
  color: var(--ts-text-muted);
}

/* 合计行上方一条 stronger separator（DESIGN.md §5 表格） */
:deep(.total-row td) {
  border-top: 1px solid var(--ts-separator-strong);
}
/* UX01：合计行字重 600 命中真实单元格（修复前是死选择器 .total-row strong，
   实际合计单元格由 render 输出 span，字重仍是 400） */
:deep(.total-row td) {
  font-weight: 600;
}
.table-hint {
  font-size: 12px;
  color: var(--ts-text-muted);
  margin-top: var(--ts-space-1);
}
.table-note {
  font-size: 12px;
  line-height: 1.5;
  color: var(--ts-text-secondary);
  margin-top: var(--ts-space-2);
}
.source-collapse {
  margin-top: var(--ts-space-2);
}
.source-line {
  font-size: 12px;
  line-height: 1.9;
  color: var(--ts-text-secondary);
}
</style>
