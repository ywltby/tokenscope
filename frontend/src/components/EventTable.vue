<script setup lang="ts">
import { computed, h, ref } from "vue";
import { NButton, NDataTable, NTag, NTooltip, type DataTableColumn } from "naive-ui";
import { AGENT_LABEL, fmtNum, projectLabel, type EventList } from "../types";
import { formatMoney } from "../lib/formatMoney";
import { tokenBucketLabel } from "../lib/tokenDisplay";
import CostBreakdownTooltip from "./CostBreakdownTooltip.vue";

const props = defineProps<{
  list: EventList;
  filterLabel: string;
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
function closeRowTip(key: string): void {
  if (hoverKey.value === key) hoverKey.value = null;
  if (focusKey.value === key) focusKey.value = null;
  if (pinnedKey.value === key) pinnedKey.value = null;
}
const remaining = () => props.list.total - props.list.rows.length;

// UX07：请求金额走单一入口（含 $、微小非零保护）；列头不再带 $。
function fmtPrice(v: number): string {
  return formatMoney(v, "request");
}

const columns = computed<DataTableColumn[]>(() => [
  { title: "时间", key: "ts", minWidth: 150, className: "ts-num" },
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
    minWidth: 120,
    // Claude 项目目录名是压成一段的长路径串：单行省略，悬浮/聚焦可见完整值。
    ellipsis: { tooltip: true },
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
    title: tokenBucketLabel("cache_write"),
    key: "cache_write",
    align: "right",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).cache_write),
  },
  {
    title: tokenBucketLabel("cache_read"),
    key: "cache_read",
    align: "right",
    className: "ts-num",
    render: (r) => fmtNum(asRow(r).cache_read),
  },
  {
    title: "费用（估算）",
    key: "cost_usd",
    align: "right",
    className: "ts-num",
    render: (r) => {
      const row = asRow(r);
      const c = row.cost_usd;
      const bd = row.cost_breakdown;
      const key = row.cursor;
      if (c == null) {
        // 未知价格：明确状态标签，无浮层（不强转 null 成数字）。
        return h(
          NTag,
          { size: "small", bordered: false, type: "warning" },
          { default: () => "未知" },
        );
      }
      // 设计系统 Task 6 / UX03：breakdown 存在时展示"事实→公式→结果→
      // 来源"（提取为 CostBreakdownTooltip 组件——可访问交互与真实浮层
      // 测试内聚）；旧/异常响应（有价无明细）保持金额原样不崩溃。
      if (!bd) return h("span", { class: "ts-num" }, fmtPrice(c));
      const open = openKey.value === key;
      return h(CostBreakdownTooltip, {
        cost: c,
        breakdown: bd,
        open,
        // 任务 6：elevated 玻璃浮层——480px 上限 + 16px 模糊 + 12px 圆角
        //（背景色来自 naiveTheme Tooltip.color = --ts-surface-elevated 85%）
        // UX01：盒模型按 border-box 计算，长文本换行；外框宽度受视口约束
        tooltipStyle:
          "box-sizing: border-box; max-width: min(480px, calc(100vw - 32px)); backdrop-filter: var(--ts-glass-blur-popover); -webkit-backdrop-filter: var(--ts-glass-blur-popover); border-radius: var(--ts-radius-popover);",
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
          if (pinnedKey.value === key) {
            pinnedKey.value = null;
          } else {
            pinnedKey.value = key;
          }
        },
        onEscape: () => closeRowTip(key),
      });
    },
  },
]);

// 设计系统 Task 5：暴露列定义供组件测试断言（无行渲染环境）。
defineExpose({ columns });

type EventRowT = EventList["rows"][number];
const asRow = (r: object): EventRowT => r as unknown as EventRowT;

// R10：行身份 = 后端唯一游标（完整精度 UTC 时间 + record_id），不再用
// 展示时间拼串——同秒同会话的两条请求有不同 cursor，行身份必须互异
//（同秒请求共用费用浮层状态即因旧拼法冲突）。
const rowKey = (r: object): string => asRow(r).cursor;
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
      :scroll-x="1180"
      virtual-scroll
    />
    <div class="table-hint">
      共 {{ fmtNum(props.list.total) }} 条（时间倒序）· 已加载
      {{ fmtNum(props.list.rows.length) }} 条
      <template v-if="props.filterLabel">· 筛选：{{ props.filterLabel }}</template>
    </div>
    <div v-if="props.more" class="load-more-row">
      <NButton
        size="tiny"
        :loading="props.moreLoading"
        :disabled="!!props.moreBlockedHint"
        :title="props.moreBlockedHint"
        @click="emit('load-more')"
      >
        加载更多（还剩 {{ fmtNum(remaining()) }} 条）
      </NButton>
      <span v-if="props.moreBlockedHint" class="load-more-hint" role="status">
        {{ props.moreBlockedHint }}
      </span>
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
