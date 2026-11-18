<script setup lang="ts">
import { computed, h } from "vue";
import { NDataTable, type DataTableColumn } from "naive-ui";
import { AGENT_LABEL, fmtNum, type Group, type SummaryReport } from "../types";
import { tokenBucketLabel } from "../lib/tokenDisplay";
import { displayTokens, totalTokens } from "../lib/tokenUsage";
import { buildSourceLines } from "../lib/statsView";
import ScrollList from "./ScrollList.vue";
import CostCell from "./CostCell.vue";
import AggregateInfoTooltip from "./AggregateInfoTooltip.vue";
import { useT } from "../lib/i18n";

const t = useT();

const props = defineProps<{ report: SummaryReport }>();
/// U04：合计行不是维度值——用**独立事件**通知父级，父级不再靠字面量「合计」
/// 判断（真实维度行走 `row-click`，合计行走 `total-row`）。
const emit = defineEmits<{ (e: "row-click", key: string): void; (e: "total-row"): void }>();

/// 后端契约：groups 含合计行，key 为字面「合计」（行样式、`›` 指示与事件分流用）。
const TOTAL_ROW_KEY = "合计";

/// Naive UI 表格行类型是 Record<string, unknown>，统一经 unknown 转 Group。
const asGroup = (row: object): Group => row as unknown as Group;

const DIM_LABEL = (): Record<string, string> => ({
  day: t("dashboard.drillDay"),
  model: t("dashboard.drillModel"),
  project: t("dashboard.drillProject"),
  agent: t("dashboard.colAgent"),
});

const columns = computed<DataTableColumn[]>(() => {
  const first: DataTableColumn = {
    title: DIM_LABEL()[props.report.by] ?? t("dashboard.colDim"),
    key: "key",
    // C2：项目身份是完整路径，展示用 label（末段），悬浮可见完整 key。
    // 任务 5：可点击行右侧 › 指示（合计行不可点，不带）。
    render: (row) => {
      const g = asGroup(row);
      const isTotal = g.key === TOTAL_ROW_KEY;
      // i18n：合计行的**展示名**按当前语言显示，身份键仍是后端契约的
      // `合计`（行样式、事件分流与下钻匹配都用它，绝不翻译）。
      return h("span", { class: "dim-cell" }, [
        isTotal ? t("dashboard.totalRow") : (g.label ?? g.key),
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
    render: (row) => fmtNum(displayTokens(asGroup(row).tokens)[path]),
  });
  const cols: DataTableColumn[] = [
    first,
    num(t("dashboard.colRequests"), "requests"),
    token(tokenBucketLabel("input"), "input"),
    token(tokenBucketLabel("output"), "output"),
    token(tokenBucketLabel("cache_write"), "cache_write"),
    token(tokenBucketLabel("cache_read"), "cache_read"),
    {
      title: t("dashboard.colTotal"),
      key: "total",
      align: "center",
      className: "ts-num",
      render: (row) => fmtNum(totalTokens(asGroup(row).tokens)),
    },
    {
      // 设计系统 Task 5：金额列使用估算语义表头
      title: t("dashboard.colCost"),
      key: "cost_usd",
      align: "center",
      className: "ts-num",
      // v0.9.1 缺陷 B：render 回调创建的节点不带本组件 scope 标记，scoped
      // CSS 永不命中（真实浏览器 computed display = inline，金额与来源费用
      // 粘成一行）。费用列抽成独立组件 CostCell——SFC scoped 天然生效；
      // 缺陷 C 的费用状态展示规则也收在该组件内。
      render: (row) => h(CostCell, { group: asGroup(row) }),
    },
  ];
  return cols;
});

const rows = computed<Group[]>(() => props.report.groups);
const rowKey = (row: object): string => asGroup(row).key;
const rowClass = (row: object): string => (asGroup(row).key === TOTAL_ROW_KEY ? "total-row" : "");
/// U04：行激活统一入口——合计行发 `total-row`（不是维度值，不可下钻），
/// 其余行走 `row-click`。
function activate(g: Group): void {
  if (g.key === TOTAL_ROW_KEY) emit("total-row");
  else emit("row-click", g.key);
}
// 设计系统 Task 5：行可聚焦，Enter/Space 与点击等价下钻（键盘路径）；
// ts-focusable 提供 2px 焦点环。
const rowProps = (row: object) => ({
  style: "cursor: pointer",
  class: "ts-focusable",
  tabindex: 0,
  role: "button",
  // 复核修订：可访问名称用展示名（模型维度是友好名），身份键只用于点击参数——
  // 屏幕阅读器不应读出 `claudeopus55` 这类压缩身份键。
  "aria-label": t("dashboard.viewEventsAria", { name: asGroup(row).label ?? asGroup(row).key }),
  // PR #72：行内金额按钮的点击/键盘只操作费用浮层——行处理器只响应落在
  // 行自身的激活，冒泡自按钮的事件一律放行，保留原生 button 的一次 click。
  onclick: (e: MouseEvent) => {
    if ((e.target as Element | null)?.closest("button")) return;
    activate(asGroup(row));
  },
  onkeydown: (e: KeyboardEvent) => {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      activate(asGroup(row));
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

/** 可展开的统计来源说明：正常选择不是告警，依据只在这里给出。 */
const selectionNotes = computed<string[]>(() => {
  const coverage = props.report.rollup_coverage;
  if (!coverage) return [];
  const notes: string[] = [];
  if ((coverage.detail_selected_buckets ?? 0) > 0) {
    notes.push(t("dashboard.selectionBasis", { count: coverage.detail_selected_buckets }));
  }
  if ((coverage.conflict_kinds ?? []).length > 0) {
    notes.push(t("dashboard.sourceConflictBasis"));
  }
  return notes;
});
</script>

<template>
  <section class="ts-card usage-card">
    <div class="card-head">
      <AggregateInfoTooltip :source-lines="sourceLines" :selection-notes="selectionNotes">{{
        t("dashboard.aggregateTitle")
      }}</AggregateInfoTooltip>
    </div>
    <ScrollList :rows="rows" :label="t('dashboard.aggregateListAria')">
      <template #default="{ rows: visibleRows }">
        <NDataTable
          :columns="columns"
          :data="visibleRows"
          :row-key="rowKey"
          :row-class-name="rowClass"
          :row-props="rowProps"
          size="small"
          :bordered="false"
          table-layout="auto"
          class="ts-auto-table"
        />
      </template>
    </ScrollList>
    <div class="table-hint">{{ t("dashboard.tableHint") }}</div>
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
/* v0.9.1 缺陷 B：费用列排版移入 CostCell.vue（render 回调节点不带本组件
   scope 标记，这里的 scoped 选择器永不命中——真实浏览器 computed display
   是 inline，金额与来源费用粘成一行）。 */
/* v0.9.3：表格后的两组说明与独立未知列已移除——说明进「聚合」标题浮层，
   未定价 token 明细进费用浮层。 */
</style>
