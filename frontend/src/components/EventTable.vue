<script setup lang="ts">
import { computed, h, ref } from "vue";
import { NDataTable, NTag, NTooltip, type DataTableColumn } from "naive-ui";
import {
  AGENT_LABEL,
  fmtNum,
  projectLabel,
  type EventList,
  type Group,
  type RollupRowMeta,
} from "../types";
import { formatMoney } from "../lib/formatMoney";
import { stableEventRowKey } from "../lib/eventRowKey";
import { tokenBucketLabel } from "../lib/tokenDisplay";
import { totalInput } from "../lib/tokenUsage";
import CostBreakdownTooltip from "./CostBreakdownTooltip.vue";
import GroupCostBreakdownTooltip from "./GroupCostBreakdownTooltip.vue";
import ScrollList from "./ScrollList.vue";
import { useT } from "../lib/i18n";

const t = useT();

const props = defineProps<{
  list: EventList;
  filterLabel: string;
  /** 筛选重置身份：来源/时间/日粒度/时区/下钻变化时才回顶部，同筛选刷新保持滚动与行锚点。 */
  resetKey?: string | null;
  /** D1 游标分页：还有未加载的行 */
  more?: boolean;
  moreLoading?: boolean;
  /**
   * RC02：还有余量但当前**没有分页资格**（恢复视图 / 新批次首页尚未成功）
   * 时的说明。非空 → 按钮禁用并显示该原因，避免"点了没反应"。
   */
  moreBlockedHint?: string;
  /** 任务 5：下钻筛选标签可关闭（由 Dashboard 传当前是否处于下钻态） */
  filterClosable?: boolean;
}>();
const emit = defineEmits<{ (e: "load-more"): void; (e: "clear-filter"): void }>();
/// 设计系统 Task 6：当前展开的费用 tooltip（行 cursor 作为键）。
// UX03：费用浮层三态（hover/focus/pinned）——hover 随指针、focus 随
// 键盘焦点（focus 在时鼠标离开不关闭）、click/Enter/Space 固定切换；
// Escape 一律关闭。openKey = 当前应显示的行。
const hoverKey = ref<string | null>(null);
const focusKey = ref<string | null>(null);
const pinnedKey = ref<string | null>(null);
const openKey = computed(() => hoverKey.value ?? focusKey.value ?? pinnedKey.value);
// 合并后的原始模型 ID 也必须可由键盘读取，不只支持 tooltip 默认的 hover。
const modelHoverKey = ref<string | null>(null);
const modelFocusKey = ref<string | null>(null);
function closeRowTip(key: string): void {
  if (hoverKey.value === key) hoverKey.value = null;
  if (focusKey.value === key) focusKey.value = null;
  if (pinnedKey.value === key) pinnedKey.value = null;
}

// UX07：请求金额走单一入口（含 $、微小非零保护）；列头不再带 $。
function fmtPrice(v: number): string {
  return formatMoney(v, "request");
}

/// 费用浮层三态事件接线（请求级与日汇总行共用同一套 openKey 状态）。
function tipHandlers(key: string) {
  return {
    "onHover-enter": () => {
      hoverKey.value = key;
    },
    "onHover-leave": () => {
      hoverKey.value = null;
    },
    "onFocus-enter": () => {
      focusKey.value = key;
    },
    "onFocus-leave": () => {
      focusKey.value = null;
    },
    onToggle: () => {
      pinnedKey.value = pinnedKey.value === key ? null : key;
    },
    onEscape: () => closeRowTip(key),
  };
}

/// v0.9.4：日汇总行复用分组费用浮层——元数据映射为伪 Group。金额、构成与
/// 缺失部分由后端同一冻结快照的 rollup_cost_parts 产出，与聚合行同源。
function rollupGroup(row: EventRowT, m: RollupRowMeta): Group {
  return {
    key: row.model,
    label: m.label,
    requests: m.requests,
    tokens: {
      input: row.input,
      output: row.output,
      cache_write: row.cache_write,
      cache_read: row.cache_read,
    },
    cost_usd: m.cost_usd,
    unknown_pricing: m.unknown_pricing,
    cache_write_split_estimated: m.cache_write_split_estimated,
    cache_price_zero_estimated: m.cache_price_zero_estimated,
    unknown_tokens: m.unknown_tokens,
    unknown_reason: m.unknown_reason,
    source_cost_usd: m.source_cost_usd,
    cost_detail: m.cost_detail,
    source_cost_state: m.source_cost_state,
    total_cost_usd: m.total_cost_usd,
    has_estimated_cost: m.has_estimated_cost,
    cost_partially_unknown: m.cost_partially_unknown,
    agents: [],
  };
}

