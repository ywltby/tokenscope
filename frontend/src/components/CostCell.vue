<script setup lang="ts">
// v0.9.1 缺陷 B + C：费用列独立组件。
//
// 缺陷 B（2026-11-14）：NDataTable 的 render 回调创建的节点不带 UsageTable
// 的 scope 标记，`<style scoped>` 的 .cost-cell/.source-cost-cell 选择器
// 永不命中 → 真实浏览器里 computed display 是 inline，主金额与「CCS 来源
// 记录费用」排成一行。抽成独立 SFC 后 scope 天然生效，样式回归组件自己的
// 节点；不用全局 span 样式修补。
//
// 缺陷 C（2026-11-14，用户展示规则）：把「没有请求级估算」「来源已提供费
// 用」「模型真正缺价」分开——
//   规则1 cost_detail=rollup_only 且有来源费用：主读数 = 来源金额 + 「来源
//         记录」标识，不并列「未知†」（来源值就是该行的费用读数）；
//   规则2 来源显式 0：保留 $0.00 + 来源标识——不宣称模型免费；
//   规则3 cost_detail=mixed：请求级估算与来源费用分别标明，不静默混合；
//   规则4 缺少费用依据的部分照旧如实披露：rollup_only 无来源费用维持未知†，
//         纯明细缺价维持未知†；「来源未提供费用」不说成「模型没有价格」；
//   规则5 不靠清零 unknown_pricing 掩盖真缺价——unknown_tokens 列与
//         SummaryCards 胶囊仍按后端口径显示。
// 规则6/7 由后端保证（不伪造请求字段；来源费用只累计被采用的汇总）。
import { computed } from "vue";
import type { Group } from "../types";
import { formatMoney } from "../lib/formatMoney";
import { useT } from "../lib/i18n";

const t = useT();

const props = defineProps<{ group: Group }>();

/// 完全未计价（缺价 ≠ 免费，G-P3-1）：显示「未知†」而不是 $0.00。
const unknownOnly = computed(() => props.group.unknown_pricing && props.group.cost_usd === 0);
const mainText = computed(() =>
  unknownOnly.value
    ? t("dashboard.costUnknown")
    : formatMoney(props.group.cost_usd, "summary") + (props.group.unknown_pricing ? "†" : ""),
);

/// 规则1：纯日汇总组以来源记录费用作为该行的费用读数——没有请求级估算，
/// 「未知†」不并列展示；来源未提供费用时回退未知（规则4）。
const sourceAsPrimary = computed(
  () => props.group.cost_detail === "rollup_only" && props.group.source_cost_usd != null,
);

// 8.2（2026-11-14 第二轮复核）：来源费用的覆盖完整性——已知合计
// （source_cost_usd）与「是否覆盖全部采用的汇总」（source_cost_state）是
// 两件事。partial = 部分汇总未提供费用：金额照常显示，但必须披露覆盖
// 不完整（悬停可见缺失原因）；full = 全部提供（含全部显式 0，正常
// $0.00）；absent = 全部缺失（规则 4 的「来源未提供费用」）。
const sourceState = computed(() => props.group.source_cost_state ?? null);
const sourcePartial = computed(() => sourceState.value === "partial");
const sourceTagText = computed(() =>
  sourcePartial.value ? t("dashboard.sourceCostPartialTag") : t("dashboard.sourceCostTag"),
);
const sourceTagTitle = computed(() =>
  sourcePartial.value ? t("dashboard.sourceCostPartialHint") : t("dashboard.sourceCostHint"),
);

const sourceText = computed(() => {
  const v = props.group.source_cost_usd;
  return v == null ? null : formatMoney(v, "summary");
});
</script>

<template>
  <span class="cost-cell">
    <!-- 规则1：来源记录费用就是主读数（纯日汇总组）；partial 时标识升级为
         「部分来源记录」并带覆盖缺失说明（8.2） -->
    <template v-if="sourceAsPrimary">
      <span class="cost-main source-primary ts-num">
        <span class="source-amount">{{ sourceText }}</span>
        <span
          class="ts-pill"
          :class="sourcePartial ? 'ts-pill-warning source-pill' : 'ts-pill-info source-pill'"
          :title="sourceTagTitle"
        >
          {{ sourceTagText }}
        </span>
      </span>
    </template>
    <template v-else>
      <!-- 主读数：请求级估算或未知† -->
      <span
        class="cost-main"
        :style="props.group.unknown_pricing ? 'color: var(--ts-warning)' : ''"
      >
        {{ mainText }}
        <span
          v-if="props.group.cache_write_split_estimated"
          class="ts-pill ts-pill-warning"
          :title="t('dashboard.tierEstimatedTitle')"
        >
          {{ t("dashboard.tierEstimated") }}
        </span>
      </span>
      <!-- 规则3：mixed（估算 + 来源费用并存）→ 来源费用独立一行，归属清晰；
           partial 时标签升级为「部分 CCS 来源记录费用」并带覆盖说明（8.2） -->
      <span
        v-if="props.group.source_cost_usd != null"
        class="source-cost-cell"
        :title="sourceTagTitle"
      >
        <span class="source-cost-label">{{
          sourcePartial ? t("dashboard.sourceCostPartialLabel") : t("dashboard.sourceCostLabel")
        }}</span>
        <span class="source-cost-value ts-num">{{ sourceText }}</span>
      </span>
    </template>
  </span>
</template>

<style scoped>
/* 费用列：上下两行（主读数 / 来源费用），不再是拼接一行（缺陷 B）。 */
.cost-cell {
  display: inline-flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  min-width: 0;
}
.cost-main {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
}
/* 规则1：来源记录读数——金额 + 「来源记录」小标识。 */
.source-primary {
  color: var(--ts-text);
}
.source-amount {
  font-weight: 600;
}
.source-pill {
  font-size: 11px;
  line-height: 1.4;
}
/* 规则3：来源费用独立行——「标签 + 金额」成对不拆行，整行允许在列内换行。 */
.source-cost-cell {
  display: inline-flex;
  align-items: baseline;
  gap: 4px;
  font-size: 12px;
  font-weight: 400;
  color: var(--ts-text-secondary);
  white-space: nowrap;
}
.source-cost-label {
  color: var(--ts-text-muted);
}
.source-cost-value {
  color: var(--ts-text);
  font-weight: 600;
}
</style>
