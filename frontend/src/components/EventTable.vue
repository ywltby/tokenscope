<script setup lang="ts">
import { computed, h, ref } from "vue";
import { NButton, NDataTable, NTag, NTooltip, type DataTableColumn } from "naive-ui";
import { AGENT_LABEL, fmtNum, projectLabel, type EventList } from "../types";
import { formatCostBreakdownRows } from "../lib/costBreakdown";

const props = defineProps<{
  list: EventList;
  filterLabel: string;
  /** D1 游标分页：还有未加载的行 */
  more?: boolean;
  moreLoading?: boolean;
  /** 任务 5：下钻筛选标签可关闭（由 Dashboard 传当前是否处于下钻态） */
  filterClosable?: boolean;
}>();
const emit = defineEmits<{ (e: "load-more"): void; (e: "clear-filter"): void }>();
/// 设计系统 Task 6：当前展开的费用 tooltip（行 cursor 作为键）。
const openKey = ref<string | null>(null);
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
      const key = row.cursor;
      // c 在此分支已非空（下方 !bd 分支共用 trigger 前置判断）
      const trigger =
        c == null
          ? () =>
              h(
                NTag,
                { size: "small", bordered: false, type: "warning" },
                { default: () => "未知" },
              )
          : () => h("span", { style: "cursor: help" }, fmtPrice(c));
      // 设计系统 Task 6：breakdown 存在时展示"事实→公式→结果→来源"；
      // 触发器支持 hover/focus/click，Escape 与点击外部关闭；
      // 旧/异常响应（有价无明细）保持原样不崩溃。
      if (!bd) return trigger();
      const open = openKey.value === key;
      const openIt = () => {
        openKey.value = key;
      };
      const closeIt = () => {
        if (openKey.value === key) openKey.value = null;
      };
      return h(
        NTooltip,
        {
          trigger: "manual",
          placement: "left",
          show: open,
          style: "max-width: 480px",
          onClickoutside: closeIt,
        },
        {
          trigger: () =>
            h(
              "span",
              {
                style: "cursor: help",
                tabindex: 0,
                role: "button",
                "aria-label": "费用计算明细",
                "aria-expanded": open,
                class: "ts-focusable",
                onClick: openIt,
                onKeydown: (e: KeyboardEvent) => {
                  if (e.key === "Escape") closeIt();
                },
                onMouseenter: openIt,
                onMouseleave: closeIt,
                onFocus: openIt,
                onBlur: closeIt,
              },
              fmtPrice(c as number),
            ),
          default: () =>
            h(
              "div",
              { class: "cost-tooltip" },
              formatCostBreakdownRows(bd).map((row2) => {
                if (row2.divider) return h("div", { class: "bd-divider" });
                if (row2.detail != null) {
                  return h(
                    "div",
                    { class: ["bd-row", row2.unknown ? "bd-unknown" : ""] },
                    `${row2.label} ${row2.detail}`,
                  );
                }
                return h(
                  "div",
                  {
                    class: [
                      "bd-row",
                      row2.unknown ? "bd-unknown" : "",
                      row2.total ? "bd-total" : "",
                    ],
                  },
                  [
                    h("span", { class: "bd-label" }, row2.label),
                    h("span", { class: "bd-value ts-num" }, row2.value ?? ""),
                  ],
                );
              }),
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
  <section class="ts-card events-card">
    <div class="card-head">
      <span class="card-title">请求明细</span>
      <!-- 任务 5：下钻筛选标签（可关闭，键盘可达） -->
      <button
        v-if="filterClosable && props.filterLabel"
        type="button"
        class="filter-chip ts-focusable"
        :aria-label="`清除筛选 ${props.filterLabel}`"
        @click="emit('clear-filter')"
      >
        {{ props.filterLabel }}
        <span aria-hidden="true" class="chip-close">×</span>
      </button>
    </div>
    <NDataTable
      :columns="columns"
      :data="props.list.rows"
      :row-key="rowKey"
      size="small"
      :bordered="false"
      :max-height="380"
      virtual-scroll
    />
    <div class="table-hint">
      共 {{ fmtNum(props.list.total) }} 条（时间倒序）· 已加载
      {{ fmtNum(props.list.rows.length) }} 条
      <template v-if="props.filterLabel">· 筛选：{{ props.filterLabel }}</template>
    </div>
    <div v-if="props.more" class="load-more-row">
      <NButton size="tiny" :loading="props.moreLoading" @click="emit('load-more')">
        加载更多（还剩 {{ fmtNum(remaining()) }} 条）
      </NButton>
    </div>
  </section>
</template>

<style scoped>
/* 卡头：标题 17px/600 + 右侧筛选标签（DESIGN.md §5 卡片头部） */
.card-head {
  display: flex;
  align-items: center;
  gap: var(--ts-space-3);
  margin-bottom: var(--ts-space-2);
  flex-wrap: wrap;
}
.card-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.filter-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--ts-space-1);
  border: none;
  background: var(--ts-accent-soft);
  color: var(--ts-accent);
  font: inherit;
  font-size: 12px;
  border-radius: var(--ts-radius-pill);
  padding: 2px var(--ts-space-2);
  cursor: pointer;
}

.chip-close {
  font-size: 14px;
  line-height: 1;
}

.table-hint {
  font-size: 12px;
  line-height: 1.5;
  color: var(--ts-text-muted);
  margin-top: var(--ts-space-1);
}

.load-more-row {
  text-align: center;
  margin-top: var(--ts-space-2);
}

.cost-tooltip {
  font-size: 12px;
  line-height: 1.7;
  text-align: left;
  min-width: 320px;
}

.bd-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--ts-space-3);
  white-space: normal;
}

.bd-label {
  color: var(--ts-text-secondary);
  flex: 0 0 auto;
}

.bd-value {
  font-family: var(--ts-font-mono);
  text-align: right;
  color: var(--ts-text);
}

.bd-row.bd-unknown .bd-label,
.bd-row.bd-unknown .bd-value {
  color: var(--ts-warning);
}

.bd-row.bd-total .bd-value {
  font-weight: 700;
}

.bd-divider {
  height: 1px;
  background: var(--ts-separator);
  margin: var(--ts-space-1) 0;
}
</style>