const columns = computed<DataTableColumn[]>(() => [
  {
    title: t("dashboard.colTime"),
    key: "ts",
    className: "ts-num",
    // v0.9.4：日汇总行只到日粒度（后端 ts 即统计日，不补造时分秒）；
    // 被汇总覆盖的逐请求行加标记，悬浮说明用量已计入当日汇总。
    render: (r) => {
      const row = asRow(r);
      if (row.rollup) {
        return h("span", { class: "ts-rollup-time" }, [
          row.ts,
          h(
            NTag,
            { size: "small", bordered: false, type: "info" },
            { default: () => t("dashboard.rollupRowTag") },
          ),
        ]);
      }
      const day = row.covered_by_rollup;
      if (day) {
        return h(NTooltip, null, {
          trigger: () =>
            h(
              "span",
              { class: "ts-rollup-time ts-focusable", style: "cursor: help", tabindex: 0 },
              [
                row.ts,
                h(
                  NTag,
                  { size: "small", bordered: false },
                  { default: () => t("dashboard.rollupCoveredTag") },
                ),
              ],
            ),
          default: () => t("dashboard.rollupCoveredTip", { day }),
        });
      }
      return row.ts;
    },
  },
  {
    title: t("dashboard.colAgent"),
    key: "agent",
    render: (r) => {
      const row = asRow(r);
      // v0.9.4：日汇总行的应用列 = 逻辑来源名（ccs → CCS）。
      if (row.rollup) return row.rollup.source.toUpperCase();
      return AGENT_LABEL[row.agent] ?? row.agent;
    },
  },
  {
    title: t("dashboard.colModel"),
    key: "model",
    // 设计系统 Task 5：模型名可聚焦获取完整值（不只 hover）
    render: (r) => {
      const row = asRow(r);
      const m = row.rollup;
      // v0.9.4：日汇总行显示可信展示名 + 真实请求数；悬浮可见合并的
      // 渠道模型与来源统计时区（同日多渠道汇总合并为一行）。
      if (m) {
        const content = t("dashboard.rollupModelsTip", {
          models: m.models.join(", "),
          tz: m.source_tz,
        });
        const key = rowKey(row);
        return h(
          NTooltip,
          {
            style: "white-space: pre-line",
            trigger: "manual",
            show: modelHoverKey.value === key || modelFocusKey.value === key,
          },
          {
            trigger: () =>
              h(
                "span",
                {
                  tabindex: 0,
                  class: "ts-focusable",
                  style: "cursor: help",
                  "aria-label": `${m.label} · ${content}`,
                  onMouseenter: () => {
                    modelHoverKey.value = key;
                  },
                  onMouseleave: () => {
                    if (modelHoverKey.value === key) modelHoverKey.value = null;
                  },
                  onFocus: () => {
                    modelFocusKey.value = key;
                  },
                  onBlur: () => {
                    if (modelFocusKey.value === key) modelFocusKey.value = null;
                  },
                  onKeydown: (e: KeyboardEvent) => {
                    if (e.key === "Escape") {
                      modelHoverKey.value = null;
                      modelFocusKey.value = null;
                    }
                  },
                },
                [
                  m.label,
                  h(
                    "span",
                    { class: "ts-rollup-requests" },
                    ` · ${t("dashboard.rollupRequests", { n: fmtNum(m.requests) })}`,
                  ),
                ],
              ),
            default: () => content,
          },
        );
      }
      return h("span", { tabindex: 0, class: "ts-focusable" }, row.model);
    },
  },
  {
    title: t("dashboard.colProject"),
    key: "project",
    // 长项目名允许换行；悬浮/聚焦可见完整路径。
    // C2：明细项目显示末段（身份是完整路径），悬浮可见完整值。
    render: (r) => {
      const row = asRow(r);
      // v0.9.4：日汇总无项目归属，不补造——显示占位符。
      if (row.rollup) return "—";
      const p = row.project;
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
    title: t("dashboard.colInput"),
    key: "input",
    align: "center",
    className: "ts-num",
    render: (r) => fmtNum(totalInput(asRow(r))),
  },
  {
    title: t("dashboard.colOutput"),
    key: "output",
    align: "center",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).output),
  },
  {
    title: tokenBucketLabel("cache_write"),
    key: "cache_write",
    align: "center",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).cache_write),
  },
  {
    title: tokenBucketLabel("cache_read"),
    key: "cache_read",
    align: "center",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).cache_read),
  },
  {
    title: t("dashboard.colCost"),
    key: "cost_usd",
    align: "center",
    className: "ts-num",
    render: (r) => {
      const row = asRow(r);
      const c = row.cost_usd;
      const bd = row.cost_breakdown;
      const key = rowKey(row);
      const m = row.rollup;
      // v0.9.4：日汇总行走分组费用浮层（构成/缺失与聚合行同一依据）；
      // 无费用依据时显示「未知」，仍可打开浮层查看缺项。
      if (m) {
        return h(GroupCostBreakdownTooltip, {
          group: rollupGroup(row, m),
          displayText: c == null ? t("dashboard.unknownTag") : fmtPrice(c),
          unknownOnly: c == null,
          open: openKey.value === key,
          pinned: pinnedKey.value === key,
          ...tipHandlers(key),
        });
      }
      if (c == null) {
        // 未知价格：明确状态标签，无浮层（不强转 null 成数字）。
        return h(
          NTag,
          { size: "small", bordered: false, type: "warning" },
          { default: () => t("dashboard.unknownTag") },
        );
      }
      // 设计系统 Task 6 / UX03：breakdown 存在时展示"事实→公式→结果→
      // 来源"（提取为 CostBreakdownTooltip 组件——可访问交互与真实浮层
      // 测试内聚）；旧/异常响应（有价无明细）保持金额原样不崩溃。
      if (!bd) return h("span", { class: "ts-num" }, fmtPrice(c));
      return h(CostBreakdownTooltip, {
        cost: c,
        breakdown: bd,
        open: openKey.value === key,
        // RC06：外部点击只对**固定态**生效，由本层清除 pinnedKey。
        pinned: pinnedKey.value === key,
        // 任务 6：elevated 玻璃浮层——480px 上限 + 16px 模糊 + 12px 圆角
        //（背景色来自 naiveTheme Tooltip.color = --ts-surface-elevated 85%）
        // UX01：盒模型按 border-box 计算，长文本换行；外框宽度受视口约束
        tooltipStyle:
          "box-sizing: border-box; max-width: min(480px, calc(100vw - 32px)); backdrop-filter: var(--ts-glass-blur-popover); -webkit-backdrop-filter: var(--ts-glass-blur-popover); border-radius: var(--ts-radius-popover);",
        ...tipHandlers(key),
      });
    },
  },
]);

