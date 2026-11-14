<script setup lang="ts">
import { computed, h } from "vue";
import { NCollapse, NCollapseItem, NDataTable, type DataTableColumn } from "naive-ui";
import { AGENT_LABEL, fmtNum, type Group, type SummaryReport } from "../types";
import { formatMoney } from "../lib/formatMoney";
import { tokenBucketLabel } from "../lib/tokenDisplay";
import { displayTokens, totalTokens } from "../lib/tokenUsage";
import { buildSourceLines } from "../lib/statsView";
import ScrollList from "./ScrollList.vue";
import CostCell from "./CostCell.vue";
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
  // C5：未计价 token 单列可追溯（无价格模型 / 部分计价的缺价分项）。
  if (props.report.totals.unknown_pricing) {
    cols.push({
      title: t("dashboard.colUnknown"),
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
  onclick: () => activate(asGroup(row)),
  onkeydown: (e: KeyboardEvent) => {
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

/** 确有 rollup 桶（费用未知披露的范围）但来源没给费用 → 显示"来源未提供费用"。 */
const hasSourceCostRollupBuckets = computed<boolean>(() => {
  const coverage = props.report.rollup_coverage;
  return (coverage?.rollup_buckets ?? 0) > 0 || (coverage?.covered_buckets ?? 0) > 0;
});
</script>

<template>
  <section class="ts-card usage-card">
    <div class="card-head">
      <span class="card-title">{{ t("dashboard.aggregateTitle") }}</span>
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
    <!-- 2026-11-05：只有日粒度的历史是统计口径（已计入总量），费用按未知
         披露——说明放在汇总区，不再出现在"本轮采集存在部分问题"里。 -->
    <div v-if="report.totals.unknown_reason === 'rollup_only'" class="table-note">
      {{ t("dashboard.rollupOnly") }}
      <!-- T2：来源记录费用单独披露——只来自被统计采用的日汇总，不是请求级估算。 -->
      <span v-if="report.totals.source_cost_usd != null" class="source-cost-note">{{
        t("dashboard.sourceCostNote", {
          cost: formatMoney(report.totals.source_cost_usd, "summary"),
        })
      }}</span>
      <span v-else-if="hasSourceCostRollupBuckets" class="source-cost-note">{{
        t("dashboard.sourceCostAbsentNote")
      }}</span>
    </div>
    <div v-if="report.totals.unknown_pricing" class="table-note">
      {{ t("dashboard.tableNote") }}
    </div>
    <NCollapse class="source-collapse">
      <NCollapseItem :title="t('dashboard.sourceStats')" name="sources">
        <div v-for="s in sourceLines" :key="s.agent" class="source-line">
          <strong>{{ s.agent }}</strong
          >：{{ s.parts.join(" · ") }}
        </div>
        <!-- R08：warnings 已在页面级采集诊断通知展示，此处不再重复 -->
      </NCollapseItem>
      <!-- 2026-11-07：统计来源选择是正常统计口径，不弹告警；依据放在这层
           可展开的统计说明里（A 类按明细、B 类按汇总补足）。 -->
      <NCollapseItem
        v-if="selectionNotes.length > 0"
        :title="t('dashboard.selectionStatsNote')"
        name="selection"
      >
        <div v-for="(note, i) in selectionNotes" :key="i" class="source-line">
          {{ note }}
        </div>
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
/* v0.9.1 缺陷 B：费用列排版移入 CostCell.vue（render 回调节点不带本组件
   scope 标记，这里的 scoped 选择器永不命中——真实浏览器 computed display
   是 inline，金额与来源费用粘成一行）。脚注 .source-cost-note 仍在模板
   内渲染，保留在本组件。 */
/* T2：来源记录费用披露——次要色小字，与统计口径说明并列。 */
.source-cost-note {
  display: block;
  margin-top: var(--ts-space-1);
  color: var(--ts-text-muted);
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