// 设计系统 Task 5：暴露列定义供组件测试断言（无行渲染环境）。
defineExpose({ columns });

type EventRowT = EventList["rows"][number];
const asRow = (r: object): EventRowT => r as unknown as EventRowT;

// R10 + 跨刷新稳定锚点：行身份使用稳定 row_id（原生请求 = event:<id>，
// CCS 日汇总 = rollup 四元组），与会话绑定的分页游标 row.cursor 解耦；
// 同秒同会话的两条请求仍互异，后台刷新新建 query_id 时行身份与滚动锚点不漂移。
const rowKey = (r: object): string => stableEventRowKey(asRow(r));
</script>

<template>
  <section class="ts-card events-card">
    <div class="card-head">
      <span class="card-title">{{ t("dashboard.eventsTitle") }}</span>
      <!-- 任务 5：下钻筛选标签（可关闭，键盘可达） -->
      <button
        v-if="filterClosable && props.filterLabel"
        type="button"
        class="filter-chip ts-focusable"
        :aria-label="t('dashboard.clearFilterAria', { label: props.filterLabel })"
        @click="emit('clear-filter')"
      >
        {{ props.filterLabel }}
        <span aria-hidden="true" class="chip-close">×</span>
      </button>
    </div>
    <ScrollList
      :rows="props.list.rows"
      :total="props.list.total"
      :more="more"
      :loading="moreLoading"
      :blocked="moreBlockedHint"
      :reset-key="props.resetKey"
      :row-key="rowKey"
      :label="t('dashboard.eventsListAria')"
      class="ts-auto-table-events"
      @load-more="emit('load-more')"
    >
      <template #default="{ rows }">
        <NDataTable
          :columns="columns"
          :data="rows"
          :row-key="rowKey"
          size="small"
          :bordered="false"
          table-layout="auto"
          class="ts-auto-table"
        />
      </template>
    </ScrollList>
    <div class="table-hint">
      {{
        t("dashboard.eventsCount", {
          total: fmtNum(props.list.total),
          loaded: fmtNum(props.list.rows.length),
        })
      }}
      <template v-if="props.filterLabel"
        >· {{ t("dashboard.filterPrefix") }}{{ props.filterLabel }}</template
      >
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
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--ts-space-2);
  margin-top: var(--ts-space-2);
}

/* RC02：无分页资格时的原因说明（禁用按钮旁可见，不只靠 title） */
.load-more-hint {
  font-size: 12px;
  color: var(--ts-text-muted);
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

/* 公式区实色衬底（.ts-card-solid 提供背景），数字等宽右对齐 */
.bd-formula {
  padding: var(--ts-space-2) var(--ts-space-3);
  margin: var(--ts-space-1) 0;
  border-radius: var(--ts-radius-control);
}
</style>

<style>
/* v0.9.4：列 render 用 h() 创建的节点不带 SFC scope 属性（与
   GroupCostBreakdownTooltip 同根因），日汇总行样式用全局作用域 +
   ts-rollup- 前缀类名隔离。 */
.ts-rollup-time {
  display: inline-flex;
  gap: var(--ts-space-1);
  align-items: center;
}
.ts-rollup-requests {
  color: var(--ts-text-muted);
  font-size: 12px;
}
</style>
